use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read};

use super::{build_chunk_from_elements, parse_chunk_elements};
use crate::utils::dds_decoder::TextureFormat;
use crate::utils::dds_encoder::generate_dds_header;

pub struct OverlordTexture {
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat,
    pub is_cubemap: bool,
    pub pixel_data: Vec<u8>,
}

/// Universally parses any Overlord texture chunk (Standard 3D Texture, TGA Interface Image, Cubemap, or MipMap).
pub fn parse_texture_chunk(data: &[u8]) -> Result<OverlordTexture, String> {
    if data.len() < 4 {
        return Err("Chunk data is too short.".into());
    }

    // -------------------------------------------------------------
    // CASE 1: Interface Image (TGA / TIF) — Signature 98 00 41 00
    // -------------------------------------------------------------
    if data.starts_with(b"\x98\x00\x41\x00") {
        let mut pos = 4;
        if pos >= data.len() {
            return Err("Truncated TGA chunk header.".into());
        }
        let num_offsets = data[pos] as usize;
        pos += 1 + num_offsets * 2;
        if pos >= data.len() {
            return Err("Corrupted TGA offset table.".into());
        }

        // Skip String 1 (Pointer)
        if pos + 4 > data.len() {
            return Err("Truncated TGA string table.".into());
        }
        let s1_len = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap_or_default()) as usize;
        pos += 4 + s1_len;

        // Skip String 2 (File Name)
        if pos + 4 > data.len() {
            return Err("Truncated TGA string table.".into());
        }
        let s2_len = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap_or_default()) as usize;
        pos += 4 + s2_len;

        // Read Width & Height
        if pos + 8 > data.len() {
            return Err("Truncated TGA dimensions.".into());
        }
        let width = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap_or_default());
        let height = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap_or_default());

        let pixel_size = (width * height * 4) as usize;
        if pixel_size == 0 || pixel_size > data.len() {
            return Err(format!("Invalid TGA dimensions: {}x{}", width, height));
        }

        let pixel_data = data[data.len() - pixel_size..].to_vec();

        return Ok(OverlordTexture {
            width,
            height,
            format: TextureFormat::UncompressedRGBA,
            is_cubemap: false,
            pixel_data,
        });
    }

    // -------------------------------------------------------------
    // CASE 2: Standard Texture (3D 00 41 00) or Cubemap (99 00 41 00)
    // -------------------------------------------------------------
    let is_cubemap = data.starts_with(b"\x99\x00\x41\x00");

    if let Some(mip_offset) = data.windows(4).position(|w| w == b"\x24\x00\x41\x00") {
        let mut pos = mip_offset + 4;
        if pos < data.len() {
            let num_offsets = data[pos] as usize;
            pos += 1 + num_offsets * 2;

            if pos + 12 <= data.len() {
                let width = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap_or_default());
                let height =
                    u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap_or_default());
                let fmt_code =
                    u32::from_le_bytes(data[pos + 8..pos + 12].try_into().unwrap_or_default());
                pos += 12;

                let format = TextureFormat::from_u32(fmt_code).unwrap_or(TextureFormat::DXT5);

                let end_pos = if is_cubemap {
                    data.len()
                } else {
                    let block_size = if format == TextureFormat::DXT1 { 8 } else { 16 };
                    let mip_size = if format == TextureFormat::UncompressedRGBA {
                        (width * height * 4) as usize
                    } else {
                        (width.div_ceil(4) * height.div_ceil(4) * block_size) as usize
                    };
                    (pos + mip_size).min(data.len())
                };

                let pixel_data = data[pos..end_pos].to_vec();

                return Ok(OverlordTexture {
                    width,
                    height,
                    format,
                    is_cubemap,
                    pixel_data,
                });
            }
        }
    }

    // -------------------------------------------------------------
    // CASE 3: Standalone DDS File ("DDS ")
    // -------------------------------------------------------------
    if data.starts_with(b"DDS ") && data.len() >= 128 {
        let height = u32::from_le_bytes(data[12..16].try_into().unwrap_or_default());
        let width = u32::from_le_bytes(data[16..20].try_into().unwrap_or_default());
        let fourcc = &data[84..88];
        let format = match fourcc {
            b"DXT1" => TextureFormat::DXT1,
            b"DXT3" => TextureFormat::DXT3,
            b"DXT5" => TextureFormat::DXT5,
            _ => TextureFormat::UncompressedRGBA,
        };

        let caps2 = u32::from_le_bytes(data[112..116].try_into().unwrap_or_default());
        let is_cubemap = (caps2 & 0x200) != 0;

        return Ok(OverlordTexture {
            width,
            height,
            format,
            is_cubemap,
            pixel_data: data[128..].to_vec(),
        });
    }

    // -------------------------------------------------------------
    // CASE 4: Standard Sub-Container (ID 20=W, 21=H, 23=Fmt, 22=Data)
    // -------------------------------------------------------------
    if let Ok((_, elements)) = parse_chunk_elements(data) {
        let mut width = 0;
        let mut height = 0;
        let mut format = 7;
        let mut pixel_data = Vec::new();

        for (id, chunk) in elements {
            let mut c = Cursor::new(&chunk);
            match id {
                20 if chunk.len() >= 4 => width = c.read_u32::<LittleEndian>().unwrap_or(0),
                21 if chunk.len() >= 4 => height = c.read_u32::<LittleEndian>().unwrap_or(0),
                23 if chunk.len() >= 4 => format = c.read_u32::<LittleEndian>().unwrap_or(7),
                22 => pixel_data = chunk.to_vec(),
                _ => {}
            }
        }

        if width > 0 && height > 0 && !pixel_data.is_empty() {
            let tex_format = TextureFormat::from_u32(format).unwrap_or(TextureFormat::DXT5);
            return Ok(OverlordTexture {
                width,
                height,
                format: tex_format,
                is_cubemap: false,
                pixel_data,
            });
        }
    }

    Err("Could not detect texture dimensions or valid pixel payload in this chunk.".into())
}

