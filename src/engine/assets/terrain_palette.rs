use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    pub render_flags: RenderFlagsJson,
    pub sub_layer_counter: u8,
    pub textures: Vec<TextureLinkJson>,
    pub terrain_splat_layers: Vec<SplatLayerJson>,
    pub foliage_scatter_groups: HashMap<String, Vec<FoliageMeshJson>>,
    pub environment: EnvironmentLinksJson,
    pub terminator_sentinel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_fallback_components: Option<Vec<RawComponentJson>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RenderFlagsJson {
    pub raw_hex: String,
    pub blend_mode: u8,
    pub has_alpha_layers: bool,
    pub foliage_density_multiplier: u8,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TextureLinkJson {
    pub slot_id: u32,
    pub role: String,
    pub pointer_tag: String,
    pub filename: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SplatLayerJson {
    pub id: u32,
    pub name: String,
    pub brush_index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foliage_group: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FoliageMeshJson {
    pub mesh_slot: String,
    pub mesh_file: String,
    pub display_name: String,
    pub density: f32,
    pub height_min: f32,
    pub height_max: f32,
    pub width_scale: f32,
    pub tint_variation: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct EnvironmentLinksJson {
    pub sky_textures: Vec<TextureLinkJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soil_objects_texture: Option<TextureLinkJson>,
    pub water_textures: Vec<TextureLinkJson>,
    pub sky_meshes: Vec<TextureLinkJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RawComponentJson {
    pub id: u32,
    pub role: String,
    pub hex: String,
}

pub fn export_terrain_palette_to_json(data: &[u8]) -> Result<String> {
    if data.len() < 5 {
        bail!("Data is too short for a Terrain Palette container");
    }

    let (type_id, elements) =
        parse_typed_container(data).context("Failed to parse Terrain Palette root container")?;

    let mut resource_tag = None;
    let mut palette_name = None;
    let mut thumbnail = None;
    let mut render_flags = RenderFlagsJson {
        raw_hex: "01004005".into(),
        blend_mode: 1,
        has_alpha_layers: true,
        foliage_density_multiplier: 5,
    };
    let mut sub_layer_counter = 1u8;
    let mut textures = Vec::new();
    let mut terrain_splat_layers = Vec::new();
    let mut foliage_scatter_groups: HashMap<String, Vec<FoliageMeshJson>> = HashMap::new();
    let mut environment = EnvironmentLinksJson::default();
    let mut terminator_sentinel = "FFFFFFFF".to_string();
    let mut raw_components = Vec::new();

    for (id, chunk) in &elements {
        raw_components.push(RawComponentJson {
            id: *id,
            role: get_component_role(*id).to_string(),
            hex: hex::encode_upper(chunk),
        });

        match *id {
            20 => resource_tag = read_length_prefixed_string(chunk),
            21 => palette_name = read_length_prefixed_string(chunk),
            22 if chunk.len() >= 4 => {
                render_flags = RenderFlagsJson {
                    raw_hex: hex::encode_upper(chunk),
                    blend_mode: chunk[0],
                    has_alpha_layers: (chunk[2] & 0x40) != 0,
                    foliage_density_multiplier: chunk[3],
                };
            }
            23 if !chunk.is_empty() => {
                sub_layer_counter = chunk[0];
            }
            40..=53 => {
                if let Some(mut link) = parse_texture_link(*id, chunk) {
                    link.role = get_texture_slot_role(*id).to_string();
                    textures.push(link);
                }
            }
            54 => {
                decode_splat_and_foliage_container(
                    chunk,
                    &mut terrain_splat_layers,
                    &mut foliage_scatter_groups,
                );
            }
            56..=60 | 63 => {
                if let Some(link) = parse_texture_link(*id, chunk) {
                    environment.sky_textures.push(link);
                }
            }
            61 => {
                environment.soil_objects_texture = parse_texture_link(*id, chunk);
            }
            62 => {
                thumbnail = read_length_prefixed_string(chunk);
            }
            64..=66 => {
                if let Some(link) = parse_texture_link(*id, chunk) {
                    environment.water_textures.push(link);
                }
            }
            67..=69 => {
                if let Some(link) = parse_texture_link(*id, chunk) {
                    environment.sky_meshes.push(link);
                }
            }
            19 => {
                terminator_sentinel = hex::encode_upper(chunk);
            }
            _ => {}
        }
    }

    let json_data = TerrainPaletteJson {
        type_id_hex: format!("{:08X}", type_id),
        resource_tag,
        palette_name,
        thumbnail,
        render_flags,
        sub_layer_counter,
        textures,
        terrain_splat_layers,
        foliage_scatter_groups,
        environment,
        terminator_sentinel,
        raw_fallback_components: Some(raw_components),
    };

    serde_json::to_string_pretty(&json_data).map_err(|e| anyhow::anyhow!(e))
}

pub fn import_terrain_palette_from_json(json_str: &str) -> Result<Vec<u8>> {
    let parsed: TerrainPaletteJson = serde_json::from_str(json_str)?;
    let type_id = u32::from_str_radix(&parsed.type_id_hex, 16)
        .context("Invalid TypeID hex in Terrain Palette JSON")?;

    let mut elements = Vec::new();

    if let Some(raw_list) = parsed.raw_fallback_components {
        for comp in raw_list {
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
                22 => {
                    if let Ok(flags_bytes) = hex::decode(&parsed.render_flags.raw_hex) {
                        raw_bytes = flags_bytes;
                    }
                }
                23 => {
                    raw_bytes = vec![parsed.sub_layer_counter];
                }
                62 => {
                    if let Some(ref thumb) = parsed.thumbnail {
                        raw_bytes = write_length_prefixed_string(thumb);
                    }
                }
                _ => {}
            }

            elements.push((comp.id, raw_bytes));
        }
    }

    Ok(build_typed_container(type_id, &elements))
}

// -----------------------------------------------------------------------------
// Internal Parsers for Sub-container ID 54
// -----------------------------------------------------------------------------

fn decode_splat_and_foliage_container(
    container_bytes: &[u8],
    layers: &mut Vec<SplatLayerJson>,
    foliage: &mut HashMap<String, Vec<FoliageMeshJson>>,
) {
    let (_, entries) = match parse_chunk_elements(container_bytes) {
        Ok(res) => res,
        Err(_) => return,
    };

    let mut layer_id_counter = 0;

    for (_, chunk) in entries {
        if let Ok((sub_type, sub_elems)) = parse_typed_container(&chunk)
            && sub_type == 0x04000080
        {
            let mut name = String::new();
            let mut brush_idx = layer_id_counter;

            for (pid, pdata) in &sub_elems {
                // Soilcover group names are stored under ID 10, terrain layers under ID 20
                if (*pid == 20 || *pid == 10)
                    && let Some(s) = read_length_prefixed_string(pdata)
                {
                    name = s;
                } else if *pid == 26 && pdata.len() >= 4 {
                    brush_idx = u32::from_le_bytes(pdata[0..4].try_into().unwrap_or_default());
                }
            }

            if !name.is_empty() && name != "noname" {
                if name.ends_with("_SOIL") || name == "Forest A" || name == "Forest B" {
                    let group_name = name.clone();
                    let mut meshes = Vec::new();
                    for (pid, pdata) in &sub_elems {
                        if *pid == 1 {
                            scan_foliage_meshes(pdata, &mut meshes);
                        }
                    }
                    if !meshes.is_empty() {
                        foliage.insert(group_name, meshes);
                    }
                } else {
                    let foliage_group = match name.as_str() {
                        "Grass" => Some("Grass_SOIL".to_string()),
                        "Grass_moss" => Some("Grass_moss_SOIL".to_string()),
                        "Forest A" => Some("Forest A".to_string()),
                        "Forest B" => Some("Forest B".to_string()),
                        _ => None,
                    };

                    layers.push(SplatLayerJson {
                        id: layer_id_counter,
                        name,
                        brush_index: brush_idx,
                        foliage_group,
                    });
                    layer_id_counter += 1;
                }
            }
        }
    }
}

fn scan_foliage_meshes(data: &[u8], out: &mut Vec<FoliageMeshJson>) {
    if let Ok((type_id, elems)) = parse_typed_container(data) {
        if type_id == 0x04000079 {
            let mut mesh_slot = String::new();
            let mut mesh_file = String::new();
            let mut display_name = String::new();
            let mut floats = [0.5f32, 12.0f32, 24.0f32, 1.0f32, 1.0f32];

            for (id, chunk) in &elems {
                if *id == 20
                    && let Some(s) = read_length_prefixed_string(chunk)
                {
                    mesh_slot = s;
                } else if *id == 21
                    && let Some(s) = read_length_prefixed_string(chunk)
                {
                    mesh_file = s;
                } else if *id == 22
                    && let Some(s) = read_length_prefixed_string(chunk)
                {
                    display_name = s;
                } else if *id == 30 && chunk.len() >= 20 {
                    let mut cur = Cursor::new(chunk);
                    for f in &mut floats {
                        if let Ok(val) = cur.read_f32::<LittleEndian>()
                            && val.is_finite()
                        {
                            *f = val;
                        }
                    }
                }
            }

            if !mesh_file.is_empty() {
                out.push(FoliageMeshJson {
                    mesh_slot,
                    mesh_file,
                    display_name,
                    density: floats[0],
                    height_min: floats[1],
                    height_max: floats[2],
                    width_scale: floats[3],
                    tint_variation: floats[4],
                });
            }
        }

        for (_, chunk) in elems {
            scan_foliage_meshes(&chunk, out);
        }
    } else if let Ok((_, sub_elems)) = parse_chunk_elements(data) {
        for (_, chunk) in sub_elems {
            scan_foliage_meshes(&chunk, out);
        }
    }
}

fn parse_texture_link(slot_id: u32, data: &[u8]) -> Option<TextureLinkJson> {
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
            return Some(TextureLinkJson {
                slot_id,
                role: get_texture_slot_role(slot_id).to_string(),
                pointer_tag,
                filename,
            });
        }
    }
    None
}

fn get_texture_slot_role(id: u32) -> &'static str {
    match id {
        40 => "Base Diffuse",
        41 => "Base Normal Map",
        42 => "Cliff Diffuse",
        43 => "Cliff Normal Map",
        44 => "Cliff Mask (Rough/Spec)",
        45 => "Cliff Sand Diffuse",
        46 => "Cliff Sand Normal Map",
        47 | 48 => "System Mask Texture",
        49 => "Detail Normal Map",
        50 => "Detail Specular Map",
        51 => "Bedrock Diffuse",
        52 => "Bedrock Normal Map",
        53 => "Bedrock Mask",
        56..=60 | 63 => "Skybox / Atmosphere Texture",
        61 => "Soil Objects Multi-Texture",
        64..=66 => "Water Surface Flow Texture",
        67..=69 => "Sky Dome Geometry Mesh",
        _ => "Palette Texture Link",
    }
}

fn get_component_role(id: u32) -> &'static str {
    match id {
        20 => "Resource ID Group Tag",
        21 => "Palette Biome Name",
        22 => "Layer Render Flags",
        23 => "Sub-Layer Counter",
        40..=53 => "Terrain Texture Link",
        54 => "Splat Layers & Foliage Scatter Table",
        55 => "Table Terminator Flag",
        56..=63 => "Environment & Sky Texture Link",
        64..=66 => "Water Simulation Textures",
        67..=69 => "Skybox Mesh Links",
        19 => "Sentinel Terminator",
        1 => "Root Container Terminator",
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
