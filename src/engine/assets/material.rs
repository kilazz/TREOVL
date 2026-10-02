use anyhow::Result;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::{build_typed_container, parse_typed_container};
use crate::engine::common::read_length_prefixed_string;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MaterialJson {
    pub _engine_metadata: MaterialEngineMetadataJson,
    pub material_name: String,
    pub blocks: Vec<MaterialBlock>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct MaterialEngineMetadataJson {
    pub type_id_hex: String,
    pub engine_generation: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MaterialBlock {
    pub id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub btype: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub float_value: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uint_value: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ptr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

fn get_material_role(type_id: u32, chunk_id: u32) -> Option<&'static str> {
    match type_id {
        0x00410608 => match chunk_id {
            30 => Some("Diffuse Texture"),
            _ => None,
        },
        0x0041060A => match chunk_id {
            30 => Some("Diffuse Texture"),
            40 => Some("Alpha Cutoff Threshold"),
            50 => Some("Normal Map"),
            _ => None,
        },
        0x00410612 => match chunk_id {
            30 => Some("Diffuse Texture"),
            42 => Some("Reflection Map"),
            43 => Some("Specular Color"),
            50 => Some("Normal Map"),
            _ => None,
        },
        0x00410630 => match chunk_id {
            30 => Some("Lightbeam Texture"),
            31 => Some("Additive Blending Flag"),
            40 => Some("Blend Mode"),
            41 => Some("Ray Brightness / Alpha"),
            52 => Some("UV Scroll Speed X"),
            53 => Some("UV Tiling X"),
            54 => Some("UV Tiling Y"),
            _ => None,
        },
        0x00410636 => match chunk_id {
            30 => Some("Diffuse Texture"),
            42 => Some("Normal Map"),
            43 => Some("Reflection Map"),
            44 => Some("Mask (Specular / Metallic)"),
            46 => Some("Normal Map Strength"),
            47 => Some("Specular Power / Gloss"),
            48 => Some("Sampler / Blend Flags"),
            52 => Some("Reflection Intensity"),
            _ => None,
        },
        0x00410624 => match chunk_id {
            30 => Some("Diffuse / Base Color"),
            42 => Some("Normal Map"),
            43 => Some("Reflection Cubemap"),
            44 => Some("Mask (Roughness / Metal / AO)"),
            45 => Some("Mask Opacity / Threshold"),
            46 => Some("Normal Strength / Roughness"),
            47 => Some("Specular Power / Shininess"),
            49 => Some("Detail / Secondary Texture"),
            50 => Some("UV Tiling X"),
            51 => Some("UV Tiling Y"),
            52 => Some("Reflection Intensity"),
            _ => None,
        },
        0x00410632 => match chunk_id {
            30 => Some("Diffuse Texture"),
            41 => Some("Animation Speed X"),
            50 => Some("Animation Speed Y"),
            _ => None,
        },
        0x00460009 => match chunk_id {
            30 => Some("Base Terrain Texture"),
            32 => Some("Layer 2 Texture Link"),
            42 => Some("Normal Map"),
            50 => Some("UV Scale X"),
            51 => Some("UV Scale Y"),
            _ => None,
        },
        0x00460015 | 0x00460013 => match chunk_id {
            30 => Some("Water Flow Normal Map 1"),
            42 => Some("Water Flow Normal Map 2"),
            43 => Some("Sky Reflection Cubemap"),
            45 => Some("Flow Speed"),
            46 => Some("Wave Amplitude"),
            _ => None,
        },
        0x0046001F => match chunk_id {
            30 => Some("Diffuse / Foliage Texture"),
            40 => Some("Alpha Cutoff Threshold"),
            42 => Some("Wind Sway Amplitude"),
            _ => None,
        },
        0x00464620 => match chunk_id {
            30 => Some("Decal Texture"),
            42 => Some("Decal Normal Map"),
            45 => Some("Decal Fade Distance"),
            _ => None,
        },
        _ => None,
    }
}

fn get_material_info(type_id: u32) -> (&'static str, &'static str) {
    match type_id {
        0x00410608 => ("Overlord 1", "Diffuse Material"),
        0x0041060A => ("Overlord 1", "Standard Material"),
        0x0041060F => ("Overlord 1", "Diffuse + Specular Material"),
        0x00410612 => ("Overlord 1", "Standard Material (+Spec)"),
        0x0041061B => ("Overlord 1", "Vegetation Material"),
        0x00410620 => ("Overlord 1", "Props Material"),
        0x00410626 => ("Overlord 1", "Complex Material (DNMS spec/height)"),
        0x00410628 => ("Overlord 1", "Vegetation D Material"),
        0x0041062A => ("Overlord 1", "Vegetation Normal Material"),
        0x00410630 => ("Overlord 1", "Animated Light Rays / Godrays Material"),
        0x00410636 => ("Overlord 1", "Armor Material (DNRM)"),
        0x00410624 => ("Overlord 2", "Masked PBR Material (DNRMS)"),
        0x00410632 => ("Overlord 2", "Animated Surface Material"),
        0x00460009 => ("Overlord 2", "Terrain Blend Material"),
        0x00460013 => ("Overlord 2", "Water Surface Material"),
        0x00460015 => ("Overlord 2", "Environmental Fluid Material"),
        0x0046001F => ("Overlord 2", "Foliage / Flora Material"),
        0x00464608 => ("Overlord 2", "Atmospheric Skybox Material"),
        0x00464614 => ("Overlord 2", "Particle / FX Material"),
        0x00464620 => ("Overlord 2", "Decal Overlay Material"),
        _ => {
            if (type_id >> 16) == 0x0046 {
                ("Overlord 2", "Unknown Overlord 2 Material")
            } else {
                ("Overlord 1", "Unknown Overlord 1 Material")
            }
        }
    }
}

pub fn export_material_to_json(chunk_data: &[u8]) -> Result<String> {
    let (type_id, elements) = parse_typed_container(chunk_data)?;
    let mut blocks = Vec::new();
    let (engine_gen, mat_name) = get_material_info(type_id);

    for (id, chunk) in elements {
        let mut btype = "raw".to_string();
        let mut value = Some(hex::encode_upper(&chunk));
        let mut float_value = None;
        let mut uint_value = None;
        let mut ptr = None;
        let mut name = None;

        if let Some(s) = read_length_prefixed_string(&chunk) {
            btype = "string".to_string();
            value = Some(s);
        }

        if btype == "raw" && !chunk.is_empty() {
            let num_offsets = chunk[0] as usize;
            if (num_offsets == 1 || num_offsets == 2) && chunk.len() > 1 + num_offsets * 2 {
                let mut p = 1;
                let mut str_offsets = Vec::new();
                for _ in 0..num_offsets {
                    str_offsets.push(chunk[p + 1] as usize);
                    p += 2;
                }

                let t_base = p;
                let s1_start = t_base + str_offsets[0];

                if s1_start < chunk.len()
                    && let Some(s) = read_length_prefixed_string(&chunk[s1_start..])
                {
                    ptr = Some(s);
                    btype = "texture_link".to_string();
                    value = None;
                }

                if num_offsets == 2 && btype == "texture_link" {
                    let s2_start = t_base + str_offsets[1];
                    if s2_start < chunk.len()
                        && let Some(s) = read_length_prefixed_string(&chunk[s2_start..])
                    {
                        name = Some(s);
                    }
                }
            }
        }

        if btype == "raw" && chunk.len() == 4 && (41..=55).contains(&id) {
            let mut cur = Cursor::new(&chunk);
            let raw_u32 = cur.read_u32::<LittleEndian>().unwrap_or(0);
            let f = f32::from_bits(raw_u32);

            if f.is_finite() && !f.is_nan() && f.abs() >= 1e-5 && f.abs() <= 100_000.0 {
                btype = "float".to_string();
                float_value = Some(f);
                value = None;
            } else if raw_u32 < 10_000 {
                btype = "uint".to_string();
                uint_value = Some(raw_u32);
                value = None;
            }
        }

        let role = get_material_role(type_id, id).map(|s| s.to_string());

        blocks.push(MaterialBlock {
            id,
            role,
            btype,
            value,
            float_value,
            uint_value,
            ptr,
            name,
        });
    }

    let metadata = MaterialEngineMetadataJson {
        type_id_hex: format!("{:08X}", type_id),
        engine_generation: engine_gen.to_string(),
    };

    let mat_json = MaterialJson {
        _engine_metadata: metadata,
        material_name: mat_name.to_string(),
        blocks,
    };

    serde_json::to_string_pretty(&mat_json).map_err(|e| anyhow::anyhow!(e))
}

pub fn import_material_from_json(json_str: &str) -> Result<Vec<u8>> {
    let mat_json: MaterialJson = serde_json::from_str(json_str)?;
    let type_id = u32::from_str_radix(&mat_json._engine_metadata.type_id_hex, 16)
        .map_err(|_| anyhow::anyhow!("Invalid hexadecimal Type ID metadata"))?;

    let mut elements = Vec::new();

    for b in mat_json.blocks {
        let mut chunk = Vec::new();
        match b.btype.as_str() {
            "string" => {
                let s = b.value.unwrap_or_default().into_bytes();
                chunk.write_u32::<LittleEndian>(s.len() as u32)?;
                chunk.extend(s);
            }
            "float" => {
                let f = b.float_value.unwrap_or(0.0);
                chunk.write_f32::<LittleEndian>(f)?;
            }
            "uint" => {
                let u = b.uint_value.unwrap_or(0);
                chunk.write_u32::<LittleEndian>(u)?;
            }
            "texture_link" => {
                let ptr = b.ptr.unwrap_or_default().into_bytes();
                let name = b.name.unwrap_or_default().into_bytes();

                if !name.is_empty() {
                    chunk.push(2);
                    chunk.extend_from_slice(&[20, 0]);
                    chunk.extend_from_slice(&[21, (4 + ptr.len()) as u8]);

                    chunk.write_u32::<LittleEndian>(ptr.len() as u32)?;
                    chunk.extend(ptr);
                    chunk.write_u32::<LittleEndian>(name.len() as u32)?;
                    chunk.extend(name);
                } else {
                    chunk.push(1);
                    chunk.extend_from_slice(&[20, 0]);
                    chunk.write_u32::<LittleEndian>(ptr.len() as u32)?;
                    chunk.extend(ptr);
                }
            }
            _ => {
                let hex_str = b.value.unwrap_or_default();
                let bytes = hex::decode(hex_str.trim())
                    .map_err(|_| anyhow::anyhow!("Invalid hex sequence in block ID {}", b.id))?;
                chunk.extend(bytes);
            }
        }
        elements.push((b.id, chunk));
    }

    Ok(build_typed_container(type_id, &elements))
}
