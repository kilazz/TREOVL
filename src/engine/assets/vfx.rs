use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::{
    build_chunk_from_elements, build_typed_container, parse_chunk_elements, parse_typed_container,
};
use crate::engine::common::{read_length_prefixed_string, write_length_prefixed_string};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VfxGraphJson {
    pub _engine_metadata: VfxEngineMetadataJson,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vfx_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect_id_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub emitters: Vec<VfxEmitterJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct VfxEngineMetadataJson {
    pub type_id_hex: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub components: Vec<VfxComponentBlockJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VfxEmitterJson {
    pub id: u32,
    pub type_id_hex: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_seconds: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifetime: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawn_rate: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_rgba_hex: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub properties: Vec<VfxPropertyBlockJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub sub_emitters: Vec<VfxEmitterJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VfxPropertyBlockJson {
    pub id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub ptype: String, // "string", "float", "uint", "vector3", "vector4", "color_hex", "null_descriptor", "hex"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string_val: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub float_val: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uint_val: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_val: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector3_val: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector4_val: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hex_val: Option<String>,
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
    let mut effect_id_hex = None;
    let mut enabled = None;
    let mut emitters = Vec::new();
    let mut components = Vec::new();

    for (id, chunk) in &elements {
        match *id {
            20 => group_path = read_length_prefixed_string(chunk),
            21 => vfx_name = read_length_prefixed_string(chunk),
            22 => effect_id_hex = Some(hex::encode_upper(chunk)),
            23 => {
                if !chunk.is_empty() {
                    enabled = Some(chunk[0] != 0);
                }
            }
            1 => {
                emitters = extract_all_emitters_recursive(chunk);
            }
            _ => {}
        }

        // Do not dump component 1 as a redundant multi-kilobyte hex string
        if *id != 1 {
            components.push(VfxComponentBlockJson {
                id: *id,
                role: get_vfx_component_role(*id).to_string(),
                hex: hex::encode_upper(chunk),
            });
        }
    }

    let metadata = VfxEngineMetadataJson {
        type_id_hex: format!("{:08X}", type_id),
        components,
    };

    let vfx_json = VfxGraphJson {
        _engine_metadata: metadata,
        group_path,
        vfx_name,
        effect_id_hex,
        enabled,
        emitters,
    };

    serde_json::to_string_pretty(&vfx_json).map_err(|e| anyhow::anyhow!(e))
}

pub fn import_vfx_from_json(json_str: &str) -> Result<Vec<u8>> {
    let parsed: VfxGraphJson = serde_json::from_str(json_str)?;
    let type_id = u32::from_str_radix(&parsed._engine_metadata.type_id_hex, 16)
        .context("Invalid TypeID hex in VFX JSON metadata")?;

    let mut elements = Vec::new();

    for comp in &parsed._engine_metadata.components {
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
            22 => {
                if let Some(ref eff_hex) = parsed.effect_id_hex
                    && let Ok(b) = hex::decode(eff_hex)
                {
                    raw_bytes = b;
                }
            }
            23 => {
                if let Some(en) = parsed.enabled {
                    raw_bytes = vec![if en { 1 } else { 0 }];
                }
            }
            _ => {}
        }

        elements.push((comp.id, raw_bytes));
    }

    if !parsed.emitters.is_empty() {
        let rebuilt_emitters_container = rebuild_emitter_hierarchy(&parsed.emitters)?;
        elements.push((1, rebuilt_emitters_container));
    }

    elements.sort_by_key(|&(id, _)| id);
    Ok(build_typed_container(type_id, &elements))
}

// -----------------------------------------------------------------------------
// RECURSIVE EMITTER EXTRACTION
// -----------------------------------------------------------------------------

fn extract_all_emitters_recursive(data: &[u8]) -> Vec<VfxEmitterJson> {
    let mut result = Vec::new();

    if data.len() >= 4
        && (data[2] == 0x73
            || (u32::from_le_bytes(data[0..4].try_into().unwrap_or_default()) >> 16 == 0x0073))
        && let Ok(emitter) = parse_single_emitter(data)
    {
        result.push(emitter);
        return result;
    }

    if let Ok((_, sub_elements)) = parse_chunk_elements(data) {
        for (sid, sdata) in sub_elements {
            if sdata.len() >= 4
                && (sdata[2] == 0x73
                    || (u32::from_le_bytes(sdata[0..4].try_into().unwrap_or_default()) >> 16
                        == 0x0073))
            {
                if let Ok(mut emitter) = parse_single_emitter(&sdata) {
                    emitter.id = sid;
                    result.push(emitter);
                }
            } else {
                let nested = extract_all_emitters_recursive(&sdata);
                result.extend(nested);
            }
        }
    }

    result
}

fn parse_single_emitter(data: &[u8]) -> Result<VfxEmitterJson> {
    let (type_id, elements) = parse_typed_container(data)?;
    let mut name = String::new();
    let mut tag = None;
    let mut delay_seconds = None;
    let mut lifetime = None;
    let mut spawn_rate = None;
    let mut speed = None;
    let mut size = None;
    let mut color_rgba_hex = None;
    let mut properties = Vec::new();
    let mut sub_emitters = Vec::new();

    for (id, chunk) in elements {
        if id == 50 && chunk.len() > 16 {
            let nested = extract_all_emitters_recursive(&chunk);
            if !nested.is_empty() {
                sub_emitters.extend(nested);
                continue;
            }
        }

        let mut ptype = "hex".to_string();
        let mut string_val = None;
        let mut float_val = None;
        let mut uint_val = None;
        let mut color_val = None;
        let mut vector3_val = None;
        let mut vector4_val = None;
        let mut hex_val = Some(hex::encode_upper(&chunk));

        // 1. Strings
        if (id == 2 || id == 20 || id == 21)
            && let Some(s) = read_length_prefixed_string(&chunk)
        {
            if tag.is_none() && id != 21 {
                tag = Some(s.clone());
            }
            if name.is_empty() || id == 21 {
                name = s.clone();
            }
            string_val = Some(s);
            ptype = "string".to_string();
            hex_val = None;
        }

        // 2. Color codes & Hashes (IDs 40, 41, 46, 58, 69)
        if ptype == "hex"
            && chunk.len() == 4
            && (id == 40 || id == 41 || id == 46 || id == 58 || id == 69)
        {
            let val = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
            if val > 0x0000_FFFF || id == 40 || id == 41 || id == 46 {
                let col_hex = format!("#{:08X}", val);
                color_val = Some(col_hex.clone());
                ptype = "color_hex".to_string();
                hex_val = None;
                if id == 40 || id == 46 {
                    color_rgba_hex = Some(col_hex);
                }
            }
        }

        // 3. Vectors (12 and 16 bytes)
        let is_pure_zero = chunk.iter().all(|&b| b == 0);
        if ptype == "hex" && chunk.len() == 12 {
            if is_pure_zero {
                ptype = "null_descriptor".to_string();
                hex_val = None;
            } else {
                let mut cur = Cursor::new(&chunk);
                if let (Ok(x), Ok(y), Ok(z)) = (
                    cur.read_f32::<LittleEndian>(),
                    cur.read_f32::<LittleEndian>(),
                    cur.read_f32::<LittleEndian>(),
                ) && x.is_finite()
                    && y.is_finite()
                    && z.is_finite()
                    && x.abs() < 50_000.0
                    && y.abs() < 50_000.0
                    && z.abs() < 50_000.0
                {
                    vector3_val = Some([x, y, z]);
                    ptype = "vector3".to_string();
                    hex_val = None;
                }
            }
        }

        if ptype == "hex" && chunk.len() == 16 && !is_pure_zero {
            let mut cur = Cursor::new(&chunk);
            if let (Ok(x), Ok(y), Ok(z), Ok(w)) = (
                cur.read_f32::<LittleEndian>(),
                cur.read_f32::<LittleEndian>(),
                cur.read_f32::<LittleEndian>(),
                cur.read_f32::<LittleEndian>(),
            ) && x.is_finite()
                && y.is_finite()
                && z.is_finite()
                && w.is_finite()
                && x.abs() < 50_000.0
                && y.abs() < 50_000.0
                && z.abs() < 50_000.0
            {
                vector4_val = Some([x, y, z, w]);
                ptype = "vector4".to_string();
                hex_val = None;
            }
        }

        // 4. Scalar 32-bit floats & ints
        if ptype == "hex" && chunk.len() == 4 {
            let mut cur = Cursor::new(&chunk);
            let raw_u32 = cur.read_u32::<LittleEndian>().unwrap_or(0);
            let f = f32::from_bits(raw_u32);

            let is_clean_float = f.is_finite()
                && !f.is_nan()
                && (f.abs() >= 1e-4 || f == 0.0)
                && f.abs() <= 50_000.0
                && (id == 23 || (29..=40).contains(&id) || (56..=72).contains(&id));

            if is_clean_float {
                float_val = Some(f);
                ptype = "float".to_string();
                hex_val = None;

                match id {
                    29 => delay_seconds = Some(f),
                    30 => spawn_rate = Some(f),
                    31 => lifetime = Some(f),
                    32 | 33 => size = Some(f),
                    34 | 35 => speed = Some(f),
                    _ => {}
                }
            } else {
                uint_val = Some(raw_u32);
                ptype = "uint".to_string();
                hex_val = None;
            }
        }

        // 5. Short integers (1-2 bytes)
        if ptype == "hex" && (chunk.len() == 1 || chunk.len() == 2) {
            let mut val = 0u32;
            for (b_i, &byte) in chunk.iter().enumerate() {
                val |= (byte as u32) << (b_i * 8);
            }
            uint_val = Some(val);
            ptype = "uint".to_string();
            hex_val = None;
        }

        properties.push(VfxPropertyBlockJson {
            id,
            role: Some(get_vfx_property_role(id).to_string()),
            ptype,
            string_val,
            float_val,
            uint_val,
            color_val,
            vector3_val,
            vector4_val,
            hex_val,
        });
    }

    if name.is_empty() {
        name = tag.clone().unwrap_or_else(|| "Unnamed_Emitter".to_string());
    }

    Ok(VfxEmitterJson {
        id: 0,
        type_id_hex: format!("{:08X}", type_id),
        name,
        tag,
        delay_seconds,
        lifetime,
        spawn_rate,
        speed,
        size,
        color_rgba_hex,
        properties,
        sub_emitters,
    })
}

// -----------------------------------------------------------------------------
// REBUILDING EMITTER BINARY HIERARCHY
// -----------------------------------------------------------------------------

fn rebuild_emitter_hierarchy(emitters: &[VfxEmitterJson]) -> Result<Vec<u8>> {
    let mut rebuilt_layers = Vec::new();

    for (idx, em) in emitters.iter().enumerate() {
        let type_id = u32::from_str_radix(&em.type_id_hex, 16).unwrap_or(0x00730004);
        let mut em_elements = Vec::new();

        for prop in &em.properties {
            let chunk = match prop.ptype.as_str() {
                "string" => {
                    let s = prop.string_val.as_deref().unwrap_or(&em.name);
                    write_length_prefixed_string(s)
                }
                "float" => {
                    let f = prop.float_val.unwrap_or(0.0);
                    f.to_le_bytes().to_vec()
                }
                "uint" => {
                    let u = prop.uint_val.unwrap_or(0);
                    u.to_le_bytes().to_vec()
                }
                "color_hex" => {
                    if let Some(ref col) = prop.color_val {
                        let clean = col.trim_start_matches('#');
                        u32::from_str_radix(clean, 16)
                            .unwrap_or(0xFFFFFFFF)
                            .to_le_bytes()
                            .to_vec()
                    } else {
                        vec![0xFF, 0xFF, 0xFF, 0xFF]
                    }
                }
                "null_descriptor" => vec![0u8; 12],
                "vector3" => {
                    let [x, y, z] = prop.vector3_val.unwrap_or([0.0, 0.0, 0.0]);
                    let mut b = Vec::with_capacity(12);
                    b.extend_from_slice(&x.to_le_bytes());
                    b.extend_from_slice(&y.to_le_bytes());
                    b.extend_from_slice(&z.to_le_bytes());
                    b
                }
                "vector4" => {
                    let [x, y, z, w] = prop.vector4_val.unwrap_or([0.0, 0.0, 0.0, 0.0]);
                    let mut b = Vec::with_capacity(16);
                    b.extend_from_slice(&x.to_le_bytes());
                    b.extend_from_slice(&y.to_le_bytes());
                    b.extend_from_slice(&z.to_le_bytes());
                    b.extend_from_slice(&w.to_le_bytes());
                    b
                }
                _ => {
                    if let Some(ref h) = prop.hex_val {
                        hex::decode(h).unwrap_or_default()
                    } else {
                        Vec::new()
                    }
                }
            };
            em_elements.push((prop.id, chunk));
        }

        if !em.sub_emitters.is_empty() {
            let mut sub_chunks = Vec::new();
            for (s_idx, sub_em) in em.sub_emitters.iter().enumerate() {
                let sub_type = u32::from_str_radix(&sub_em.type_id_hex, 16).unwrap_or(0x00730008);
                let mut sub_props = Vec::new();
                for sp in &sub_em.properties {
                    let s_chunk = match sp.ptype.as_str() {
                        "string" => write_length_prefixed_string(
                            sp.string_val.as_deref().unwrap_or(&sub_em.name),
                        ),
                        "float" => sp.float_val.unwrap_or(0.0).to_le_bytes().to_vec(),
                        "uint" => sp.uint_val.unwrap_or(0).to_le_bytes().to_vec(),
                        "color_hex" => {
                            let clean = sp
                                .color_val
                                .as_deref()
                                .unwrap_or("FFFFFFFF")
                                .trim_start_matches('#');
                            u32::from_str_radix(clean, 16)
                                .unwrap_or(0xFFFFFFFF)
                                .to_le_bytes()
                                .to_vec()
                        }
                        "null_descriptor" => vec![0u8; 12],
                        "vector3" => {
                            let [x, y, z] = sp.vector3_val.unwrap_or([0.0, 0.0, 0.0]);
                            let mut b = Vec::with_capacity(12);
                            b.extend_from_slice(&x.to_le_bytes());
                            b.extend_from_slice(&y.to_le_bytes());
                            b.extend_from_slice(&z.to_le_bytes());
                            b
                        }
                        _ => sp
                            .hex_val
                            .as_deref()
                            .and_then(|h| hex::decode(h).ok())
                            .unwrap_or_default(),
                    };
                    sub_props.push((sp.id, s_chunk));
                }
                let sub_container = build_typed_container(sub_type, &sub_props);
                sub_chunks.push((s_idx as u32, sub_container));
            }
            let sub_table = build_chunk_from_elements(true, &sub_chunks);
            em_elements.push((50, sub_table));
        }

        em_elements.sort_by_key(|&(id, _)| id);
        let emitter_blob = build_typed_container(type_id, &em_elements);
        rebuilt_layers.push((idx as u32, emitter_blob));
    }

    let emitter_list_container = build_chunk_from_elements(true, &rebuilt_layers);
    let wrapper_sub = vec![(10, vec![38u8, 0, 0, 0]), (1, emitter_list_container)];
    let wrapper_chunk = build_chunk_from_elements(false, &wrapper_sub);
    let top_layer = vec![(30, wrapper_chunk)];

    Ok(build_chunk_from_elements(false, &top_layer))
}

fn get_vfx_component_role(id: u32) -> &'static str {
    match id {
        20 => "Asset Group Path",
        21 => "VFX Graph Name",
        22 => "Effect Trigger Flags / ID",
        23 => "Layer Enabled Flag",
        19 => "Sentinel Terminator",
        1 => "Particle Emitter Subcontainer Layers",
        _ => "VFX Graph Parameter",
    }
}

fn get_vfx_property_role(id: u32) -> &'static str {
    match id {
        20 => "Asset / Shader Tag",
        21 => "Emitter Layer Name",
        22 => "Render Mode / Particle Type",
        23 => "Blending & Alpha Mode",
        24 => "Texture / Material Link",
        25 => "Emitter Loop Count",
        26 => "Emitter Enabled Flag",
        29 => "Initial Delay (s)",
        30 => "Emission Rate / Burst Count",
        31 => "Particle Lifetime (s)",
        32 => "Initial Size / Radius",
        33 => "Size Multiplier Over Life",
        34 => "Initial Velocity / Speed",
        35 => "Velocity Randomness Spread",
        36 => "Gravity Multiplier / Accel Z",
        37 => "Damping / Friction Factor",
        38 => "Rotation / Angular Velocity",
        39 => "Spin Randomness Spread",
        40 => "Primary Particle Color (RGBA)",
        41 => "Secondary / Fade Color (RGBA)",
        42 => "Emissive Brightness Scale",
        44 => "Dynamic Light Radius",
        45 => "Dynamic Light Intensity",
        46 => "Light Color (RGBA)",
        47 => "Keyframe Curve Progress",
        48 => "Keyframe Value",
        50 => "Nested Sub-Emitter Layers Table",
        51 => "Emitter Slot Index",
        52 => "Loop Mode (-1 = Infinite Loop)",
        53 => "Initial Delay Offset (s)",
        54 => "Burst Emit Count",
        55 => "Emitter Type Identifier",
        56 => "Spawn Interval / Delta Time",
        57 => "Alpha Multiplier / Master Opacity",
        58 => "Particle Color / Texture Hash",
        59 => "Particle Flags Mask",
        60 => "Collision Mode (0 = None, 1 = Ground)",
        61 => "Velocity Scale / Burst Force",
        62 => "Damping / Air Resistance (Drag)",
        63 => "Min Spawn Distance / Radius",
        64 => "Max Spawn Distance / Radius",
        65 => "Velocity Min Bounding Box (XYZ)",
        66 => "Velocity Max Bounding Box (XYZ)",
        67 => "Uniform Scale Multiplier",
        68 => "Random Rotation Angle Spread",
        69 => "Particle Trigger Event ID / Color",
        70 => "Max Active Particles Limit",
        71 => "Follow Parent / World Space Flag",
        72 => "Sorting / Render Order Priority",
        1 => "Nested Sub-Emitter Layers",
        _ => "Emitter Parameter",
    }
}