/// Prepends a standard 128-byte DDS header to the raw texture payload.
pub fn export_to_dds(data: &[u8]) -> Result<Vec<u8>, String> {
    let tex = parse_texture_chunk(data)?;

    // If it's a TGA interface image chunk (98 00 41 00), generate an 18-byte TGA file
    if data.starts_with(b"\x98\x00\x41\x00") {
        let mut tga_file = Vec::with_capacity(18 + tex.pixel_data.len());
        tga_file.push(0); // ID length
        tga_file.push(0); // Color Map Type
        tga_file.push(2); // Image Type (Uncompressed True-Color)
        tga_file.extend_from_slice(&[0, 0, 0, 0, 0]); // Color Map Spec
        tga_file.extend_from_slice(&[0, 0, 0, 0]); // X, Y origin
        tga_file.extend_from_slice(&(tex.width as u16).to_le_bytes()); // Width
        tga_file.extend_from_slice(&(tex.height as u16).to_le_bytes()); // Height
        tga_file.push(32); // Bits per pixel
        tga_file.push(8); // Image Descriptor (8 bits alpha)

        // Flip rows vertically for standard bottom-up TGA files
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

    let header = generate_dds_header(tex.width, tex.height, tex.format, 1, tex.is_cubemap);
    let mut dds = header;
    dds.extend(tex.pixel_data);
    Ok(dds)
}

/// Replaces the texture payload inside an existing chunk from a user-supplied image file (DDS or TGA).
pub fn replace_texture_in_chunk(chunk_data: &[u8], input_image: &[u8]) -> Result<Vec<u8>, String> {
    // -------------------------------------------------------------
    // CASE 1: Processing a TGA Interface Image chunk (98 00 41 00)
    // -------------------------------------------------------------
    if chunk_data.starts_with(b"\x98\x00\x41\x00") {
        let (width, height, is_top_left, pixel_data) = if input_image.starts_with(b"DDS ")
            && input_image.len() >= 128
        {
            let mut c = Cursor::new(input_image);
            c.set_position(12);
            let h = c.read_u32::<LittleEndian>().map_err(|e| e.to_string())?;
            let w = c.read_u32::<LittleEndian>().map_err(|e| e.to_string())?;
            (w, h, true, &input_image[128..])
        } else if input_image.len() >= 18 {
            let w = u16::from_le_bytes(input_image[12..14].try_into().unwrap_or_default()) as u32;
            let h = u16::from_le_bytes(input_image[14..16].try_into().unwrap_or_default()) as u32;
            let descriptor = input_image[17];
            let top_left = (descriptor & 0x20) != 0;
            (w, h, top_left, &input_image[18..])
        } else {
            return Err("Invalid image file selected for TGA chunk.".into());
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

        // Rebuild TGA chunk
        let mut pos = 4;
        let num_offsets = chunk_data[pos] as usize;
        pos += 1 + num_offsets * 2;
        let s1_len =
            u32::from_le_bytes(chunk_data[pos..pos + 4].try_into().unwrap_or_default()) as usize;
        pos += 4 + s1_len;
        let s2_len =
            u32::from_le_bytes(chunk_data[pos..pos + 4].try_into().unwrap_or_default()) as usize;
        pos += 4 + s2_len;

        let old_width = u32::from_le_bytes(chunk_data[pos..pos + 4].try_into().unwrap_or_default());
        let old_height =
            u32::from_le_bytes(chunk_data[pos + 4..pos + 8].try_into().unwrap_or_default());
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

    // -------------------------------------------------------------
    // CASE 2: Standard 3D Texture (3D 00 41 00) or Cubemap (99 00 41 00)
    // -------------------------------------------------------------
    if chunk_data.starts_with(b"\x3D\x00\x41\x00") || chunk_data.starts_with(b"\x99\x00\x41\x00") {
        if input_image.len() < 128 || &input_image[0..4] != b"DDS " {
            return Err("Invalid DDS file selected (must begin with 'DDS ').".into());
        }

        let mut c = Cursor::new(input_image);
        c.set_position(12);
        let new_height = c.read_u32::<LittleEndian>().map_err(|e| e.to_string())?;
        let new_width = c.read_u32::<LittleEndian>().map_err(|e| e.to_string())?;

        c.set_position(28);
        let mut mip_count = c.read_u32::<LittleEndian>().unwrap_or(1);
        if mip_count == 0 {
            mip_count = 1;
        }

        c.set_position(84);
        let mut fourcc = [0u8; 4];
        c.read_exact(&mut fourcc).map_err(|e| e.to_string())?;

        let (new_format, block_size) = match &fourcc {
            b"DXT1" => (7u32, 8usize),
            b"DXT3" => (9u32, 16usize),
            b"DXT5" => (11u32, 16usize),
            _ => (5u32, 4usize), // Uncompressed RGBA
        };

        // 1. Extract strings and padding from the original chunk
        let type_id = &chunk_data[0..4];
        let mut pos = 4;
        let num_offsets = chunk_data[pos] as usize;
        pos += 1 + num_offsets * 2;

        let s1_len =
            u32::from_le_bytes(chunk_data[pos..pos + 4].try_into().unwrap_or_default()) as usize;
        let s1_bytes = &chunk_data[pos + 4..pos + 4 + s1_len];
        pos += 4 + s1_len;

        let s2_len =
            u32::from_le_bytes(chunk_data[pos..pos + 4].try_into().unwrap_or_default()) as usize;
        let s2_bytes = &chunk_data[pos + 4..pos + 4 + s2_len];
        pos += 4 + s2_len;

        let mut ff_pad = Vec::new();
        while pos + 4 <= chunk_data.len() && &chunk_data[pos..pos + 4] == b"\xFF\xFF\xFF\xFF" {
            ff_pad.extend_from_slice(b"\xFF\xFF\xFF\xFF");
            pos += 4;
        }

        // 2. Build MipMap blocks (24 00 41 00 sub-containers) from the new DDS file
        let mut mip_blocks = Vec::new();
        let mut mip_offsets = Vec::new();
        let mut current_mip_offset = 0usize;
        let mut pixel_pos = 128usize;

        let mut w = new_width;
        let mut h = new_height;

        for _ in 0..mip_count {
            let mip_size = if new_format == 5 {
                (w * h * 4) as usize
            } else {
                (w.div_ceil(4) * h.div_ceil(4) * block_size as u32) as usize
            };

            if pixel_pos + mip_size > input_image.len() {
                break;
            }

            let mip_pixels = &input_image[pixel_pos..pixel_pos + mip_size];

            // Build Sub-Container: 24 00 41 00 + Table (04 1400 1504 1708 160C) + W + H + Format + Pixels
            let mut mip_chunk = Vec::with_capacity(25 + mip_pixels.len());
            mip_chunk.extend_from_slice(b"\x24\x00\x41\x00");
            mip_chunk.push(4); // 4 small entries
            mip_chunk.extend_from_slice(&[20, 0, 21, 4, 23, 8, 22, 12]);
            mip_chunk.extend_from_slice(&w.to_le_bytes());
            mip_chunk.extend_from_slice(&h.to_le_bytes());
            mip_chunk.extend_from_slice(&new_format.to_le_bytes());
            mip_chunk.extend_from_slice(mip_pixels);

            mip_offsets.push((mip_blocks.len(), current_mip_offset));
            current_mip_offset += mip_chunk.len();
            mip_blocks.push(mip_chunk);

            pixel_pos += mip_size;
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }

        // 3. Assemble the top-level chunk
        let mut out = Vec::new();
        out.extend_from_slice(type_id);

        out.push(4); // 4 table entries in parent container
        out.extend_from_slice(&[20, 0]);
        out.extend_from_slice(&[21, (4 + s1_bytes.len()) as u8]);
        out.extend_from_slice(&[19, (4 + s1_bytes.len() + 4 + s2_bytes.len()) as u8]);
        out.extend_from_slice(&[
            1,
            (4 + s1_bytes.len() + 4 + s2_bytes.len() + ff_pad.len()) as u8,
        ]);

        out.extend_from_slice(&(s1_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(s1_bytes);
        out.extend_from_slice(&(s2_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(s2_bytes);
        out.extend_from_slice(&ff_pad);

        // MipMap TOC Table
        let max_offset = mip_offsets.last().map(|&(_, off)| off).unwrap_or(0);
        if max_offset <= 255 {
            out.push(mip_offsets.len() as u8);
            for (idx, off) in &mip_offsets {
                out.push(*idx as u8);
                out.push(*off as u8);
            }
        } else {
            out.push(0x81); // 1 small + N large entries
            out.extend_from_slice(&((mip_offsets.len().saturating_sub(1)) as u32).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            for (idx, off) in &mip_offsets[1..] {
                out.extend_from_slice(&(*idx as u32).to_le_bytes());
                out.extend_from_slice(&(*off as u32).to_le_bytes());
            }
        }

        for mip in &mip_blocks {
            out.extend_from_slice(mip);
        }

        if new_format == 5 {
            out.extend_from_slice(&[0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        } else {
            out.extend_from_slice(&[0, 0, 0, 0]);
        }

        return Ok(out);
    }

    // -------------------------------------------------------------
    // CASE 3: Fallback for generic sub-containers
    // -------------------------------------------------------------
    if input_image.len() < 128 || &input_image[0..4] != b"DDS " {
        return Err("Invalid DDS file selected.".into());
    }

    let mut c = Cursor::new(input_image);
    c.set_position(12);
    let height = c.read_u32::<LittleEndian>().map_err(|e| e.to_string())?;
    let width = c.read_u32::<LittleEndian>().map_err(|e| e.to_string())?;

    c.set_position(84);
    let mut fourcc = [0u8; 4];
    c.read_exact(&mut fourcc).map_err(|e| e.to_string())?;

    let new_format = match &fourcc {
        b"DXT1" => 7u32,
        b"DXT3" => 9u32,
        b"DXT5" => 11u32,
        _ => 5u32,
    };

    let new_pixels = input_image[128..].to_vec();
    let (has_magic, mut elements) = parse_chunk_elements(chunk_data)?;

    for (id, chunk) in elements.iter_mut() {
        match *id {
            20 => *chunk = width.to_le_bytes().to_vec(),
            21 => *chunk = height.to_le_bytes().to_vec(),
            23 => *chunk = new_format.to_le_bytes().to_vec(),
            22 => *chunk = new_pixels.clone(),
            _ => {}
        }
    }

    Ok(build_chunk_from_elements(has_magic, &elements))
}
