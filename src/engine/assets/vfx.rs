use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::{build_typed_container, parse_chunk_elements, parse_typed_container};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VfxGraphJson {
    pub type_id_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vfx_name: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub emitters: Vec<VfxEmitterSummaryJson>,
    pub components: Vec<VfxComponentBlockJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VfxEmitterSummaryJson {
    pub id: u32,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VfxComponentBlockJson {
    pub id: u32,
    pub role: String,
    pub hex: String,
}

pub fn export_vfx_to_json(chunk_data: &[u8]) -> Result<String> {
    if chunk_data.len() < 5 {
        bail!("Chunk data too short to be a valid VFX Container.");
    }

    let (type_id, elements) =
        parse_typed_container(chunk_data).context("Failed to parse VFX root typed container")?;

    let mut group_path = None;
    let mut vfx_name = None;
    let mut emitters = Vec::new();
    let mut components = Vec::new();

    for (id, chunk) in &elements {
        match *id {
            20 => group_path = read_length_prefixed_string(chunk),
            21 => vfx_name = read_length_prefixed_string(chunk),
            1 => {
                // Parse Subcontainer containing Emitter Layers
                if let Ok((_, sub_elements)) = parse_chunk_elements(chunk) {
                    for (e_id, e_data) in sub_elements {
                        if let Some(ename) = extract_emitter_name(&e_data) {
                            emitters.push(VfxEmitterSummaryJson {
                                id: e_id,
                                name: ename,
                            });
                        }
                    }
                }
            }
            _ => {}
        }

        components.push(VfxComponentBlockJson {
            id: *id,
            role: get_vfx_component_role(*id).to_string(),
            hex: hex::encode_upper(chunk),
        });
    }

    let vfx_json = VfxGraphJson {
        type_id_hex: format!("{:08X}", type_id),
        group_path,
        vfx_name,
        emitters,
        components,
    };

    serde_json::to_string_pretty(&vfx_json).map_err(|e| anyhow::anyhow!(e))
}

pub fn import_vfx_from_json(json_str: &str) -> Result<Vec<u8>> {
    let parsed: VfxGraphJson = serde_json::from_str(json_str)?;
    let type_id =
        u32::from_str_radix(&parsed.type_id_hex, 16).context("Invalid TypeID hex in VFX JSON")?;

    let mut elements = Vec::new();

    for comp in parsed.components {
        let mut raw_bytes = hex::decode(&comp.hex)
            .with_context(|| format!("Invalid hex payload in component ID {}", comp.id))?;

        match comp.id {
            20 => {
                if let Some(ref path) = parsed.group_path {
                    raw_bytes = write_length_prefixed_string(path);
                }
            }
            21 => {
                if let Some(ref name) = parsed.vfx_name {
                    raw_bytes = write_length_prefixed_string(name);
                }
            }
            _ => {}
        }

        elements.push((comp.id, raw_bytes));
    }

    Ok(build_typed_container(type_id, &elements))
}

fn get_vfx_component_role(id: u32) -> &'static str {
    match id {
        20 => "Asset Group Path",
        21 => "VFX Graph Name",
        22 => "Effect Trigger Flags / ID",
        23 => "Layer Enabled Flag",
        19 => "Sentinel Terminator",
        1 => "Particle Emitter Subcontainer Layers",
        _ => "VFX Parameter Block",
    }
}

fn read_length_prefixed_string(data: &[u8]) -> Option<String> {
    if data.len() < 4 {
        return None;
    }
    let len = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
    if len > 0 && len <= data.len() - 4 {
        let slice = &data[4..4 + len];
        let clean = slice.strip_suffix(&[0]).unwrap_or(slice);
        std::str::from_utf8(clean)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        None
    }
}

fn write_length_prefixed_string(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(4 + bytes.len());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
    out
}

fn extract_emitter_name(data: &[u8]) -> Option<String> {
    let mut search_pos = 0;
    while search_pos + 8 <= data.len() {
        let len = u32::from_le_bytes(
            data[search_pos..search_pos + 4]
                .try_into()
                .unwrap_or_default(),
        ) as usize;
        if (2..=32).contains(&len) && search_pos + 4 + len <= data.len() {
            let slice = &data[search_pos + 4..search_pos + 4 + len];
            if slice.iter().all(|&b| (0x20..=0x7E).contains(&b) || b == 0)
                && let Ok(s) = std::str::from_utf8(slice)
            {
                let clean = s.trim_matches(char::from(0)).trim();
                if !clean.is_empty() && !clean.starts_with('[') {
                    return Some(clean.to_string());
                }
            }
        }
        search_pos += 1;
    }
    None
}
