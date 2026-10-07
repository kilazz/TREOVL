use anyhow::{Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read};

use super::{
    build_chunk_from_elements, build_typed_container, parse_chunk_elements, parse_typed_container,
};
use crate::engine::common::magic;
use crate::utils::dds_decoder::TextureFormat;
use crate::utils::dds_encoder::generate_dds_header;

pub struct OverlordTexture {
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat,
    pub mip_count: u32,
    pub is_cubemap: bool,
    pub pixel_data: Vec<u8>,
}

/// Parses an Overlord texture chunk (UI image, 2D/3D mipmapped texture, or cubemap).
/// Extracts the entire pixel stream and detects dimensions, formats, and mipmap chains.
pub fn parse_texture_chunk(data: &[u8]) -> Result<OverlordTexture> {
    if data.len() < 4 {
        bail!("Chunk data is too short to be a valid texture.");
    }

    // =========================================================================
    // 1. TGA Interface Image (TypeID: 0x00410098)
    // =========================================================================
    if data.starts_with(magic::TEX_INTERFACE) {
        let mut pos = 4;
        if pos >= data.len() {
            bail!("Truncated TGA chunk header.");
        }
        let num_offsets = data[pos] as usize;
        pos += 1 + num_offsets * 2;
        if pos >= data.len() {
            bail!("Corrupted TGA offset table.");
        }

        let s1_len = u32::from_le_bytes(data[pos..pos + 4].try_into()?) as usize;
        pos += 4 + s1_len;

        let s2_len = u32::from_le_bytes(data[pos..pos + 4].try_into()?) as usize;
        pos += 4 + s2_len;

        let width = u32::from_le_bytes(data[pos..pos + 4].try_into()?);
        let height = u32::from_le_bytes(data[pos + 4..pos + 8].try_into()?);

        let pixel_size = (width * height * 4) as usize;
        if pixel_size == 0 || pixel_size > data.len() {
            bail!("Invalid TGA dimensions: {}x{}", width, height);
        }

        let pixel_data = data[data.len() - pixel_size..].to_vec();

        return Ok(OverlordTexture {
            width,
            height,
            format: TextureFormat::UncompressedRGBA,
            mip_count: 1,
            is_cubemap: false,
            pixel_data,
        });
    }

    let is_cubemap = data.starts_with(magic::TEX_CUBEMAP);

    // =========================================================================
    // 2. Mipmapped 2D/3D Texture (0x0041003D) or Cubemap (0x00410099)
    // =========================================================================
    if data.starts_with(magic::TEX_3D) || is_cubemap {
        if let Ok((_, root_elements)) = parse_typed_container(data)
            && let Some((_, level_1_data)) = root_elements.into_iter().find(|(id, _)| *id == 1)
            && let Ok((_, level_1_elements)) = parse_chunk_elements(&level_1_data)
            && let Some((_, level_2_data)) = level_1_elements.into_iter().find(|(id, _)| *id == 20)
            && let Ok((_, mips)) = parse_chunk_elements(&level_2_data)
            && !mips.is_empty()
        {
            let mut all_pixels = Vec::new();
            let mut first_width = 0;
            let mut first_height = 0;
            let mut first_format = TextureFormat::DXT5;
            let total_mips = mips.len() as u32;

            for (idx, (_, mip_bytes)) in mips.into_iter().enumerate() {
                if let Ok((_, mip_props)) = parse_typed_container(&mip_bytes) {
                    let mut w = 0;
                    let mut h = 0;
                    let mut fmt = 7;
                    let mut pixels = Vec::new();

                    for (pid, pdata) in mip_props {
                        match pid {
                            20 if pdata.len() >= 4 => {
                                w = u32::from_le_bytes(pdata[..4].try_into().unwrap_or_default())
                            }
                            21 if pdata.len() >= 4 => {
                                h = u32::from_le_bytes(pdata[..4].try_into().unwrap_or_default())
                            }
                            23 if pdata.len() >= 4 => {
                                fmt = u32::from_le_bytes(pdata[..4].try_into().unwrap_or_default())
                            }
                            22 => pixels = pdata,
                            _ => {}
                        }
                    }

                    if idx == 0 {
                        first_width = w;
                        first_height = h;
                        first_format = TextureFormat::from_u32(fmt).unwrap_or(TextureFormat::DXT5);
                    }
                    all_pixels.extend(pixels);
                }
            }

            let mip_count = if is_cubemap {
                total_mips / 6
            } else {
                total_mips
            };

            if first_width > 0 && first_height > 0 && !all_pixels.is_empty() {
                return Ok(OverlordTexture {
                    width: first_width,
                    height: first_height,
                    format: first_format,
                    mip_count,
                    is_cubemap,
                    pixel_data: all_pixels,
                });
            }
        }

        // Resilient binary scanning fallback across 0x00410024 mip chunks
        let mut all_mips_data = Vec::new();
        let mut first_width = 0;
        let mut first_height = 0;
        let mut first_format = TextureFormat::DXT5;
        let mut total_mips_scanned = 0;

        let mut search_pos = 0;
        while search_pos + 4 <= data.len() {
            if let Some(rel) = data[search_pos..]
                .windows(4)
                .position(|w| w == magic::TEX_MIPMAP)
            {
                let mip_start = search_pos + rel;
                let mut pos = mip_start + 4;
                if pos < data.len() {
                    let num_offsets = data[pos] as usize;
                    pos += 1 + num_offsets * 2;

                    if pos + 12 <= data.len() {
                        let width =
                            u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap_or_default());
                        let height = u32::from_le_bytes(
                            data[pos + 4..pos + 8].try_into().unwrap_or_default(),
                        );
                        let fmt_code = u32::from_le_bytes(
                            data[pos + 8..pos + 12].try_into().unwrap_or_default(),
                        );
                        pos += 12;

                        let format =
                            TextureFormat::from_u32(fmt_code).unwrap_or(TextureFormat::DXT5);
                        let block_size = if format == TextureFormat::DXT1 { 8 } else { 16 };
                        let mip_size = if format == TextureFormat::UncompressedRGBA {
                            (width * height * 4) as usize
                        } else if format == TextureFormat::UncompressedRGB {
                            (width * height * 3) as usize
                        } else {
                            (width.div_ceil(4) * height.div_ceil(4) * block_size) as usize
                        };

                        if pos + mip_size <= data.len() {
                            if total_mips_scanned == 0 {
                                first_width = width;
                                first_height = height;
                                first_format = format;
                            }
                            all_mips_data.extend_from_slice(&data[pos..pos + mip_size]);
                            total_mips_scanned += 1;
                            search_pos = pos + mip_size;
                            continue;
                        }
                    }
                }
                search_pos = mip_start + 4;
            } else {
                break;
            }
        }

        if total_mips_scanned > 0 {
            let mip_count = if is_cubemap {
                total_mips_scanned / 6
            } else {
                total_mips_scanned
            };

            return Ok(OverlordTexture {
                width: first_width,
                height: first_height,
                format: first_format,
                mip_count,
                is_cubemap,
                pixel_data: all_mips_data,
            });
        }
    }

    // =========================================================================
    // 3. Raw DDS Container Fallback
    // =========================================================================
    if data.starts_with(b"DDS ") && data.len() >= 128 {
        let height = u32::from_le_bytes(data[12..16].try_into()?);
        let width = u32::from_le_bytes(data[16..20].try_into()?);
        let mip_count = u32::from_le_bytes(data[28..32].try_into()?).max(1);
        let fourcc = &data[84..88];
        let rgb_bit_count = u32::from_le_bytes(data[88..92].try_into().unwrap_or_default());

        let format = match fourcc {
            b"DXT1" => TextureFormat::DXT1,
            b"DXT3" => TextureFormat::DXT3,
            b"DXT5" => TextureFormat::DXT5,
            _ => {
                if rgb_bit_count == 24 {
                    TextureFormat::UncompressedRGB
                } else {
                    TextureFormat::UncompressedRGBA
                }
            }
        };

        let caps2 = u32::from_le_bytes(data[112..116].try_into()?);
        let is_cubemap = (caps2 & 0x200) != 0;

        return Ok(OverlordTexture {
            width,
            height,
            format,
            mip_count,
            is_cubemap,
            pixel_data: data[128..].to_vec(),
        });
    }

    bail!("Could not detect texture dimensions or valid pixel payload in chunk.")
}

