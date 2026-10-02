use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::{build_chunk_from_elements, build_typed_container, parse_chunk_elements, parse_typed_container};
use crate::engine::common::{read_length_prefixed_string, write_length_prefixed_string};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SoundCueVariation {
    pub id: u32,
    pub cue_name: String,
    pub sound_resource: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitch_min: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitch_max: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_hex: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SoundEventJson {
    pub type_id_hex: String,
    pub event_name: String,
    pub group_path: String,
    pub flags_hex: String,
    pub variations: Vec<SoundCueVariation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_properties: Option<Vec<RawEventProp>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RawEventProp {
    pub id: u32,
    pub hex: String,
}

pub fn export_event_to_json(chunk_data: &[u8]) -> Result<String> {
    if chunk_data.len() < 5 {
        bail!("Chunk data too short for Sound Event table.");
    }

    let (type_id, elements) = parse_typed_container(chunk_data)
        .context("Failed to parse Sound Event root container")?;

    let mut group_path = String::new();
    let mut event_name = String::new();
    let mut flags_hex = String::from("04004021");
    let mut variations = Vec::new();
    let mut raw_properties = Vec::new();

    for (id, chunk) in &elements {
        match *id {
            20 => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    group_path = s;
                }
            }
            21 => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    event_name = s;
                }
            }
            22 => {
                flags_hex = hex::encode_upper(chunk);
            }
            40 => {
                variations = parse_sound_cue_variations(chunk);
            }
            _ => {
                raw_properties.push(RawEventProp {
                    id: *id,
                    hex: hex::encode_upper(chunk),
                });
            }
        }
    }

    let event_json = SoundEventJson {
        type_id_hex: format!("{:08X}", type_id),
        event_name,
        group_path,
        flags_hex,
        variations,
        raw_properties: if raw_properties.is_empty() {
            None
        } else {
            Some(raw_properties)
        },
    };

    serde_json::to_string_pretty(&event_json).map_err(|e| anyhow::anyhow!(e))
}

pub fn import_event_from_json(json_str: &str) -> Result<Vec<u8>> {
    let parsed: SoundEventJson = serde_json::from_str(json_str)?;
    let type_id = u32::from_str_radix(&parsed.type_id_hex, 16)
        .context("Invalid TypeID hex in Event JSON")?;

    let mut elements = Vec::new();
    elements.push((20, write_length_prefixed_string(&parsed.group_path)));
    elements.push((21, write_length_prefixed_string(&parsed.event_name)));

    if let Ok(flags_bytes) = hex::decode(&parsed.flags_hex) {
        elements.push((22, flags_bytes));
    }
    elements.push((23, vec![1u8]));

    if !parsed.variations.is_empty() {
        let chunk_40 = rebuild_sound_cue_variations(&parsed.variations)?;
        elements.push((40, chunk_40));
    }

    elements.push((19, vec![0xFF, 0xFF, 0xFF, 0xFF]));
    elements.push((1, vec![0u8]));

    if let Some(raw_props) = parsed.raw_properties {
        for p in raw_props {
            if ![20, 21, 22, 23, 40, 19, 1].contains(&p.id)
                && let Ok(b) = hex::decode(&p.hex)
            {
                elements.push((p.id, b));
            }
        }
    }

    elements.sort_by_key(|&(id, _)| id);
    Ok(build_typed_container(type_id, &elements))
}

fn parse_sound_cue_variations(data: &[u8]) -> Vec<SoundCueVariation> {
    let mut out = Vec::new();
    let (_, sub_elements) = match parse_chunk_elements(data) {
        Ok(r) => r,
        Err(_) => return out,
    };

    for (var_id, var_chunk) in sub_elements {
        if let Ok((_, props)) = parse_chunk_elements(&var_chunk) {
            let mut cue_name = String::new();
            let mut sound_resource = String::new();

            for (pid, pdata) in props {
                if (pid == 20 || pid == 21) && let Some(s) = read_length_prefixed_string(&pdata) {
                    if s.contains(']') || s.contains('\\') {
                        sound_resource = s;
                    } else {
                        cue_name = s;
                    }
                }
            }

            if !cue_name.is_empty() || !sound_resource.is_empty() {
                out.push(SoundCueVariation {
                    id: var_id,
                    cue_name,
                    sound_resource,
                    volume: Some(1.0),
                    pitch_min: Some(0.95),
                    pitch_max: Some(1.05),
                    radius: Some(25.0),
                    raw_hex: None,
                });
            }
        }
    }
    out
}

fn rebuild_sound_cue_variations(variations: &[SoundCueVariation]) -> Result<Vec<u8>> {
    let mut var_chunks = Vec::new();

    for v in variations {
        let cue_elements = vec![
            (20, write_length_prefixed_string(&v.sound_resource)),
            (21, write_length_prefixed_string(&v.cue_name)),
        ];
        let var_sub = build_chunk_from_elements(false, &cue_elements);
        var_chunks.push((v.id, var_sub));
    }

    Ok(build_chunk_from_elements(true, &var_chunks))
}
