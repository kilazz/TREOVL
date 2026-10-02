use anyhow::{Context, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::{build_typed_container, parse_typed_container};
use crate::engine::common::{read_length_prefixed_string, write_length_prefixed_string};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FontJson {
    pub _engine_metadata: FontEngineMetadataJson,
    pub font_name: String,
    pub font_size: f32,
    pub line_height: f32,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub glyphs: Vec<GlyphMetricJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct FontEngineMetadataJson {
    pub type_id_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture_link: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub raw_blocks: Vec<RawFontBlock>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GlyphMetricJson {
    pub char_code: u32,
    pub character: String,
    pub width: f32,
    pub height: f32,
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RawFontBlock {
    pub id: u32,
    pub hex: String,
}

pub fn export_font_to_json(data: &[u8]) -> Result<String> {
    if data.len() < 5 {
        anyhow::bail!("Data too short for TREFont container");
    }

    let (type_id, elements) =
        parse_typed_container(data).context("Failed to parse TREFont typed container")?;

    let mut font_name = String::from("Standard_Font");
    let mut font_size = 14.0f32;
    let mut line_height = 16.0f32;
    let mut texture_link = None;
    let mut glyphs = Vec::new();
    let mut raw_blocks = Vec::new();

    for (id, chunk) in &elements {
        match *id {
            20 => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    font_name = s;
                }
            }
            21 => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    texture_link = Some(s);
                }
            }
            30 if chunk.len() >= 8 => {
                let mut cur = Cursor::new(chunk);
                if let Ok(sz) = cur.read_f32::<LittleEndian>()
                    && sz.is_finite()
                    && sz > 0.0
                {
                    font_size = sz;
                }
                if let Ok(lh) = cur.read_f32::<LittleEndian>()
                    && lh.is_finite()
                    && lh > 0.0
                {
                    line_height = lh;
                }
            }
            40 => {
                glyphs = parse_glyph_metrics(chunk);
            }
            _ => {
                raw_blocks.push(RawFontBlock {
                    id: *id,
                    hex: hex::encode_upper(chunk),
                });
            }
        }
    }

    let metadata = FontEngineMetadataJson {
        type_id_hex: format!("{:08X}", type_id),
        texture_link,
        raw_blocks,
    };

    let font_json = FontJson {
        _engine_metadata: metadata,
        font_name,
        font_size,
        line_height,
        glyphs,
    };

    serde_json::to_string_pretty(&font_json).map_err(|e| anyhow::anyhow!(e))
}

pub fn import_font_from_json(json_str: &str, _baseline: &[u8]) -> Result<Vec<u8>> {
    let parsed: FontJson = serde_json::from_str(json_str)?;
    let type_id =
        u32::from_str_radix(&parsed._engine_metadata.type_id_hex, 16).unwrap_or(0x00410072);

    let mut elements = Vec::new();

    for block in parsed._engine_metadata.raw_blocks {
        if let Ok(bytes) = hex::decode(&block.hex) {
            elements.push((block.id, bytes));
        }
    }

    elements.push((20, write_length_prefixed_string(&parsed.font_name)));

    if let Some(ref tlink) = parsed._engine_metadata.texture_link {
        elements.push((21, write_length_prefixed_string(tlink)));
    }

    let mut size_bytes = Vec::with_capacity(8);
    let _ = size_bytes.write_f32::<LittleEndian>(parsed.font_size);
    let _ = size_bytes.write_f32::<LittleEndian>(parsed.line_height);
    elements.push((30, size_bytes));

    if !parsed.glyphs.is_empty() {
        elements.push((40, rebuild_glyph_metrics(&parsed.glyphs)?));
    }

    elements.sort_by_key(|&(id, _)| id);
    Ok(build_typed_container(type_id, &elements))
}

fn parse_glyph_metrics(data: &[u8]) -> Vec<GlyphMetricJson> {
    let mut out = Vec::new();
    let mut cur = Cursor::new(data);
    while (cur.position() as usize) + 32 <= data.len() {
        let char_code = cur.read_u32::<LittleEndian>().unwrap_or(0);
        let width = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let height = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let u1 = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let v1 = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let u2 = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let v2 = cur.read_f32::<LittleEndian>().unwrap_or(0.0);

        let character = std::char::from_u32(char_code)
            .map(|c| c.to_string())
            .unwrap_or_else(|| "?".into());

        out.push(GlyphMetricJson {
            char_code,
            character,
            width,
            height,
            uv_min: [u1, v1],
            uv_max: [u2, v2],
        });
    }
    out
}

fn rebuild_glyph_metrics(glyphs: &[GlyphMetricJson]) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(glyphs.len() * 32);
    let mut cur = Cursor::new(&mut buf);
    for g in glyphs {
        cur.write_u32::<LittleEndian>(g.char_code)?;
        cur.write_f32::<LittleEndian>(g.width)?;
        cur.write_f32::<LittleEndian>(g.height)?;
        cur.write_f32::<LittleEndian>(g.uv_min[0])?;
        cur.write_f32::<LittleEndian>(g.uv_min[1])?;
        cur.write_f32::<LittleEndian>(g.uv_max[0])?;
        cur.write_f32::<LittleEndian>(g.uv_max[1])?;
    }
    Ok(buf)
}