/// Exports any texture chunk to a standard DirectDraw Surface (DDS) or TGA file.
/// Preserves all mipmap levels and cubemap face layouts in the exported DDS header.
pub fn export_to_dds(data: &[u8]) -> Result<Vec<u8>> {
    let tex = parse_texture_chunk(data)?;

    if data.starts_with(magic::TEX_INTERFACE) {
        let mut tga_file = Vec::with_capacity(18 + tex.pixel_data.len());
        tga_file.push(0);
        tga_file.push(0);
        tga_file.push(2);
        tga_file.extend_from_slice(&[0, 0, 0, 0, 0]);
        tga_file.extend_from_slice(&[0, 0, 0, 0]);
        tga_file.extend_from_slice(&(tex.width as u16).to_le_bytes());
        tga_file.extend_from_slice(&(tex.height as u16).to_le_bytes());
        tga_file.push(32);
        tga_file.push(8);

        let row_len = (tex.width * 4) as usize;
        for y in (0..tex.height as usize).rev() {
            let row_start = y * row_len;
            let row_end = (row_start + row_len).min(tex.pixel_data.len());
            if row_start < tex.pixel_data.len() {
                tga_file.extend_from_slice(&tex.pixel_data[row_start..row_end]);
            }
        }
        return Ok(tga_file);
    }

    let header = generate_dds_header(
        tex.width,
        tex.height,
        tex.format,
        tex.mip_count,
        tex.is_cubemap,
    );
    let mut dds = header;
    dds.extend(tex.pixel_data);
    Ok(dds)
}

