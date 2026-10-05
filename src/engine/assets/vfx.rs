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
    pub trigger_flags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub emitters: Vec<VfxEmitterJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct VfxEngineMetadataJson {
    pub type_id: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub unmapped_components: Vec<VfxComponentBlockJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VfxEmitterJson {
    pub id: u32,
    pub type_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emitter_type: Option<String>,
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
    pub color_rgba: Option<String>,
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
    pub ptype: String, // "string", "float", "int", "uint", "vector3", "vector4", "color", "emitter_link", "raw"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string_val: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub float_val: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub int_val: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uint_val: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_emitter_id: Option<u32>,
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
    let mut trigger_flags = None;
    let mut enabled = None;
    let mut emitters = Vec::new();
    let mut unmapped_components = Vec::new();

    for (id, chunk) in &elements {
        match *id {
            20 => group_path = read_length_prefixed_string(chunk),
            21 => vfx_name = read_length_prefixed_string(chunk),
            22 => {
                let mask =
                    u32::from_le_bytes(chunk[0..4.min(chunk.len())].try_into().unwrap_or_default());
                trigger_flags = Some(format!("0x{:08X}", mask));
            }
            23 => {
                if !chunk.is_empty() {
                    enabled = Some(chunk[0] != 0);
                }
            }
            1 => {
                emitters = extract_all_emitters_recursive(chunk);
            }
            19 => {
                // Known sentinel terminator [0xFF, 0xFF, 0xFF, 0xFF], no need to dump in JSON
            }
            _ => {
                // Only genuinely unknown / unmapped components go here
                unmapped_components.push(VfxComponentBlockJson {
                    id: *id,
                    role: get_vfx_component_role(*id).to_string(),
                    hex: hex::encode_upper(chunk),
                });
            }
        }
    }

    let metadata = VfxEngineMetadataJson {
        type_id: format!("0x{:08X}", type_id),
        unmapped_components,
    };

    let vfx_json = VfxGraphJson {
        _engine_metadata: metadata,
        group_path,
        vfx_name,
        trigger_flags,
        enabled,
        emitters,
    };

    serde_json::to_string_pretty(&vfx_json).map_err(|e| anyhow::anyhow!(e))
}

pub fn import_vfx_from_json(json_str: &str) -> Result<Vec<u8>> {
    let parsed: VfxGraphJson = serde_json::from_str(json_str)?;
    let clean_type_id = parsed._engine_metadata.type_id.trim_start_matches("0x");
    let type_id = u32::from_str_radix(clean_type_id, 16)
        .context("Invalid TypeID hex in VFX JSON metadata")?;

    let mut elements = Vec::new();

    // 1. Group Path (ID 20)
    if let Some(ref path) = parsed.group_path {
        elements.push((20, write_length_prefixed_string(path)));
    }

    // 2. VFX Name (ID 21)
    if let Some(ref name) = parsed.vfx_name {
        elements.push((21, write_length_prefixed_string(name)));
    }

    // 3. Trigger Flags (ID 22)
    if let Some(ref flags_str) = parsed.trigger_flags {
        let clean = flags_str
            .trim()
            .trim_start_matches("0x")
            .trim_start_matches("0X");
        let mask = u32::from_str_radix(clean, 16).unwrap_or(0x16004007);
        elements.push((22, mask.to_le_bytes().to_vec()));
    }

    // 4. Enabled Flag (ID 23)
    if let Some(en) = parsed.enabled {
        elements.push((23, vec![if en { 1 } else { 0 }]));
    }

    // 5. Unmapped Components
    for comp in &parsed._engine_metadata.unmapped_components {
        if let Ok(b) = hex::decode(&comp.hex) {
            elements.push((comp.id, b));
        }
    }

    // 6. Sentinel Terminator (ID 19)
    elements.push((19, vec![0xFF, 0xFF, 0xFF, 0xFF]));

    // 7. Emitters Hierarchy (ID 1)
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
    let mut emitter_type = None;
    let mut delay_seconds = None;
    let mut lifetime = None;
    let mut spawn_rate = None;
    let mut speed = None;
    let mut size = None;
    let mut color_rgba = None;
    let mut properties = Vec::new();
    let mut sub_emitters = Vec::new();

    for (id, chunk) in elements {
        if id == 50 {
            if chunk.len() > 16 {
                let nested = extract_all_emitters_recursive(&chunk);
                if !nested.is_empty() {
                    sub_emitters.extend(nested);
                    continue;
                }
            }

            if let Ok((_, link_elements)) = parse_chunk_elements(&chunk)
                && let Some((_, data_id10)) = link_elements.iter().find(|(sub_id, _)| *sub_id == 10)
                && data_id10.len() >= 4
            {
                let target_id = u32::from_le_bytes(data_id10[0..4].try_into().unwrap_or_default());
                properties.push(VfxPropertyBlockJson {
                    id: 50,
                    role: Some("Trigger Link / Target Chain Emitter".to_string()),
                    ptype: "emitter_link".to_string(),
                    string_val: None,
                    float_val: None,
                    int_val: None,
                    uint_val: Some(target_id),
                    color: None,
                    target_emitter_id: Some(target_id),
                    vector3_val: None,
                    vector4_val: None,
                    hex_val: None,
                });
                continue;
            }

            if chunk.len() == 4 {
                let target_id = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
                if target_id < 64 {
                    properties.push(VfxPropertyBlockJson {
                        id: 50,
                        role: Some("Trigger Link / Target Chain Emitter".to_string()),
                        ptype: "emitter_link".to_string(),
                        string_val: None,
                        float_val: None,
                        int_val: None,
                        uint_val: Some(target_id),
                        color: None,
                        target_emitter_id: Some(target_id),
                        vector3_val: None,
                        vector4_val: None,
                        hex_val: None,
                    });
                    continue;
                }
            }
        }

        let mut ptype = "raw".to_string();
        let mut string_val = None;
        let mut float_val = None;
        let mut int_val = None;
        let mut uint_val = None;
        let mut color = None;
        let mut vector3_val = None;
        let mut vector4_val = None;
        let target_emitter_id = None;
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

        // 2. Color codes & Texture Hashes
        if ptype == "raw"
            && chunk.len() == 4
            && (id == 40 || id == 41 || id == 46 || id == 58 || id == 69)
        {
            let val = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
            let f = f32::from_bits(val);

            if f.is_finite() && (0.001..=10.0).contains(&f) && id == 58 {
                float_val = Some(f);
                ptype = "float".to_string();
                hex_val = None;
            } else if val > 0x0000_FFFF || id == 40 || id == 41 || id == 46 {
                let col_hex = format!("#{:08X}", val);
                color = Some(col_hex.clone());
                ptype = "color".to_string();
                hex_val = None;
                if id == 40 || id == 46 {
                    color_rgba = Some(col_hex);
                }
            }
        }

        // 3. 3D Vectors (12 bytes)
        if ptype == "raw" && chunk.len() == 12 {
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

        // 4. 4D Vectors (16 bytes)
        if ptype == "raw" && chunk.len() == 16 {
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

        // 5. Scalar 32-bit floats & ints
        if ptype == "raw" && chunk.len() == 4 {
            let raw_u32 = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
            let f = f32::from_bits(raw_u32);

            if raw_u32 == 0xFFFFFFFF && (id == 52 || id == 59 || id == 61) {
                int_val = Some(-1);
                ptype = "int".to_string();
                hex_val = None;
            } else {
                let is_clean_float = f.is_finite()
                    && !f.is_nan()
                    && (f.abs() >= 1e-4 || f == 0.0)
                    && f.abs() <= 50_000.0
                    && (id == 23 || (29..=40).contains(&id) || (53..=72).contains(&id));

                if is_clean_float
                    && id != 52
                    && id != 54
                    && id != 55
                    && id != 59
                    && id != 71
                    && id != 72
                {
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

                    if id == 55 {
                        emitter_type = Some(match raw_u32 {
                            12 => "LightningArc".to_string(),
                            13 => "BillboardSprite".to_string(),
                            14 => "RibbonTrail".to_string(),
                            15 => "MeshDebris".to_string(),
                            other => format!("Type_{}", other),
                        });
                    }
                }
            }
        }

        // 6. Short integers (1-2 bytes)
        if ptype == "raw" && (chunk.len() == 1 || chunk.len() == 2) {
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
            int_val,
            uint_val,
            color,
            target_emitter_id,
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
        type_id: format!("0x{:08X}", type_id),
        name,
        emitter_type,
        tag,
        delay_seconds,
        lifetime,
        spawn_rate,
        speed,
        size,
        color_rgba,
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
        let clean_type_id = em.type_id.trim_start_matches("0x");
        let type_id = u32::from_str_radix(clean_type_id, 16).unwrap_or(0x00730004);
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
                "int" => {
                    let i = prop.int_val.unwrap_or(0);
                    i.to_le_bytes().to_vec()
                }
                "uint" => {
                    let u = prop.uint_val.unwrap_or(0);
                    u.to_le_bytes().to_vec()
                }
                "emitter_link" => {
                    let target_id = prop.target_emitter_id.or(prop.uint_val).unwrap_or(0);
                    if target_id < 64 {
                        target_id.to_le_bytes().to_vec()
                    } else {
                        let link_elements =
                            vec![(10, target_id.to_le_bytes().to_vec()), (1, vec![0u8])];
                        build_chunk_from_elements(false, &link_elements)
                    }
                }
                "color" => {
                    if let Some(ref col) = prop.color {
                        let clean = col.trim_start_matches('#');
                        u32::from_str_radix(clean, 16)
                            .unwrap_or(0xFFFFFFFF)
                            .to_le_bytes()
                            .to_vec()
                    } else {
                        vec![0xFF, 0xFF, 0xFF, 0xFF]
                    }
                }
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
                let clean_sub_type = sub_em.type_id.trim_start_matches("0x");
                let sub_type = u32::from_str_radix(clean_sub_type, 16).unwrap_or(0x00730008);
                let mut sub_props = Vec::new();
                for sp in &sub_em.properties {
                    let s_chunk = match sp.ptype.as_str() {
                        "string" => write_length_prefixed_string(
                            sp.string_val.as_deref().unwrap_or(&sub_em.name),
                        ),
                        "float" => sp.float_val.unwrap_or(0.0).to_le_bytes().to_vec(),
                        "int" => sp.int_val.unwrap_or(0).to_le_bytes().to_vec(),
                        "uint" => sp.uint_val.unwrap_or(0).to_le_bytes().to_vec(),
                        "color" => {
                            let clean = sp
                                .color
                                .as_deref()
                                .unwrap_or("FFFFFFFF")
                                .trim_start_matches('#');
                            u32::from_str_radix(clean, 16)
                                .unwrap_or(0xFFFFFFFF)
                                .to_le_bytes()
                                .to_vec()
                        }
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
        22 => "Trigger Flags",
        23 => "Layer Enabled Flag",
        19 => "Sentinel Terminator",
        1 => "Particle Emitter Subcontainer Layers",
        _ => "VFX Graph Parameter",
    }
}

fn get_vfx_property_role(id: u32) -> &'static str {
    match id {
        20 => "Asset / Shader Tag",
        21 => "Spawn Position Offset (XYZ)",
        22 => "Render Mode / Particle Type",
        23 => "Blending & Alpha Mode",
        24 => "Arc Direction / Cylinder Dimensions (XYZ)",
        25 => "Emitter Loop Count / Direction",
        26 => "Rotation Spread / Angles (Radians)",
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
        42 => "Orientation Euler Angles / Tilt (Radians)",
        43 => "Spread Angle / Conical Aperture (Radians)",
        44 => "Dynamic Light Radius (Min/Max/Falloff)",
        45 => "Dynamic Light Intensity (Start/Peak/End)",
        46 => "Light Color (RGBA)",
        47 => "Keyframe Curve Progress",
        48 => "Keyframe Value",
        50 => "Trigger Link / Target Chain Emitter",
        51 => "Emitter Priority / Hardware Channel",
        52 => "Loop Mode (-1 = Infinite Loop)",
        53 => "Initial Delay Offset (s)",
        54 => "Burst Emit Count",
        55 => "Particle Primitive Type",
        56 => "Spawn Interval / Delta Time",
        57 => "Alpha Multiplier / Master Opacity",
        58 => "Particle Texture / Color Tint",
        59 => "Particle Behavior Bitmask",
        60 => "Collision Mode (0: None, 1: Ground, 4: World, 11: Terrain/Water)",
        61 => "Velocity Scale / Burst Force (-1 = Parent)",
        62 => "Air Resistance / Drag Damping",
        63 => "Inner Spawn Radius (m)",
        64 => "Outer Spawn Radius (m)",
        65 => "Velocity Box Min / Segment Length",
        66 => "Velocity Box Max / Segment Range",
        67 => "Uniform Scale Multiplier",
        68 => "Angular Randomness Spread",
        69 => "Trigger Event ID / Impact Tint",
        70 => "Max Particle Pool Capacity",
        71 => "Simulation Space (0: Local, 1: World)",
        72 => "Depth Sorting Priority (Z-Order)",
        1 => "Nested Sub-Emitter Layers",
        _ => "Emitter Parameter",
    }
}
