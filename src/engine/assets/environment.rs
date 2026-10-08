use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::{
    build_chunk_from_elements, build_typed_container, parse_chunk_elements, parse_typed_container,
};
use crate::engine::common::{Endian, parse_entity_handle, read_length_prefixed_string};

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct EnvironmentMetadataJson {
    pub is_typed_container: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_id_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_handle: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EnvironmentConfigJson {
    pub _engine_metadata: EnvironmentMetadataJson,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags_hex: Option<String>,

    // Fog Settings
    pub fog_near: f32,
    pub fog_far: f32,
    pub fog_color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_fog_near: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_fog_far: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_fog_color: Option<String>,

    // Illumination & Ambient Lighting
    pub ambient_color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ground_ambient_color: Option<String>,
    pub sun_direction: [f32; 3],
    pub sun_color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sun_intensity: Option<f32>,
    pub fill_direction: [f32; 3],
    pub fill_color: String,
    pub fill_range: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_range: Option<f32>,

    // Sky Dome & Cloud Simulation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sky_dome_distance: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sky_horizon_height: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sky_zenith_height: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sky_blend_mode: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fog_mode: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow_quality: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_shadow_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_density: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub sky_gradient_colors: Vec<String>,

    // Post-Processing & Color Grading
    pub gamma: f32,
    pub exposure: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contrast: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saturation: Option<f32>,
    pub bloom_threshold: f32,
    pub bloom_intensity: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bloom_blur: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bloom_tint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_process_tint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_of_field_near: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_of_field_far: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_of_field_intensity: Option<f32>,

    // Water & Wind Simulation
    pub water_tint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub water_reflection_strength: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub water_transparency: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub water_refraction_scale: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub water_wave_height: Option<f32>,
    pub far_clip: f32,
    pub flow_vector: [f32; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wind_speed: Option<f32>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub raw_unmapped_chunks: Vec<(u32, String)>,
}

pub fn export_environment_to_json(data: &[u8]) -> Result<String> {
    let (is_typed, type_id, elements) = if let Ok((t_id, elems)) = parse_typed_container(data) {
        (true, Some(t_id), elems)
    } else if let Ok((_, elems)) = parse_chunk_elements(data) {
        (false, None, elems)
    } else {
        bail!("Failed to parse Environment .env container table");
    };

    let parse_color = |b: &[u8]| -> String {
        if b.len() >= 3 {
            format!("#{:02X}{:02X}{:02X}", b[0], b[1], b[2])
        } else {
            "#000000".into()
        }
    };

    let parse_float = |b: &[u8]| -> f32 {
        if b.len() >= 4 {
            f32::from_le_bytes(b[0..4].try_into().unwrap_or_default())
        } else {
            0.0
        }
    };

    let mut cfg = EnvironmentConfigJson {
        _engine_metadata: EnvironmentMetadataJson {
            is_typed_container: is_typed,
            type_id_hex: type_id.map(|t| format!("{:08X}", t)),
            profile_handle: None,
        },
        group_path: None,
        profile_name: None,
        flags_hex: None,
        fog_near: 20.0,
        fog_far: 40.0,
        fog_color: "#377479".into(),
        height_fog_near: None,
        height_fog_far: None,
        height_fog_color: None,
        ambient_color: "#0D1015".into(),
        ground_ambient_color: None,
        sun_direction: [0.395, -0.854, -0.336],
        sun_color: "#020202".into(),
        sun_intensity: None,
        fill_direction: [-0.816, -0.408, 0.408],
        fill_color: "#403122".into(),
        fill_range: 10.0,
        light_range: None,
        sky_dome_distance: None,
        sky_horizon_height: None,
        sky_zenith_height: None,
        sky_blend_mode: None,
        fog_mode: None,
        shadow_quality: None,
        cloud_color: None,
        cloud_shadow_color: None,
        cloud_density: None,
        sky_gradient_colors: Vec::new(),
        gamma: 2.2,
        exposure: 0.3,
        contrast: None,
        saturation: None,
        bloom_threshold: 0.05,
        bloom_intensity: 1.5,
        bloom_blur: None,
        bloom_tint: None,
        post_process_tint: None,
        depth_of_field_near: None,
        depth_of_field_far: None,
        depth_of_field_intensity: None,
        water_tint: "#ADBCC5".into(),
        water_reflection_strength: None,
        water_transparency: None,
        water_refraction_scale: None,
        water_wave_height: None,
        far_clip: 64.0,
        flow_vector: [0.6, -1.2],
        wind_speed: None,
        raw_unmapped_chunks: Vec::new(),
    };

    for (id, chunk) in elements {
        match id {
            20 => cfg.group_path = read_length_prefixed_string(&chunk),
            21 => cfg.profile_name = read_length_prefixed_string(&chunk),
            22 => {
                cfg.flags_hex = Some(hex::encode_upper(&chunk));
                if chunk.len() >= 4 {
                    let val = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
                    if let Some(handle) = parse_entity_handle(val) {
                        cfg._engine_metadata.profile_handle = Some(handle.uid);
                    }
                }
            }
            0x0BCC => cfg.fog_near = parse_float(&chunk),
            0x0BCD => cfg.fog_far = parse_float(&chunk),
            0x0BCE => cfg.fog_color = parse_color(&chunk),
            0x0BCF => cfg.height_fog_near = Some(parse_float(&chunk)),
            0x0BD0 => cfg.height_fog_far = Some(parse_float(&chunk)),
            0x0BD1 => cfg.height_fog_color = Some(parse_color(&chunk)),
            0x0BD2 => cfg.ambient_color = parse_color(&chunk),
            0x0BD3 => cfg.sun_direction[0] = parse_float(&chunk),
            0x0BD4 => cfg.sun_direction[1] = parse_float(&chunk),
            0x0BD5 => cfg.sun_direction[2] = parse_float(&chunk),
            0x0BD6 => cfg.sun_color = parse_color(&chunk),
            0x0BD7 => cfg.fill_direction[0] = parse_float(&chunk),
            0x0BD8 => cfg.fill_direction[1] = parse_float(&chunk),
            0x0BD9 => cfg.fill_direction[2] = parse_float(&chunk),
            0x0BDE => cfg.fill_color = parse_color(&chunk),
            0x0BDF => cfg.sun_intensity = Some(parse_float(&chunk)),
            0x0BE2 => cfg.ground_ambient_color = Some(parse_color(&chunk)),
            0x0BE5 => cfg.light_range = Some(parse_float(&chunk)),
            0x0BE6 if !chunk.is_empty() => cfg.sky_blend_mode = Some(chunk[0]),
            0x0BE7 if !chunk.is_empty() => cfg.fog_mode = Some(chunk[0]),
            0x0BE8 if !chunk.is_empty() => cfg.shadow_quality = Some(chunk[0]),
            0x0BE9 => cfg.sky_dome_distance = Some(parse_float(&chunk)),
            0x0BEA => {
                let mut colors = Vec::new();
                for slice in chunk.as_chunks::<4>().0 {
                    colors.push(format!("#{:02X}{:02X}{:02X}", slice[0], slice[1], slice[2]));
                }
                cfg.sky_gradient_colors = colors;
            }
            0x0BEB => cfg.sky_horizon_height = Some(parse_float(&chunk)),
            0x0BED => cfg.sky_zenith_height = Some(parse_float(&chunk)),
            0x0BEE => cfg.bloom_threshold = parse_float(&chunk),
            0x0BEF => cfg.bloom_tint = Some(parse_color(&chunk)),
            0x0BF0 => cfg.cloud_color = Some(parse_color(&chunk)),
            0x0BF1 => cfg.cloud_shadow_color = Some(parse_color(&chunk)),
            0x0BF2 => cfg.cloud_density = Some(parse_float(&chunk)),
            0x0BF3 => cfg.gamma = parse_float(&chunk),
            0x0BF4 => cfg.exposure = parse_float(&chunk),
            0x0BF5 => cfg.contrast = Some(parse_float(&chunk)),
            0x0BF6 => cfg.bloom_intensity = parse_float(&chunk),
            0x0BF7 => cfg.bloom_blur = Some(parse_float(&chunk)),
            0x0BF8 => cfg.saturation = Some(parse_float(&chunk)),
            0x0BF9 => cfg.post_process_tint = Some(parse_color(&chunk)),
            0x0BFA => cfg.depth_of_field_near = Some(parse_float(&chunk)),
            0x0BFB => cfg.depth_of_field_far = Some(parse_float(&chunk)),
            0x0BFC => cfg.depth_of_field_intensity = Some(parse_float(&chunk)),
            0x0BFD => cfg.wind_speed = Some(parse_float(&chunk)),
            0x0BFE => cfg.water_tint = parse_color(&chunk),
            0x0BFF => cfg.far_clip = parse_float(&chunk),
            0x0C02 => cfg.water_reflection_strength = Some(parse_float(&chunk)),
            0x0C03 => cfg.water_wave_height = Some(parse_float(&chunk)),
            0x0C04 => cfg.water_refraction_scale = Some(parse_float(&chunk)),
            0x0C09 => cfg.water_transparency = Some(parse_float(&chunk)),
            0x0C0B if chunk.len() >= 8 => {
                let mut cur = Cursor::new(&chunk);
                cfg.flow_vector[0] = cur.read_f32::<LittleEndian>()?;
                cfg.flow_vector[1] = cur.read_f32::<LittleEndian>()?;
            }
            0x10CD => cfg.fill_range = parse_float(&chunk),
            19 | 1 => {} // Sentinel & Terminator
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

    let (is_typed, type_id, mut elements) =
        if let Ok((t_id, elems)) = parse_typed_container(baseline) {
            (true, Some(t_id), elems)
        } else if let Ok((_, elems)) = parse_chunk_elements(baseline) {
            (false, None, elems)
        } else {
            bail!("Invalid baseline for environment import");
        };

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

    let endian = Endian::Little;

    for (id, chunk) in elements.iter_mut() {
        match *id {
            20 => {
                if let Some(ref s) = cfg.group_path {
                    *chunk = endian.write_length_prefixed_string(s);
                }
            }
            21 => {
                if let Some(ref s) = cfg.profile_name {
                    *chunk = endian.write_length_prefixed_string(s);
                }
            }
            22 => {
                if let Some(ref s) = cfg.flags_hex {
                    *chunk = hex::decode(s).unwrap_or_default();
                } else if let Some(h) = cfg._engine_metadata.profile_handle {
                    *chunk = endian
                        .u32_to_bytes(0x4D00_0000 | (h & 0x00FF_FFFF))
                        .to_vec();
                }
            }
            0x0BCC => *chunk = cfg.fog_near.to_le_bytes().to_vec(),
            0x0BCD => *chunk = cfg.fog_far.to_le_bytes().to_vec(),
            0x0BCE => *chunk = parse_hex_color(&cfg.fog_color).to_vec(),
            0x0BCF => {
                if let Some(v) = cfg.height_fog_near {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0BD0 => {
                if let Some(v) = cfg.height_fog_far {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0BD1 => {
                if let Some(ref c) = cfg.height_fog_color {
                    *chunk = parse_hex_color(c).to_vec();
                }
            }
            0x0BD2 => *chunk = parse_hex_color(&cfg.ambient_color).to_vec(),
            0x0BD3 => *chunk = cfg.sun_direction[0].to_le_bytes().to_vec(),
            0x0BD4 => *chunk = cfg.sun_direction[1].to_le_bytes().to_vec(),
            0x0BD5 => *chunk = cfg.sun_direction[2].to_le_bytes().to_vec(),
            0x0BD6 => *chunk = parse_hex_color(&cfg.sun_color).to_vec(),
            0x0BD7 => *chunk = cfg.fill_direction[0].to_le_bytes().to_vec(),
            0x0BD8 => *chunk = cfg.fill_direction[1].to_le_bytes().to_vec(),
            0x0BD9 => *chunk = cfg.fill_direction[2].to_le_bytes().to_vec(),
            0x0BDE => *chunk = parse_hex_color(&cfg.fill_color).to_vec(),
            0x0BDF => {
                if let Some(v) = cfg.sun_intensity {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0BE2 => {
                if let Some(ref c) = cfg.ground_ambient_color {
                    *chunk = parse_hex_color(c).to_vec();
                }
            }
            0x0BE5 => {
                if let Some(v) = cfg.light_range {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0BE6 => {
                if let Some(v) = cfg.sky_blend_mode {
                    *chunk = vec![v];
                }
            }
            0x0BE7 => {
                if let Some(v) = cfg.fog_mode {
                    *chunk = vec![v];
                }
            }
            0x0BE8 => {
                if let Some(v) = cfg.shadow_quality {
                    *chunk = vec![v];
                }
            }
            0x0BE9 => {
                if let Some(v) = cfg.sky_dome_distance {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0BEA => {
                if !cfg.sky_gradient_colors.is_empty() {
                    let mut b = Vec::with_capacity(cfg.sky_gradient_colors.len() * 4);
                    for col in &cfg.sky_gradient_colors {
                        b.extend_from_slice(&parse_hex_color(col));
                    }
                    *chunk = b;
                }
            }
            0x0BEB => {
                if let Some(v) = cfg.sky_horizon_height {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0BED => {
                if let Some(v) = cfg.sky_zenith_height {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0BEE => *chunk = cfg.bloom_threshold.to_le_bytes().to_vec(),
            0x0BEF => {
                if let Some(ref c) = cfg.bloom_tint {
                    *chunk = parse_hex_color(c).to_vec();
                }
            }
            0x0BF0 => {
                if let Some(ref c) = cfg.cloud_color {
                    *chunk = parse_hex_color(c).to_vec();
                }
            }
            0x0BF1 => {
                if let Some(ref c) = cfg.cloud_shadow_color {
                    *chunk = parse_hex_color(c).to_vec();
                }
            }
            0x0BF2 => {
                if let Some(v) = cfg.cloud_density {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0BF3 => *chunk = cfg.gamma.to_le_bytes().to_vec(),
            0x0BF4 => *chunk = cfg.exposure.to_le_bytes().to_vec(),
            0x0BF5 => {
                if let Some(v) = cfg.contrast {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0BF6 => *chunk = cfg.bloom_intensity.to_le_bytes().to_vec(),
            0x0BF7 => {
                if let Some(v) = cfg.bloom_blur {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0BF8 => {
                if let Some(v) = cfg.saturation {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0BF9 => {
                if let Some(ref c) = cfg.post_process_tint {
                    *chunk = parse_hex_color(c).to_vec();
                }
            }
            0x0BFA => {
                if let Some(v) = cfg.depth_of_field_near {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0BFB => {
                if let Some(v) = cfg.depth_of_field_far {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0BFC => {
                if let Some(v) = cfg.depth_of_field_intensity {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0BFD => {
                if let Some(v) = cfg.wind_speed {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0BFE => *chunk = parse_hex_color(&cfg.water_tint).to_vec(),
            0x0BFF => *chunk = cfg.far_clip.to_le_bytes().to_vec(),
            0x0C02 => {
                if let Some(v) = cfg.water_reflection_strength {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0C03 => {
                if let Some(v) = cfg.water_wave_height {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0C04 => {
                if let Some(v) = cfg.water_refraction_scale {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0C09 => {
                if let Some(v) = cfg.water_transparency {
                    *chunk = v.to_le_bytes().to_vec();
                }
            }
            0x0C0B => {
                let mut b = Vec::with_capacity(8);
                b.extend_from_slice(&cfg.flow_vector[0].to_le_bytes());
                b.extend_from_slice(&cfg.flow_vector[1].to_le_bytes());
                *chunk = b;
            }
            0x10CD => *chunk = cfg.fill_range.to_le_bytes().to_vec(),
            _ => {}
        }
    }

    if is_typed {
        Ok(build_typed_container(
            type_id.unwrap_or(0x04000083),
            &elements,
        ))
    } else {
        Ok(build_chunk_from_elements(false, &elements))
    }
}