/// Injects a new DDS or TGA image into an existing Overlord texture chunk.
pub fn replace_texture_in_chunk(chunk_data: &[u8], input_image: &[u8]) -> Result<Vec<u8>> {
    // =========================================================================
    // 1. TGA Interface Image Replacement
    // =========================================================================
    if chunk_data.starts_with(magic::TEX_INTERFACE) {
        let (width, height, is_top_left, pixel_data) =
            if input_image.starts_with(b"DDS ") && input_image.len() >= 128 {
                let mut c = Cursor::new(input_image);
                c.set_position(12);
                let h = c.read_u32::<LittleEndian>()?;
                let w = c.read_u32::<LittleEndian>()?;
                (w, h, true, &input_image[128..])
            } else if input_image.len() >= 18 {
                let w = u16::from_le_bytes(input_image[12..14].try_into()?) as u32;
                let h = u16::from_le_bytes(input_image[14..16].try_into()?) as u32;
                let descriptor = input_image[17];
                let top_left = (descriptor & 0x20) != 0;
                (w, h, top_left, &input_image[18..])
            } else {
                bail!("Invalid image file selected for TGA chunk.");
            };

        let mut final_pixels = Vec::with_capacity(pixel_data.len());
        let row_len = (width * 4) as usize;

        if is_top_left {
            final_pixels.extend_from_slice(pixel_data);
        } else {
            for y in (0..height as usize).rev() {
                let row_start = y * row_len;
                let row_end = row_start + row_len;
                if row_end <= pixel_data.len() {
                    final_pixels.extend_from_slice(&pixel_data[row_start..row_end]);
                }
            }
        }

        let mut pos = 4;
        let num_offsets = chunk_data[pos] as usize;
        pos += 1 + num_offsets * 2;
        let s1_len = u32::from_le_bytes(chunk_data[pos..pos + 4].try_into()?) as usize;
        pos += 4 + s1_len;
        let s2_len = u32::from_le_bytes(chunk_data[pos..pos + 4].try_into()?) as usize;
        pos += 4 + s2_len;

        let old_width = u32::from_le_bytes(chunk_data[pos..pos + 4].try_into()?);
        let old_height = u32::from_le_bytes(chunk_data[pos + 4..pos + 8].try_into()?);
        let old_pixel_size = (old_width * old_height * 4) as usize;
        let padding_end = chunk_data.len().saturating_sub(old_pixel_size);
        let unknown_padding = if padding_end > pos + 8 {
            &chunk_data[pos + 8..padding_end]
        } else {
            &[]
        };

        let mut out = chunk_data[..pos].to_vec();
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(unknown_padding);
        out.extend_from_slice(&final_pixels);
        return Ok(out);
    }

    let is_cubemap_target = chunk_data.starts_with(magic::TEX_CUBEMAP);

    // =========================================================================
    // 2. Mipmapped 2D/3D Texture (0x0041003D) or Cubemap (0x00410099)
    // =========================================================================
    if chunk_data.starts_with(magic::TEX_3D) || is_cubemap_target {
        if input_image.len() < 128 || &input_image[0..4] != b"DDS " {
            bail!("Invalid DDS file selected (must begin with 'DDS ').");
        }

        let mut c = Cursor::new(input_image);
        c.set_position(12);
        let new_height = c.read_u32::<LittleEndian>()?;
        let new_width = c.read_u32::<LittleEndian>()?;

        c.set_position(28);
        let mut mip_count = c.read_u32::<LittleEndian>().unwrap_or(1);
        if mip_count == 0 {
            mip_count = 1;
        }

        c.set_position(84);
        let mut fourcc = [0u8; 4];
        c.read_exact(&mut fourcc)?;

        c.set_position(88);
        let bpp = c.read_u32::<LittleEndian>().unwrap_or(32);

        let (new_format, block_size) = match &fourcc {
            b"DXT1" => (7u32, 8usize),
            b"DXT3" => (9u32, 16usize),
            b"DXT5" => (11u32, 16usize),
            _ => {
                if bpp == 24 {
                    (3u32, 3usize)
                } else {
                    (5u32, 4usize)
                }
            }
        };

        let (root_type_id, mut root_elements) = parse_typed_container(chunk_data)?;

        // Support full 6-face cubemaps by packing all faces sequentially
        let num_faces = if is_cubemap_target { 6 } else { 1 };
        let mut mip_blocks = Vec::new();
        let mut pixel_pos = 128usize;

        let mut block_counter = 0;
        for _face in 0..num_faces {
            let mut w = new_width;
            let mut h = new_height;

            for _mip_idx in 0..mip_count {
                let mip_size = if new_format == 5 {
                    (w * h * 4) as usize
                } else if new_format == 3 {
                    (w * h * 3) as usize
                } else {
                    (w.div_ceil(4) * h.div_ceil(4) * block_size as u32) as usize
                };

                if pixel_pos + mip_size > input_image.len() {
                    break;
                }

                let mip_pixels = &input_image[pixel_pos..pixel_pos + mip_size];
                let mip_chunk = build_typed_container(
                    0x00410024,
                    &[
                        (20, w.to_le_bytes().to_vec()),
                        (21, h.to_le_bytes().to_vec()),
                        (23, new_format.to_le_bytes().to_vec()),
                        (22, mip_pixels.to_vec()),
                    ],
                );

                mip_blocks.push((block_counter, mip_chunk));
                block_counter += 1;

                pixel_pos += mip_size;
                w = (w / 2).max(1);
                h = (h / 2).max(1);
            }
        }

        let level_2_mip_container = build_chunk_from_elements(true, &mip_blocks);
        let level_1_elements = vec![(20, level_2_mip_container), (21, vec![0u8, 0, 0, 0])];
        let level_1_container = build_chunk_from_elements(false, &level_1_elements);

        let mut replaced = false;
        for (id, data) in root_elements.iter_mut() {
            if *id == 1 {
                *data = level_1_container.clone();
                replaced = true;
            }
        }
        if !replaced {
            root_elements.push((1, level_1_container));
            root_elements.sort_by_key(|&(id, _)| id);
        }

        return Ok(build_typed_container(root_type_id, &root_elements));
    }

    bail!("Unsupported texture chunk format.");
}
