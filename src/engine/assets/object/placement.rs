use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use crate::engine::assets::{build_chunk_from_elements_with_endian, parse_chunk_elements};
use crate::engine::common::{Endian, read_length_prefixed_string};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ObjectModelBindingJson {
    pub object_path: String,
    pub model_name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PlacementConfigJson {
    pub physics_material_id: u32,
    pub is_enabled: bool,
    pub casts_shadows: bool,
    pub can_be_carried: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stance_id: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction_flags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_flags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_flags_hex: Option<String>,
}

pub fn parse_simple_model_binding(data: &[u8]) -> Option<ObjectModelBindingJson> {
    if let Ok((_, elements)) = parse_chunk_elements(data) {
        let mut obj_path = String::new();
        let mut model_name = String::new();

        for (id, chunk) in elements {
            if id == 20
                && let Some(s) = read_length_prefixed_string(&chunk)
            {
                obj_path = s;
            } else if id == 21
                && let Some(s) = read_length_prefixed_string(&chunk)
            {
                model_name = s;
            }
        }

        if !obj_path.is_empty() {
            return Some(ObjectModelBindingJson {
                object_path: obj_path,
                model_name,
            });
        }
    }
    None
}

pub fn build_simple_model_binding(obj_path: &str, model_name: &str, endian: Endian) -> Vec<u8> {
    let elements = vec![
        (20, endian.write_length_prefixed_string(obj_path)),
        (21, endian.write_length_prefixed_string(model_name)),
    ];
    build_chunk_from_elements_with_endian(false, &elements, endian)
}

pub fn parse_placed_object(chunk: &[u8]) -> Option<ObjectModelBindingJson> {
    if let Ok((_, elements)) = parse_chunk_elements(chunk) {
        for (id, data) in elements {
            if id == 30 {
                return parse_simple_model_binding(&data);
            }
        }
    }
    parse_simple_model_binding(chunk)
}

pub fn build_placed_object(binding: &ObjectModelBindingJson, endian: Endian) -> Vec<u8> {
    let model_bytes = build_simple_model_binding(&binding.object_path, &binding.model_name, endian);
    let sub_elements = vec![(30, model_bytes), (35, vec![0u8, 0, 0, 0])];
    build_chunk_from_elements_with_endian(false, &sub_elements, endian)
}

pub fn parse_placement_offset(chunk: &[u8]) -> Option<[f32; 3]> {
    if chunk.len() >= 15 && chunk[0] == 1 && chunk[1] == 20 {
        let mut cur = Cursor::new(&chunk[3..15]);
        let x = cur.read_f32::<LittleEndian>().ok()?;
        let y = cur.read_f32::<LittleEndian>().ok()?;
        let z = cur.read_f32::<LittleEndian>().ok()?;
        if x.is_finite() && y.is_finite() && z.is_finite() {
            return Some([x, y, z]);
        }
    }
    None
}

pub fn build_placement_offset(offset: [f32; 3], endian: Endian) -> Vec<u8> {
    let mut out = vec![1u8, 20, 0];
    let _ = endian.write_f32(&mut out, offset[0]);
    let _ = endian.write_f32(&mut out, offset[1]);
    let _ = endian.write_f32(&mut out, offset[2]);
    out
}
