use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

pub const CPTX_MAGIC: &[u8; 4] = b"CPTX";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CptxMapJson {
    pub magic: String,
    pub uv_rects: Vec<UvRectJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub unmapped_sequences: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UvRectJson {
    pub id: usize,
    pub u_min: f32,
    pub v_min: f32,
    pub u_max: f32,
    pub v_max: f32,
}

/// Decodes a binary CPTX texture atlas map into structured UV rectangles
pub fn export_cptx_to_json(data: &[u8]) -> Result<String> {
    if data.len() < 4 || &data[0..4] != CPTX_MAGIC {
        bail!("Invalid CPTX file: Missing 'CPTX' magic header");
    }

    let mut uv_rects = Vec::new();
    let mut unmapped_sequences = Vec::new();
    let payload = &data[4..];

    let mut pos = 0;
    let mut rect_id = 0;

    let is_valid_uv = |f: f32| f.is_finite() && !f.is_nan() && (-10.0..=10.0).contains(&f);

    while pos + 16 <= payload.len() {
        let chunk = &payload[pos..pos + 16];
        let mut cur = Cursor::new(chunk);
        let u1 = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let v1 = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let u2 = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let v2 = cur.read_f32::<LittleEndian>().unwrap_or(0.0);

        if is_valid_uv(u1) && is_valid_uv(v1) && is_valid_uv(u2) && is_valid_uv(v2) {
            uv_rects.push(UvRectJson {
                id: rect_id,
                u_min: u1,
                v_min: v1,
                u_max: u2,
                v_max: v2,
            });
            pos += 16;
            rect_id += 1;
        } else {
            unmapped_sequences.push(hex::encode_upper(&payload[pos..pos + 4]));
            pos += 4;
        }
    }

    if pos < payload.len() {
        unmapped_sequences.push(hex::encode_upper(&payload[pos..]));
    }

    let json_data = CptxMapJson {
        magic: "CPTX".to_string(),
        uv_rects,
        unmapped_sequences,
    };

    serde_json::to_string_pretty(&json_data).map_err(|e| anyhow::anyhow!(e))
}

/// Injects modified UV rectangle coordinates back into the baseline CPTX binary
pub fn import_cptx_from_json(json_str: &str, baseline: &[u8]) -> Result<Vec<u8>> {
    let parsed: CptxMapJson = serde_json::from_str(json_str)
        .context("Syntax error in CPTX JSON format")?;

    let mut output = baseline.to_vec();
    if output.len() < 4 || &output[0..4] != CPTX_MAGIC {
        bail!("Baseline is not a valid CPTX file");
    }

    let mut pos = 4;
    let mut rect_id = 0;

    let is_valid_uv = |f: f32| f.is_finite() && !f.is_nan() && (-10.0..=10.0).contains(&f);

    while pos + 16 <= output.len() && rect_id < parsed.uv_rects.len() {
        let chunk = &output[pos..pos + 16];
        let mut cur = Cursor::new(chunk);
        let u1 = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let v1 = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let u2 = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let v2 = cur.read_f32::<LittleEndian>().unwrap_or(0.0);

        if is_valid_uv(u1) && is_valid_uv(v1) && is_valid_uv(u2) && is_valid_uv(v2) {
            let rect = &parsed.uv_rects[rect_id];
            let mut w_cur = Cursor::new(&mut output[pos..pos + 16]);
            w_cur.write_f32::<LittleEndian>(rect.u_min)?;
            w_cur.write_f32::<LittleEndian>(rect.v_min)?;
            w_cur.write_f32::<LittleEndian>(rect.u_max)?;
            w_cur.write_f32::<LittleEndian>(rect.v_max)?;

            pos += 16;
            rect_id += 1;
        } else {
            pos += 4;
        }
    }

    Ok(output)
}
