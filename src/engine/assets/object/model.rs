use anyhow::Result;
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::physics::{BoneGroupJson, BoundingBoxJson, EntityPhysicsStateJson, FullObjectBoneJson};
use super::placement::{ObjectModelBindingJson, PlacementConfigJson};
use crate::engine::assets::{
    build_chunk_from_elements_with_endian, build_typed_container_with_endian, parse_chunk_elements,
    parse_typed_container,
};
use crate::engine::common::{Endian, read_length_prefixed_string};

#[inline]
pub fn is_empty_container(chunk: &[u8]) -> bool {
    chunk.is_empty() || chunk == [0u8]
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ObjectEntityJson {
    pub _engine_metadata: ObjectEngineMetadataJson,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounding_box: Option<BoundingBoxJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_animation: Option<DefaultAnimationLinkJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physics_state: Option<EntityPhysicsStateJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub ragdoll_bone_groups: Vec<BoneGroupJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub mesh_bindings: Vec<MeshMaterialBindingJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stand_model: Option<ObjectModelBindingJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placed_object: Option<ObjectModelBindingJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement_offset: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement_config: Option<PlacementConfigJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub bones: Vec<FullObjectBoneJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub attachments: Vec<AttachmentSlotJson>,
    #[serde(default = "default_true")]
    pub has_sentinel_terminator: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ObjectEngineMetadataJson {
    pub type_id_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub raw_fallbacks: Vec<RawFallbackComponentJson>,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RawFallbackComponentJson {
    pub id: u32,
    pub reason: String,
    pub hex: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MeshMaterialBindingJson {
    pub mesh_path: String,
    pub mesh_part_name: String,
    pub material_path: String,
    pub material_name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DefaultAnimationLinkJson {
    pub anim_group: String,
    pub clip_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AttachmentSlotJson {
    pub slot_id: u32,
    pub data_hex: String,
}

pub fn parse_mesh_material_bindings(chunk_data: &[u8]) -> Vec<MeshMaterialBindingJson> {
    let mut bindings = Vec::new();
    let elements = if let Ok((_, elems)) = parse_chunk_elements(chunk_data) {
        elems
    } else {
        return bindings;
    };

    for (_, elem_data) in elements {
        if elem_data.starts_with(b"\x67\x00\x41\x00")
            && let Ok((_, sub_parts)) = parse_typed_container(&elem_data)
        {
            let mut mesh_path = String::new();
            let mut mesh_part_name = String::new();
            let mut material_path = String::new();
            let mut material_name = String::new();

            for (part_id, part_data) in sub_parts {
                if let Ok((_, str_elements)) = parse_chunk_elements(&part_data) {
                    for (str_id, str_data) in str_elements {
                        if str_id == 20
                            && let Some(s) = read_length_prefixed_string(&str_data)
                        {
                            if part_id == 31 {
                                mesh_path = s;
                            } else if part_id == 33 {
                                material_path = s;
                            }
                        } else if str_id == 21
                            && let Some(s) = read_length_prefixed_string(&str_data)
                        {
                            if part_id == 31 {
                                mesh_part_name = s;
                            } else if part_id == 33 {
                                material_name = s;
                            }
                        }
                    }
                }
            }

            if !mesh_path.is_empty() || !material_path.is_empty() {
                bindings.push(MeshMaterialBindingJson {
                    mesh_path,
                    mesh_part_name,
                    material_path,
                    material_name,
                });
            }
        }
    }

    bindings
}

pub fn rebuild_mesh_material_bindings(
    bindings: &[MeshMaterialBindingJson],
    endian: Endian,
) -> Result<Vec<u8>> {
    let mut binding_chunks = Vec::new();

    for (idx, b) in bindings.iter().enumerate() {
        let mesh_elements = vec![
            (20, endian.write_length_prefixed_string(&b.mesh_path)),
            (21, endian.write_length_prefixed_string(&b.mesh_part_name)),
        ];
        let mesh_container = build_chunk_from_elements_with_endian(false, &mesh_elements, endian);

        let mat_elements = vec![
            (20, endian.write_length_prefixed_string(&b.material_path)),
            (21, endian.write_length_prefixed_string(&b.material_name)),
        ];
        let mat_container = build_chunk_from_elements_with_endian(false, &mat_elements, endian);

        let binding_sub = vec![(31, mesh_container), (33, mat_container)];
        let binding_chunk = build_typed_container_with_endian(0x00410067, &binding_sub, endian);
        binding_chunks.push((idx as u32, binding_chunk));
    }

    Ok(build_chunk_from_elements_with_endian(
        true,
        &binding_chunks,
        endian,
    ))
}

pub fn parse_animation_linkage(chunk_data: &[u8]) -> Option<DefaultAnimationLinkJson> {
    if let Ok((_, elements)) = parse_chunk_elements(chunk_data) {
        let mut anim_group = String::new();
        let mut clip_name = String::new();
        let mut track_name = None;

        for (id, data) in elements {
            match id {
                20 => {
                    if let Some(s) = read_length_prefixed_string(&data) {
                        anim_group = s;
                    }
                }
                21 => {
                    if let Some(s) = read_length_prefixed_string(&data) {
                        clip_name = s;
                    }
                }
                _ => {
                    if track_name.is_none()
                        && let Some(s) = read_length_prefixed_string(&data)
                    {
                        track_name = Some(s);
                    }
                }
            }
        }

        if !clip_name.is_empty() {
            return Some(DefaultAnimationLinkJson {
                anim_group,
                clip_name,
                track_name,
            });
        }
    }
    None
}

pub fn rebuild_animation_linkage(anim: &DefaultAnimationLinkJson, endian: Endian) -> Vec<u8> {
    let mut elements = vec![
        (20, endian.write_length_prefixed_string(&anim.anim_group)),
        (21, endian.write_length_prefixed_string(&anim.clip_name)),
    ];
    if let Some(ref track) = anim.track_name {
        elements.push((22, endian.write_length_prefixed_string(track)));
    }
    build_chunk_from_elements_with_endian(false, &elements, endian)
}

pub fn parse_attachment_slots(chunk: &[u8]) -> Vec<AttachmentSlotJson> {
    let mut out = Vec::new();
    if is_empty_container(chunk) {
        return out;
    }
    if let Ok((_, elements)) = parse_chunk_elements(chunk) {
        for (id, data) in elements {
            out.push(AttachmentSlotJson {
                slot_id: id,
                data_hex: hex::encode_upper(data),
            });
        }
    }
    out
}

pub fn rebuild_attachment_slots(slots: &[AttachmentSlotJson], endian: Endian) -> Vec<u8> {
    if slots.is_empty() {
        return vec![0u8];
    }
    let mut elements = Vec::new();
    for s in slots {
        if let Ok(b) = hex::decode(&s.data_hex) {
            elements.push((s.slot_id, b));
        }
    }
    build_chunk_from_elements_with_endian(false, &elements, endian)
}

pub fn read_scale_vector(data: &[u8]) -> Option<[f32; 3]> {
    if data.len() >= 12 {
        let mut cur = Cursor::new(data);
        let x = cur.read_f32::<LittleEndian>().ok()?;
        let y = cur.read_f32::<LittleEndian>().ok()?;
        let z = cur.read_f32::<LittleEndian>().ok()?;
        if x.is_finite() && y.is_finite() && z.is_finite() {
            return Some([x, y, z]);
        }
    }
    None
}

pub fn write_scale_vector(s: [f32; 3], endian: Endian) -> Vec<u8> {
    let mut out = Vec::with_capacity(12);
    let _ = endian.write_f32(&mut out, s[0]);
    let _ = endian.write_f32(&mut out, s[1]);
    let _ = endian.write_f32(&mut out, s[2]);
    out
}
