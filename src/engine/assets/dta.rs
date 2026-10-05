use anyhow::{Context, Result, bail};
use byteorder::{BigEndian, LittleEndian, ReadBytesExt, WriteBytesExt};
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::{parse_chunk_elements, parse_typed_container};
use crate::engine::container::footer::{
    FOOTER_SIZE, MAGIC_FOOTER_1, MAGIC_FOOTER_2, calculate_triumph_crc32, check_footer,
};

pub const DTA_HEADER_SIZE: usize = 48;
pub const DEFAULT_CHUNK_SIZE: usize = 2048;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DtaPackageJson {
    pub _engine_metadata: DtaEngineMetadataJson,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub lights: Vec<DtaLightRecordJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub parsed_container_elements: Vec<DtaContainerElementJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decompressed_payload_hex: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct DtaEngineMetadataJson {
    pub file_type: String,
    pub uncompressed_size: usize,
    pub compressed_size: usize,
    pub checksum_hex: String,
    pub block_count: usize,
    pub footer_crc_hex: String,
    pub footer_hash2_hex: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DtaLightRecordJson {
    pub id: usize,
    pub position: [f32; 3],
    pub radius: f32,
    pub color_rgba: [f32; 4],
    pub intensity: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DtaContainerElementJson {
    pub id: u32,
    pub size: usize,
    pub hex: String,
}

#[derive(Debug, Clone)]
pub struct DtaHeader {
    pub uncompressed_size: u32,
    pub checksum: u32,
    pub version: u32,
    pub compressed_payload_size: u32,
    pub max_block_size: u32,
}

impl DtaHeader {
    pub fn read(data: &[u8]) -> Result<Self> {
        if data.len() < DTA_HEADER_SIZE {
            bail!("Data too short for DTA header (minimum 48 bytes required)");
        }
        let uncompressed_size = u32::from_le_bytes(data[0..4].try_into()?);
        let checksum = u32::from_le_bytes(data[4..8].try_into()?);
        let version = u32::from_le_bytes(data[8..12].try_into()?);
        let compressed_payload_size = u32::from_be_bytes(data[0x28..0x2C].try_into()?);
        let max_block_size = u32::from_be_bytes(data[0x2C..0x30].try_into()?);

        Ok(Self {
            uncompressed_size,
            checksum,
            version,
            compressed_payload_size,
            max_block_size,
        })
    }

    pub fn write(&self) -> [u8; DTA_HEADER_SIZE] {
        let mut out = [0u8; DTA_HEADER_SIZE];
        out[0..4].copy_from_slice(&self.uncompressed_size.to_le_bytes());
        out[4..8].copy_from_slice(&self.checksum.to_le_bytes());
        out[8..12].copy_from_slice(&self.version.to_le_bytes());
        out[0x20..0x24].copy_from_slice(&self.uncompressed_size.to_be_bytes());
        out[0x28..0x2C].copy_from_slice(&self.compressed_payload_size.to_be_bytes());
        out[0x2C..0x30].copy_from_slice(&self.max_block_size.to_be_bytes());
        out
    }
}

/// Decompresses a block-compressed .dta stream into a raw uncompressed buffer
pub fn decompress_dta_payload(data: &[u8]) -> Result<(Vec<u8>, DtaHeader, DtaEngineMetadataJson)> {
    let footer = check_footer(data).context("Missing or invalid Triumph 16-byte footer in .dta")?;
    let header = DtaHeader::read(data)?;

    let payload_end = data.len().saturating_sub(FOOTER_SIZE);
    let mut pos = DTA_HEADER_SIZE;
    let mut decompressed_all = Vec::with_capacity(header.uncompressed_size as usize);
    let mut block_count = 0;

    while pos + 4 <= payload_end {
        let total_size = u16::from_be_bytes(data[pos..pos + 2].try_into()?) as usize;

        if total_size == 0 || pos + total_size > payload_end {
            break;
        }

        let block_slice = &data[pos + 4..pos + total_size];
        let decompressed_chunk =
            decompress_chunk_resilient(block_slice, header.max_block_size as usize)?;
        decompressed_all.extend_from_slice(&decompressed_chunk);

        pos += total_size;
        block_count += 1;
    }

    let metadata = DtaEngineMetadataJson {
        file_type: "Triumph Lighting Set / Block Container (.dta)".into(),
        uncompressed_size: decompressed_all.len(),
        compressed_size: pos.saturating_sub(DTA_HEADER_SIZE),
        checksum_hex: format!("0x{:08X}", header.checksum),
        block_count,
        footer_crc_hex: format!("0x{:08X}", footer.original_crc),
        footer_hash2_hex: format!("0x{:08X}", footer.hash2),
    };

    Ok((decompressed_all, header, metadata))
}

fn decompress_chunk_resilient(slice: &[u8], expected_max: usize) -> Result<Vec<u8>> {
    let target_buffer_size = expected_max.max(8192);

    // 1. Attempt decompression with standard subheader offsets (16 bytes, 20 bytes, or raw 0)
    for offset in [16, 20, 0] {
        if slice.len() > offset {
            let mut decompressor = Decompress::new(false);
            let mut out = vec![0u8; target_buffer_size];
            if let Ok(status) =
                decompressor.decompress(&slice[offset..], &mut out, FlushDecompress::Finish)
                && (status == flate2::Status::StreamEnd || decompressor.total_out() > 0)
            {
                out.truncate(decompressor.total_out() as usize);
                return Ok(out);
            }
        }
    }

    // 2. Fallback: scan for embedded Deflate stream signature
    for offset in 1..slice.len().min(32) {
        let mut decompressor = Decompress::new(false);
        let mut out = vec![0u8; target_buffer_size];
        if let Ok(status) =
            decompressor.decompress(&slice[offset..], &mut out, FlushDecompress::Finish)
            && (status == flate2::Status::StreamEnd || decompressor.total_out() > 0)
        {
            out.truncate(decompressor.total_out() as usize);
            return Ok(out);
        }
    }

    Ok(slice.to_vec())
}

/// Compresses a raw data buffer back into a valid Triumph .dta block stream with header and footer
pub fn compress_dta_payload(
    raw_data: &[u8],
    original_header: Option<&DtaHeader>,
) -> Result<Vec<u8>> {
    let uncompressed_size = raw_data.len() as u32;
    let checksum = calculate_triumph_crc32(raw_data);
    let chunk_size = original_header
        .map(|h| h.max_block_size as usize)
        .unwrap_or(DEFAULT_CHUNK_SIZE);

    let mut blocks = Vec::new();
    let mut total_compressed_payload_size = 0usize;

    for chunk in raw_data.chunks(chunk_size) {
        let mut compressor = Compress::new(Compression::best(), false);
        let mut comp_buf = vec![0u8; chunk.len() * 2 + 128];
        compressor.compress(chunk, &mut comp_buf, FlushCompress::Finish)?;
        let comp_len = compressor.total_out() as usize;
        comp_buf.truncate(comp_len);

        // Standard 16-byte block subheader
        let mut sub_header = [0u8; 16];
        sub_header[0..2].copy_from_slice(&(chunk.len() as u16).to_be_bytes());
        sub_header[2..4].copy_from_slice(&[0x00, 0x04]);

        let payload_len = 16 + comp_buf.len();
        let total_block_len = payload_len + 2;

        let mut block_data = Vec::with_capacity(total_block_len + 2);
        block_data.write_u16::<BigEndian>(total_block_len as u16)?;
        block_data.write_u16::<BigEndian>(payload_len as u16)?;
        block_data.extend_from_slice(&sub_header);
        block_data.extend_from_slice(&comp_buf);

        total_compressed_payload_size += block_data.len();
        blocks.push(block_data);
    }

    let header = DtaHeader {
        uncompressed_size,
        checksum,
        version: 1,
        compressed_payload_size: (total_compressed_payload_size + FOOTER_SIZE) as u32,
        max_block_size: chunk_size as u32,
    };

    let mut out_file =
        Vec::with_capacity(DTA_HEADER_SIZE + total_compressed_payload_size + FOOTER_SIZE);
    out_file.extend_from_slice(&header.write());

    for b in blocks {
        out_file.extend_from_slice(&b);
    }

    let footer_crc = calculate_triumph_crc32(&out_file);
    out_file.write_u32::<LittleEndian>(MAGIC_FOOTER_1)?;
    out_file.write_u32::<LittleEndian>(MAGIC_FOOTER_2)?;
    out_file.write_u32::<LittleEndian>(footer_crc)?;
    out_file.write_u32::<LittleEndian>(0x0012FD34)?; // Default Triumph package hash

    Ok(out_file)
}

pub fn export_dta_to_json(data: &[u8], _stem: &str) -> Result<String> {
    let (decompressed, _hdr, metadata) = decompress_dta_payload(data)?;

    let mut lights = Vec::new();
    let mut parsed_container_elements = Vec::new();

    // 1. Attempt to parse as typed Triumph container
    if let Ok((_type_id, elements)) = parse_typed_container(&decompressed) {
        for (id, chunk) in elements {
            parsed_container_elements.push(DtaContainerElementJson {
                id,
                size: chunk.len(),
                hex: hex::encode_upper(&chunk),
            });
        }
    } else if let Ok((_, elements)) = parse_chunk_elements(&decompressed) {
        for (id, chunk) in elements {
            parsed_container_elements.push(DtaContainerElementJson {
                id,
                size: chunk.len(),
                hex: hex::encode_upper(&chunk),
            });
        }
    }

    // 2. Scan for Point Light records (32-byte stride: 3x f32 pos, 1x f32 radius, 3x f32 color, 1x f32 intensity)
    if decompressed.len() >= 32 {
        let mut cur = Cursor::new(&decompressed);
        let mut light_id = 0;

        while (cur.position() as usize) + 32 <= decompressed.len() {
            let start_pos = cur.position();
            let x = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
            let y = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
            let z = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
            let radius = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
            let r = cur.read_f32::<LittleEndian>().unwrap_or(1.0);
            let g = cur.read_f32::<LittleEndian>().unwrap_or(1.0);
            let b = cur.read_f32::<LittleEndian>().unwrap_or(1.0);
            let intensity = cur.read_f32::<LittleEndian>().unwrap_or(1.0);

            let is_valid_coord = x.is_finite()
                && y.is_finite()
                && z.is_finite()
                && x.abs() < 50_000.0
                && y.abs() < 50_000.0;
            let is_valid_radius = radius.is_finite() && radius > 0.01 && radius < 10_000.0;
            let is_valid_color =
                r.is_finite() && g.is_finite() && b.is_finite() && r >= 0.0 && g >= 0.0 && b >= 0.0;

            if is_valid_coord && is_valid_radius && is_valid_color {
                lights.push(DtaLightRecordJson {
                    id: light_id,
                    position: [x, y, z],
                    radius,
                    color_rgba: [r, g, b, 1.0],
                    intensity,
                    flags: None,
                });
                light_id += 1;
            } else {
                cur.set_position(start_pos + 4);
            }
        }
    }

    let dta_json = DtaPackageJson {
        _engine_metadata: metadata,
        lights,
        parsed_container_elements,
        decompressed_payload_hex: Some(hex::encode_upper(&decompressed)),
    };

    serde_json::to_string_pretty(&dta_json).context("Failed to serialize DTA JSON")
}

pub fn import_dta_from_json(json_str: &str, baseline: &[u8]) -> Result<Vec<u8>> {
    let parsed: DtaPackageJson = serde_json::from_str(json_str)?;

    let (baseline_decompressed, baseline_hdr, _) = decompress_dta_payload(baseline).unwrap_or((
        Vec::new(),
        DtaHeader {
            uncompressed_size: 0,
            checksum: 0,
            version: 1,
            compressed_payload_size: 0,
            max_block_size: DEFAULT_CHUNK_SIZE as u32,
        },
        DtaEngineMetadataJson::default(),
    ));

    let mut working_buffer = if let Some(ref hex_str) = parsed.decompressed_payload_hex {
        hex::decode(hex_str).unwrap_or(baseline_decompressed)
    } else {
        baseline_decompressed
    };

    // Update light attributes into the working buffer if edited
    if !parsed.lights.is_empty() && working_buffer.len() >= parsed.lights.len() * 32 {
        let mut cur = Cursor::new(&mut working_buffer);
        for l in parsed.lights {
            let _ = cur.write_f32::<LittleEndian>(l.position[0]);
            let _ = cur.write_f32::<LittleEndian>(l.position[1]);
            let _ = cur.write_f32::<LittleEndian>(l.position[2]);
            let _ = cur.write_f32::<LittleEndian>(l.radius);
            let _ = cur.write_f32::<LittleEndian>(l.color_rgba[0]);
            let _ = cur.write_f32::<LittleEndian>(l.color_rgba[1]);
            let _ = cur.write_f32::<LittleEndian>(l.color_rgba[2]);
            let _ = cur.write_f32::<LittleEndian>(l.intensity);
        }
    }

    compress_dta_payload(&working_buffer, Some(&baseline_hdr))
}
