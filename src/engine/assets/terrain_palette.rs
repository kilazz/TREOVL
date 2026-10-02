use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::{build_typed_container, parse_chunk_elements, parse_typed_container};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TerrainPaletteJson {
    pub type_id_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub textures: Vec<TerrainTextureLinkJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub layers: Vec<TerrainLayerJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub foliage_scatter: Vec<TerrainFoliageScatterJson>,
    pub components: Vec<TerrainPaletteComponentJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TerrainTextureLinkJson {
    pub slot_id: u32,
    pub pointer_tag: String,
    pub filename: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TerrainLayerJson {
    pub id: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foliage_group: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TerrainFoliageScatterJson {
    pub group_name: String,
    pub mesh_slot: String,
    pub mesh_filename: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale_params: Option<[f32; 3]>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TerrainPaletteComponentJson {
    pub id: u32,
    pub role: String,
    pub hex: String,
}

pub fn export_terrain_palette_to_json(data: &[u8]) -> Result<String> {
    if data.len() < 5 {
        bail!("Data too short for a Terrain Palette container");
    }

    let (type_id, elements) = parse_typed_container(data)
        .context("Failed to parse Terrain Palette root typed container")?;

    let mut resource_tag = None;
    let mut palette_name = None;
    let mut thumbnail = None;
    let mut textures = Vec::new();
    let mut layers = Vec::new();
    let mut foliage_scatter = Vec::new();
    let mut components = Vec::new();

    for (id, chunk) in &elements {
        match *id {
            20 => resource_tag = read_length_prefixed_string(chunk),
            21 => palette_name = read_length_prefixed_string(chunk),
            _ => {
                if let Some(link) = parse_texture_link(*id, chunk) {
                    textures.push(link);
                }

                if thumbnail.is_none() && chunk.windows(4).any(|w| w == b".BMP" || w == b".bmp") {
                    thumbnail = extract_filename_with_ext(chunk, ".BMP");
                }
            }
        }

        scan_for_layers_and_foliage(chunk, &mut layers, &mut foliage_scatter);

        components.push(TerrainPaletteComponentJson {
            id: *id,
            role: get_component_role(*id).to_string(),
            hex: hex::encode_upper(chunk),
        });
    }

    let json_data = TerrainPaletteJson {
        type_id_hex: format!("{:08X}", type_id),
        resource_tag,
        palette_name,
        thumbnail,
        textures,
        layers,
        foliage_scatter,
        components,
    };

    serde_json::to_string_pretty(&json_data).map_err(|e| anyhow::anyhow!(e))
}

pub fn import_terrain_palette_from_json(json_str: &str) -> Result<Vec<u8>> {
    let parsed: TerrainPaletteJson = serde_json::from_str(json_str)?;
    let type_id = u32::from_str_radix(&parsed.type_id_hex, 16)
        .context("Invalid TypeID hex in Terrain Palette JSON")?;

    let mut elements = Vec::new();

    for comp in parsed.components {
        let mut raw_bytes = hex::decode(&comp.hex)
            .with_context(|| format!("Invalid hex payload in component ID {}", comp.id))?;

        match comp.id {
            20 => {
                if let Some(ref tag) = parsed.resource_tag {
                    raw_bytes = write_length_prefixed_string(tag);
                }
            }
            21 => {
                if let Some(ref name) = parsed.palette_name {
                    raw_bytes = write_length_prefixed_string(name);
                }
            }
            _ => {}
        }

        elements.push((comp.id, raw_bytes));
    }

    Ok(build_typed_container(type_id, &elements))
}

fn parse_texture_link(slot_id: u32, data: &[u8]) -> Option<TerrainTextureLinkJson> {
    if let Ok((_, sub_elems)) = parse_chunk_elements(data) {
        let mut pointer_tag = String::new();
        let mut filename = String::new();

        for (id, sub_data) in sub_elems {
            if id == 20
                && let Some(s) = read_length_prefixed_string(&sub_data)
            {
                pointer_tag = s;
            } else if id == 21
                && let Some(s) = read_length_prefixed_string(&sub_data)
            {
                filename = s;
            }
        }

        if !filename.is_empty() {
            return Some(TerrainTextureLinkJson {
                slot_id,
                pointer_tag,
                filename,
            });
        }
    }
    None
}

fn scan_for_layers_and_foliage(
    data: &[u8],
    layers: &mut Vec<TerrainLayerJson>,
    foliage: &mut Vec<TerrainFoliageScatterJson>,
) {
    if let Ok((type_id, elems)) = parse_typed_container(data) {
        if type_id == 0x04000080 {
            let mut layer_name = String::new();
            let mut foliage_group = None;

            for (id, chunk) in &elems {
                if *id == 21
                    && let Some(s) = read_length_prefixed_string(chunk)
                {
                    layer_name = s;
                } else if *id == 20
                    && let Some(s) = read_length_prefixed_string(chunk)
                {
                    foliage_group = Some(s);
                }
            }

            if !layer_name.is_empty() && layer_name != "noname" {
                layers.push(TerrainLayerJson {
                    id: layers.len() as u32,
                    name: layer_name,
                    foliage_group,
                });
            }
        } else if type_id == 0x04000079 {
            let mut mesh_slot = String::new();
            let mut mesh_filename = String::new();
            let mut display_name = None;
            let mut scale_params = None;

            for (id, chunk) in &elems {
                if *id == 20
                    && let Some(s) = read_length_prefixed_string(chunk)
                {
                    mesh_slot = s;
                } else if *id == 21
                    && let Some(s) = read_length_prefixed_string(chunk)
                {
                    mesh_filename = s;
                } else if *id == 22
                    && let Some(s) = read_length_prefixed_string(chunk)
                {
                    display_name = Some(s);
                } else if *id == 30 && chunk.len() >= 12 {
                    let mut cur = Cursor::new(chunk);
                    if let (Ok(x), Ok(y), Ok(z)) = (
                        cur.read_f32::<LittleEndian>(),
                        cur.read_f32::<LittleEndian>(),
                        cur.read_f32::<LittleEndian>(),
                    ) && x.is_finite()
                        && y.is_finite()
                        && z.is_finite()
                    {
                        scale_params = Some([x, y, z]);
                    }
                }
            }

            if !mesh_filename.is_empty() {
                foliage.push(TerrainFoliageScatterJson {
                    group_name: "Soilcover".to_string(),
                    mesh_slot,
                    mesh_filename,
                    display_name,
                    scale_params,
                });
            }
        }

        for (_, chunk) in elems {
            scan_for_layers_and_foliage(&chunk, layers, foliage);
        }
    } else if let Ok((_, sub_elems)) = parse_chunk_elements(data) {
        for (_, chunk) in sub_elems {
            scan_for_layers_and_foliage(&chunk, layers, foliage);
        }
    }
}

fn get_component_role(id: u32) -> &'static str {
    match id {
        20 => "Resource ID Group Tag",
        21 => "Palette Biome Name",
        22 => "Layer Render Flags",
        23 => "Sub-Layer Counter",
        40..=54 => "Texture Material Link",
        55 => "Splat Layers & Foliage Table",
        19 => "Sentinel Terminator",
        1 => "Environment & Sky Mesh Links",
        _ => "Terrain Palette Parameter",
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

fn extract_filename_with_ext(data: &[u8], ext: &str) -> Option<String> {
    let mut cur = Cursor::new(data);
    while (cur.position() as usize) + 8 <= data.len() {
        if let Ok(len) = cur.read_u32::<LittleEndian>() {
            let len = len as usize;
            let pos = cur.position() as usize;
            if (3..=100).contains(&len) && pos + len <= data.len() {
                let slice = &data[pos..pos + len];
                if let Ok(s) = std::str::from_utf8(slice) {
                    let clean = s.trim_matches(char::from(0)).trim();
                    if clean.to_lowercase().ends_with(&ext.to_lowercase()) {
                        return Some(clean.to_string());
                    }
                }
            }
        }
        cur.set_position(cur.position() + 1);
    }
    None
}
