use anyhow::{Context, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::{build_chunk_from_elements, parse_chunk_elements};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EnvironmentConfigJson {
    pub fog_near: f32,
    pub fog_far: f32,
    pub fog_color: String,
    pub ambient_color: String,
    pub sun_direction: [f32; 3],
    pub sun_color: String,
    pub fill_direction: [f32; 3],
    pub fill_color: String,
    pub fill_range: f32,
    pub gamma: f32,
    pub exposure: f32,
    pub bloom_threshold: f32,
    pub bloom_intensity: f32,
    pub water_tint: String,
    pub far_clip: f32,
    pub flow_vector: [f32; 2],
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub raw_unmapped_chunks: Vec<(u32, String)>,
}

pub fn export_environment_to_json(data: &[u8]) -> Result<String> {
    let (_, elements) =
        parse_chunk_elements(data).context("Failed to parse Environment .env container table")?;

    let mut cfg = EnvironmentConfigJson {
        fog_near: 20.0,
        fog_far: 40.0,
        fog_color: "#377479".into(),
        ambient_color: "#0D1015".into(),
        sun_direction: [0.395, -0.854, -0.336],
        sun_color: "#020202".into(),
        fill_direction: [-0.816, -0.408, 0.408],
        fill_color: "#403122".into(),
        fill_range: 10.0,
        gamma: 2.2,
        exposure: 0.3,
        bloom_threshold: 0.05,
        bloom_intensity: 1.5,
        water_tint: "#ADBCC5".into(),
        far_clip: 64.0,
        flow_vector: [0.6, -1.2],
        raw_unmapped_chunks: Vec::new(),
    };

    for (id, chunk) in elements {
        match id {
            0x0BCC if chunk.len() >= 4 => {
                cfg.fog_near = Cursor::new(&chunk).read_f32::<LittleEndian>()?
            }
            0x0BCD if chunk.len() >= 4 => {
                cfg.fog_far = Cursor::new(&chunk).read_f32::<LittleEndian>()?
            }
            0x0BCE if chunk.len() >= 3 => {
                cfg.fog_color = format!("#{:02X}{:02X}{:02X}", chunk[0], chunk[1], chunk[2])
            }
            0x0BD2 if chunk.len() >= 3 => {
                cfg.ambient_color = format!("#{:02X}{:02X}{:02X}", chunk[0], chunk[1], chunk[2])
            }
            0x0BD3 if chunk.len() >= 4 => {
                cfg.sun_direction[0] = Cursor::new(&chunk).read_f32::<LittleEndian>()?
            }
            0x0BD4 if chunk.len() >= 4 => {
                cfg.sun_direction[1] = Cursor::new(&chunk).read_f32::<LittleEndian>()?
            }
            0x0BD5 if chunk.len() >= 4 => {
                cfg.sun_direction[2] = Cursor::new(&chunk).read_f32::<LittleEndian>()?
            }
            0x0BD6 if chunk.len() >= 3 => {
                cfg.sun_color = format!("#{:02X}{:02X}{:02X}", chunk[0], chunk[1], chunk[2])
            }
            0x0BD7 if chunk.len() >= 4 => {
                cfg.fill_direction[0] = Cursor::new(&chunk).read_f32::<LittleEndian>()?
            }
            0x0BD8 if chunk.len() >= 4 => {
                cfg.fill_direction[1] = Cursor::new(&chunk).read_f32::<LittleEndian>()?
            }
            0x0BD9 if chunk.len() >= 4 => {
                cfg.fill_direction[2] = Cursor::new(&chunk).read_f32::<LittleEndian>()?
            }
            0x0BE2 if chunk.len() >= 3 => {
                cfg.fill_color = format!("#{:02X}{:02X}{:02X}", chunk[0], chunk[1], chunk[2])
            }
            0x10CD if chunk.len() >= 4 => {
                cfg.fill_range = Cursor::new(&chunk).read_f32::<LittleEndian>()?
            }
            0x0BEE if chunk.len() >= 4 => {
                cfg.bloom_threshold = Cursor::new(&chunk).read_f32::<LittleEndian>()?
            }
            0x0BF3 if chunk.len() >= 4 => {
                cfg.gamma = Cursor::new(&chunk).read_f32::<LittleEndian>()?
            }
            0x0BF4 if chunk.len() >= 4 => {
                cfg.exposure = Cursor::new(&chunk).read_f32::<LittleEndian>()?
            }
            0x0BF6 if chunk.len() >= 4 => {
                cfg.bloom_intensity = Cursor::new(&chunk).read_f32::<LittleEndian>()?
            }
            0x0BFE if chunk.len() >= 3 => {
                cfg.water_tint = format!("#{:02X}{:02X}{:02X}", chunk[0], chunk[1], chunk[2])
            }
            0x0BFF if chunk.len() >= 4 => {
                cfg.far_clip = Cursor::new(&chunk).read_f32::<LittleEndian>()?
            }
            0x0C0B if chunk.len() >= 8 => {
                let mut cur = Cursor::new(&chunk);
                cfg.flow_vector[0] = cur.read_f32::<LittleEndian>()?;
                cfg.flow_vector[1] = cur.read_f32::<LittleEndian>()?;
            }
            _ => {
                cfg.raw_unmapped_chunks
                    .push((id, hex::encode_upper(&chunk)));
            }
        }
    }

    serde_json::to_string_pretty(&cfg).context("Failed to serialize Environment JSON")
}

pub fn import_environment_from_json(json_str: &str, baseline: &[u8]) -> Result<Vec<u8>> {
    let cfg: EnvironmentConfigJson = serde_json::from_str(json_str)?;
    let (_, mut elements) = parse_chunk_elements(baseline)?;

    let parse_hex_color = |hex_str: &str| -> [u8; 4] {
        let clean = hex_str.trim_start_matches('#');
        if let Ok(val) = u32::from_str_radix(clean, 16) {
            [
                ((val >> 16) & 0xFF) as u8,
                ((val >> 8) & 0xFF) as u8,
                (val & 0xFF) as u8,
                0,
            ]
        } else {
            [255, 255, 255, 0]
        }
    };

    for (id, chunk) in elements.iter_mut() {
        match *id {
            0x0BCC => *chunk = cfg.fog_near.to_le_bytes().to_vec(),
            0x0BCD => *chunk = cfg.fog_far.to_le_bytes().to_vec(),
            0x0BCE => *chunk = parse_hex_color(&cfg.fog_color).to_vec(),
            0x0BD2 => *chunk = parse_hex_color(&cfg.ambient_color).to_vec(),
            0x0BD3 => *chunk = cfg.sun_direction[0].to_le_bytes().to_vec(),
            0x0BD4 => *chunk = cfg.sun_direction[1].to_le_bytes().to_vec(),
            0x0BD5 => *chunk = cfg.sun_direction[2].to_le_bytes().to_vec(),
            0x0BD6 => *chunk = parse_hex_color(&cfg.sun_color).to_vec(),
            0x0BD7 => *chunk = cfg.fill_direction[0].to_le_bytes().to_vec(),
            0x0BD8 => *chunk = cfg.fill_direction[1].to_le_bytes().to_vec(),
            0x0BD9 => *chunk = cfg.fill_direction[2].to_le_bytes().to_vec(),
            0x0BE2 => *chunk = parse_hex_color(&cfg.fill_color).to_vec(),
            0x10CD => *chunk = cfg.fill_range.to_le_bytes().to_vec(),
            0x0BEE => *chunk = cfg.bloom_threshold.to_le_bytes().to_vec(),
            0x0BF3 => *chunk = cfg.gamma.to_le_bytes().to_vec(),
            0x0BF4 => *chunk = cfg.exposure.to_le_bytes().to_vec(),
            0x0BF6 => *chunk = cfg.bloom_intensity.to_le_bytes().to_vec(),
            0x0BFE => *chunk = parse_hex_color(&cfg.water_tint).to_vec(),
            0x0BFF => *chunk = cfg.far_clip.to_le_bytes().to_vec(),
            0x0C0B => {
                let mut b = Vec::with_capacity(8);
                b.extend_from_slice(&cfg.flow_vector[0].to_le_bytes());
                b.extend_from_slice(&cfg.flow_vector[1].to_le_bytes());
                *chunk = b;
            }
            _ => {}
        }
    }

    Ok(build_chunk_from_elements(false, &elements))
}
