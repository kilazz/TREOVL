use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::Path;

use super::animation::{ObjectBone, RawObjectBone};
use super::{
    build_chunk_from_elements, build_typed_container, parse_chunk_elements, parse_typed_container,
};
use crate::engine::common::{read_length_prefixed_string, write_length_prefixed_string};
use crate::engine::math::{Vector3, Vector4};

#[inline]
fn is_empty_container(chunk: &[u8]) -> bool {
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
    pub raw_flags_hex: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RawFallbackComponentJson {
    pub id: u32,
    pub reason: String,
    pub hex: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BoundingBoxJson {
    pub center: [f32; 3],
    pub half_extents: [f32; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation_matrix: Option<[[f32; 3]; 3]>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EntityPhysicsStateJson {
    pub offset: [f32; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collision_radius: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logic_flags: Option<PhysicsFlagsJson>,
    pub raw_w_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_parameters: Option<[u32; 4]>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PhysicsFlagsJson {
    pub is_equippable: bool,
    pub cast_shadows: bool,
    pub spatial_query_enabled: bool,
    pub is_dynamic_actor: bool,
    pub unmapped_bits_hex: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BoneGroupJson {
    pub group_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub bone_ids: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub bone_names: Vec<String>,
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
pub struct FullObjectBoneJson {
    pub id: usize,
    pub name: String,
    pub bone_id: i32,
    pub parent_index: i32,
    pub first_child_index: i32,
    pub next_sibling_index: i32,
    pub aux_id: i32,
    pub translation: [f32; 3],
    pub rotation_quat: [f32; 4],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform_matrix: Option<[f32; 16]>,
}

impl FullObjectBoneJson {
    pub fn from_object_bone(b: &ObjectBone, id: usize) -> Self {
        Self {
            id,
            name: b.name.clone(),
            bone_id: b.bone_id,
            parent_index: b.parent_index,
            first_child_index: b.first_child_index,
            next_sibling_index: b.next_sibling_index,
            aux_id: b.aux_id,
            translation: [b.translation.x, b.translation.y, b.translation.z],
            rotation_quat: [b.rotation.x, b.rotation.y, b.rotation.z, b.rotation.w],
            transform_matrix: Some(b.matrix),
        }
    }

    pub fn to_object_bone(&self) -> ObjectBone {
        ObjectBone {
            name: self.name.clone(),
            matrix: self.transform_matrix.unwrap_or_else(|| {
                let mut m = [0.0f32; 16];
                m[0] = 1.0;
                m[5] = 1.0;
                m[10] = 1.0;
                m[15] = 1.0;
                m[12] = self.translation[0];
                m[13] = self.translation[1];
                m[14] = self.translation[2];
                m
            }),
            rotation: Vector4 {
                x: self.rotation_quat[0],
                y: self.rotation_quat[1],
                z: self.rotation_quat[2],
                w: self.rotation_quat[3],
            },
            translation: Vector3 {
                x: self.translation[0],
                y: self.translation[1],
                z: self.translation[2],
            },
            bone_id: self.bone_id,
            parent_index: self.parent_index,
            first_child_index: self.first_child_index,
            next_sibling_index: self.next_sibling_index,
            aux_id: self.aux_id,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AttachmentSlotJson {
    pub slot_id: u32,
    pub data_hex: String,
}

pub fn export_object_to_json(data: &[u8], output_dir: Option<&Path>) -> Result<String> {
    if data.len() < 5 {
        bail!("Chunk data too short to be an Object Entity container.");
    }

    let (type_id, elements) = parse_typed_container(data)
        .context("Failed to parse Object Entity root typed container")?;

    let is_placement_object = type_id == 0x00464621;

    let class_name = match type_id {
        0x0041004B => Some("TREModelResource".to_string()),
        0x00464621 => Some("TREPlacementObject".to_string()),
        _ => None,
    };

    let mut group_tag = None;
    let mut entity_name = None;
    let mut scale = None;
    let mut bounding_box = None;
    let mut default_animation = None;
    let mut physics_state = None;
    let mut mesh_bindings = Vec::new();
    let mut bones = Vec::new();
    let mut ragdoll_bone_groups = Vec::new();
    let mut attachments = Vec::new();
    let mut has_sentinel_terminator = false;
    let mut raw_fallbacks = Vec::new();

    // Placement object (0x00464621) specific properties
    let mut stand_model = None;
    let mut placed_object = None;
    let mut placement_offset = None;
    let mut pl_material_id = 2u32;
    let mut pl_is_enabled = true;
    let mut pl_casts_shadows = true;
    let mut pl_can_be_carried = true;
    let mut pl_flags_hex = None;

    for (id, chunk) in &elements {
        match *id {
            20 => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    group_tag = Some(s);
                } else {
                    raw_fallbacks.push(RawFallbackComponentJson {
                        id: *id,
                        reason: "Corrupted string format".into(),
                        hex: hex::encode_upper(chunk),
                    });
                }
            }
            21 => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    entity_name = Some(s);
                } else {
                    raw_fallbacks.push(RawFallbackComponentJson {
                        id: *id,
                        reason: "Corrupted string format".into(),
                        hex: hex::encode_upper(chunk),
                    });
                }
            }
            22 if is_placement_object && chunk.len() >= 4 => {
                let mask = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
                pl_casts_shadows = (mask & 0x2000_0000) != 0;
                pl_flags_hex = Some(format!("0x{:08X}", mask));
            }
            23 if is_placement_object && !chunk.is_empty() => {
                pl_is_enabled = chunk[0] != 0;
            }
            29 if is_placement_object => {
                pl_can_be_carried = chunk.len() >= 4 && chunk[0] != 0;
            }
            31 if is_placement_object => {
                stand_model = parse_simple_model_binding(chunk);
            }
            32 => {
                if let Some(s) = read_scale_vector(chunk) {
                    scale = Some(s);
                } else {
                    raw_fallbacks.push(RawFallbackComponentJson {
                        id: *id,
                        reason: "Invalid scale vector (expected 12 bytes / 3 floats)".into(),
                        hex: hex::encode_upper(chunk),
                    });
                }
            }
            33 => {
                if !is_empty_container(chunk) {
                    let parsed_bones = parse_full_bones_container(chunk);
                    if !parsed_bones.is_empty() {
                        bones = parsed_bones;
                    } else {
                        raw_fallbacks.push(RawFallbackComponentJson {
                            id: *id,
                            reason: "Non-standard bone container format".into(),
                            hex: hex::encode_upper(chunk),
                        });
                    }
                }
            }
            42 if is_placement_object => {
                placed_object = parse_placed_object(chunk);
            }
            46 if is_placement_object && chunk.len() >= 4 => {
                pl_material_id = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
            }
            300 if is_placement_object && chunk.len() >= 15 => {
                placement_offset = parse_placement_offset(chunk);
            }
            19 => {
                has_sentinel_terminator = true;
            }
            _ => {}
        }
    }

    for (id, chunk) in &elements {
        match *id {
            30 => {
                let bindings = parse_mesh_material_bindings(chunk);
                if !bindings.is_empty() || is_empty_container(chunk) {
                    mesh_bindings = bindings;
                } else {
                    raw_fallbacks.push(RawFallbackComponentJson {
                        id: *id,
                        reason: "Failed to parse mesh-material bindings table".into(),
                        hex: hex::encode_upper(chunk),
                    });
                }
            }
            34 => {
                if let Some(bbox) = parse_bounding_box(chunk) {
                    bounding_box = Some(bbox);
                } else {
                    raw_fallbacks.push(RawFallbackComponentJson {
                        id: *id,
                        reason: "Non-standard bounding box format (expected 60 bytes)".into(),
                        hex: hex::encode_upper(chunk),
                    });
                }
            }
            35 => {
                if !is_empty_container(chunk) {
                    let groups = parse_ragdoll_bone_groups(chunk, &bones);
                    if !groups.is_empty() {
                        ragdoll_bone_groups = groups;
                    } else {
                        raw_fallbacks.push(RawFallbackComponentJson {
                            id: *id,
                            reason: "Failed to decode ragdoll bone groups container".into(),
                            hex: hex::encode_upper(chunk),
                        });
                    }
                }
            }
            36 => {
                if !is_empty_container(chunk) {
                    if let Some(anim) = parse_animation_linkage(chunk) {
                        default_animation = Some(anim);
                    } else {
                        raw_fallbacks.push(RawFallbackComponentJson {
                            id: *id,
                            reason: "Unrecognized animation linkage format".into(),
                            hex: hex::encode_upper(chunk),
                        });
                    }
                }
            }
            37 => {
                if chunk.len() == 16 {
                    physics_state = parse_physics_state(chunk);
                } else if let Some(anim) = parse_animation_linkage(chunk) {
                    default_animation = Some(anim);
                } else if !chunk.is_empty() {
                    raw_fallbacks.push(RawFallbackComponentJson {
                        id: *id,
                        reason: "Non-standard physics/state component".into(),
                        hex: hex::encode_upper(chunk),
                    });
                }
            }
            1 => {
                attachments = parse_attachment_slots(chunk);
            }
            20 | 21 | 32 | 33 | 19 => {}
            // Handled placement object constants
            22 | 23 | 28 | 29 | 31 | 38 | 41 | 42 | 44 | 45 | 46 | 300 | 301
                if is_placement_object => {}
            _ => {
                raw_fallbacks.push(RawFallbackComponentJson {
                    id: *id,
                    reason: "Unmapped / unknown engine component".into(),
                    hex: hex::encode_upper(chunk),
                });
            }
        }
    }

    let metadata = ObjectEngineMetadataJson {
        type_id_hex: format!("{:08X}", type_id),
        class_name,
        raw_fallbacks,
    };

    let placement_config = if is_placement_object {
        Some(PlacementConfigJson {
            physics_material_id: pl_material_id,
            is_enabled: pl_is_enabled,
            casts_shadows: pl_casts_shadows,
            can_be_carried: pl_can_be_carried,
            raw_flags_hex: pl_flags_hex,
        })
    } else {
        None
    };

    let entity_json = ObjectEntityJson {
        _engine_metadata: metadata,
        group_tag,
        entity_name: entity_name.clone(),
        scale,
        bounding_box,
        default_animation,
        physics_state,
        ragdoll_bone_groups,
        mesh_bindings,
        stand_model,
        placed_object,
        placement_offset,
        placement_config,
        bones: bones.clone(),
        attachments,
        has_sentinel_terminator,
    };

    if let Some(dir) = output_dir
        && !bones.is_empty()
    {
        let rig_name = entity_name.unwrap_or_else(|| "Unknown_Rig".to_string());
        if let Ok(glb_bytes) = export_skeleton_from_json(&bones, &rig_name) {
            let glb_path = dir.join(format!("{}_MASTER_RIG.glb", rig_name));
            let _ = std::fs::write(glb_path, glb_bytes);
        }
    }

    serde_json::to_string_pretty(&entity_json).map_err(|e| anyhow::anyhow!(e))
}

pub fn import_object_from_json(json_str: &str) -> Result<Vec<u8>> {
    let parsed: ObjectEntityJson = serde_json::from_str(json_str)?;
    let type_id = u32::from_str_radix(&parsed._engine_metadata.type_id_hex, 16)
        .context("Invalid TypeID hex in Object JSON metadata")?;

    let is_placement_object = type_id == 0x00464621;
    let mut elements = Vec::new();

    let get_fallback = |id: u32| -> Option<Vec<u8>> {
        parsed
            ._engine_metadata
            .raw_fallbacks
            .iter()
            .find(|fb| fb.id == id)
            .and_then(|fb| hex::decode(&fb.hex).ok())
    };

    if let Some(ref tag) = parsed.group_tag {
        elements.push((20, write_length_prefixed_string(tag)));
    } else if let Some(raw) = get_fallback(20) {
        elements.push((20, raw));
    }

    if let Some(ref name) = parsed.entity_name {
        elements.push((21, write_length_prefixed_string(name)));
    } else if let Some(raw) = get_fallback(21) {
        elements.push((21, raw));
    }

    if is_placement_object {
        let pl = parsed.placement_config.as_ref();
        let mat_id = pl.map(|p| p.physics_material_id).unwrap_or(2);
        let enabled = pl.is_none_or(|p| p.is_enabled);
        let carry = pl.is_none_or(|p| p.can_be_carried);
        let shadows = pl.is_none_or(|p| p.casts_shadows);

        let mut mask = if let Some(raw_h) = pl.and_then(|p| p.raw_flags_hex.as_ref()) {
            let clean = raw_h
                .trim()
                .trim_start_matches("0x")
                .trim_start_matches("0X");
            u32::from_str_radix(clean, 16).unwrap_or(0x2440_000C)
        } else {
            0x2440_000C
        };
        if shadows {
            mask |= 0x2000_0000;
        } else {
            mask &= !0x2000_0000;
        }
        elements.push((22, mask.to_le_bytes().to_vec()));

        elements.push((23, vec![if enabled { 1 } else { 0 }]));
        elements.push((28, vec![0u8]));

        if carry {
            elements.push((29, vec![1, 40, 0, 3, 40, 0, 43, 4, 44, 5, 1, 1, 0, 0, 0, 1]));
        } else {
            elements.push((29, vec![0u8]));
        }

        if let Some(ref stand) = parsed.stand_model {
            elements.push((
                31,
                build_simple_model_binding(&stand.object_path, &stand.model_name),
            ));
        } else if let Some(raw) = get_fallback(31) {
            elements.push((31, raw));
        }

        elements.push((38, vec![1u8]));
        elements.push((41, vec![1, 1, 0, 0]));

        if let Some(ref placed) = parsed.placed_object {
            elements.push((42, build_placed_object(placed)));
        } else if let Some(raw) = get_fallback(42) {
            elements.push((42, raw));
        }

        elements.push((44, vec![0u8]));
        elements.push((45, vec![1, 1, 0, 0]));
        elements.push((46, mat_id.to_le_bytes().to_vec()));

        if let Some(offset) = parsed.placement_offset {
            elements.push((300, build_placement_offset(offset)));
        } else if let Some(raw) = get_fallback(300) {
            elements.push((300, raw));
        } else {
            elements.push((300, build_placement_offset([0.0, 0.5, 0.0])));
        }
        elements.push((301, vec![0u8]));
    } else {
        if !parsed.mesh_bindings.is_empty() {
            let chunk_30 = rebuild_mesh_material_bindings(&parsed.mesh_bindings)?;
            elements.push((30, chunk_30));
        } else if let Some(raw) = get_fallback(30) {
            elements.push((30, raw));
        }

        if let Some(s) = parsed.scale {
            elements.push((32, write_scale_vector(s)));
        } else if let Some(raw) = get_fallback(32) {
            elements.push((32, raw));
        }

        if !parsed.bones.is_empty() {
            let chunk_33 = rebuild_full_bones_container(&parsed.bones)?;
            elements.push((33, chunk_33));
        } else if let Some(raw) = get_fallback(33) {
            elements.push((33, raw));
        } else {
            elements.push((33, vec![0u8]));
        }

        if let Some(ref bbox) = parsed.bounding_box {
            elements.push((34, rebuild_bounding_box(bbox)));
        } else if let Some(raw) = get_fallback(34) {
            elements.push((34, raw));
        }

        if !parsed.ragdoll_bone_groups.is_empty() {
            let chunk_35 = rebuild_ragdoll_bone_groups(&parsed.ragdoll_bone_groups)?;
            elements.push((35, chunk_35));
        } else if let Some(raw) = get_fallback(35) {
            elements.push((35, raw));
        }

        if let Some(ref anim) = parsed.default_animation {
            elements.push((36, rebuild_animation_linkage(anim)));
        } else if let Some(raw) = get_fallback(36) {
            elements.push((36, raw));
        } else {
            elements.push((36, vec![0u8]));
        }

        if let Some(ref phys) = parsed.physics_state {
            elements.push((37, rebuild_physics_state(phys)));
        } else if let Some(raw) = get_fallback(37) {
            elements.push((37, raw));
        }
    }

    if parsed.has_sentinel_terminator {
        elements.push((19, vec![0xFF, 0xFF, 0xFF, 0xFF]));
    }

    if !parsed.attachments.is_empty() {
        elements.push((1, rebuild_attachment_slots(&parsed.attachments)));
    } else if let Some(raw) = get_fallback(1) {
        elements.push((1, raw));
    } else {
        elements.push((1, vec![0u8]));
    }

    for fb in &parsed._engine_metadata.raw_fallbacks {
        if ![
            20, 21, 30, 32, 33, 34, 35, 36, 37, 19, 1, 22, 23, 28, 29, 31, 38, 41, 42, 44, 45, 46,
            300, 301,
        ]
        .contains(&fb.id)
            && let Ok(raw) = hex::decode(&fb.hex)
        {
            elements.push((fb.id, raw));
        }
    }

    elements.sort_by_key(|&(id, _)| id);
    Ok(build_typed_container(type_id, &elements))
}

fn parse_simple_model_binding(data: &[u8]) -> Option<ObjectModelBindingJson> {
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

fn build_simple_model_binding(obj_path: &str, model_name: &str) -> Vec<u8> {
    let elements = vec![
        (20, write_length_prefixed_string(obj_path)),
        (21, write_length_prefixed_string(model_name)),
    ];
    build_chunk_from_elements(false, &elements)
}

fn parse_placed_object(chunk: &[u8]) -> Option<ObjectModelBindingJson> {
    if let Ok((_, elements)) = parse_chunk_elements(chunk) {
        for (id, data) in elements {
            if id == 30 {
                return parse_simple_model_binding(&data);
            }
        }
    }
    parse_simple_model_binding(chunk)
}

fn build_placed_object(binding: &ObjectModelBindingJson) -> Vec<u8> {
    let model_bytes = build_simple_model_binding(&binding.object_path, &binding.model_name);
    let sub_elements = vec![(30, model_bytes), (35, vec![0u8, 0, 0, 0])];
    build_chunk_from_elements(false, &sub_elements)
}

fn parse_placement_offset(chunk: &[u8]) -> Option<[f32; 3]> {
    if chunk.len() >= 15 && chunk[0] == 1 && chunk[1] == 20 {
        let mut cur = Cursor::new(&chunk[3..15]);
        let x = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let y = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let z = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        if x.is_finite() && y.is_finite() && z.is_finite() {
            return Some([x, y, z]);
        }
    }
    None
}

fn build_placement_offset(offset: [f32; 3]) -> Vec<u8> {
    let mut out = vec![1u8, 20, 0];
    let _ = out.write_f32::<LittleEndian>(offset[0]);
    let _ = out.write_f32::<LittleEndian>(offset[1]);
    let _ = out.write_f32::<LittleEndian>(offset[2]);
    out
}

fn parse_full_bones_container(chunk: &[u8]) -> Vec<FullObjectBoneJson> {
    let mut out = Vec::new();
    let records_blob = if let Ok((_, sub_elems)) = parse_chunk_elements(chunk) {
        sub_elems
            .into_iter()
            .find(|(id, _)| *id == 22)
            .map(|(_, d)| d)
            .unwrap_or_default()
    } else {
        chunk.to_vec()
    };

    let count = records_blob.len() / 144;
    for i in 0..count {
        let b_chunk = &records_blob[i * 144..(i + 1) * 144];
        let raw: RawObjectBone = bytemuck::pod_read_unaligned(b_chunk);

        let clean_name = String::from_utf8_lossy(&raw.name)
            .trim_matches(char::from(0))
            .trim()
            .to_string();

        let bone = ObjectBone {
            name: clean_name,
            matrix: raw.matrix,
            rotation: Vector4 {
                x: raw.rotation[0],
                y: raw.rotation[1],
                z: raw.rotation[2],
                w: raw.rotation[3],
            },
            translation: Vector3 {
                x: raw.translation[0],
                y: raw.translation[1],
                z: raw.translation[2],
            },
            bone_id: raw.bone_id,
            parent_index: raw.parent_index,
            first_child_index: raw.first_child_index,
            next_sibling_index: raw.next_sibling_index,
            aux_id: raw.aux_id,
        };

        out.push(FullObjectBoneJson::from_object_bone(&bone, i));
    }
    out
}

fn rebuild_full_bones_container(bones: &[FullObjectBoneJson]) -> Result<Vec<u8>> {
    let mut raw_records = Vec::with_capacity(bones.len() * 144);

    for b in bones {
        let bone = b.to_object_bone();
        raw_records.extend_from_slice(&bone.to_raw_bytes());
    }

    let sub_elements = vec![
        (20, vec![0u8, 0, 0, 0]),
        (21, (bones.len() as u32).to_le_bytes().to_vec()),
        (22, raw_records),
    ];
    Ok(build_chunk_from_elements(false, &sub_elements))
}

fn parse_physics_state(chunk: &[u8]) -> Option<EntityPhysicsStateJson> {
    if chunk.len() < 16 {
        return None;
    }
    let mut cur = Cursor::new(chunk);
    let ox = cur.read_f32::<LittleEndian>().ok()?;
    let oy = cur.read_f32::<LittleEndian>().ok()?;
    let oz = cur.read_f32::<LittleEndian>().ok()?;
    let w_bytes = cur.read_u32::<LittleEndian>().ok()?;

    let w_f32 = f32::from_bits(w_bytes);
    let is_clean_float = w_f32.is_finite() && w_f32.abs() >= 1e-4 && w_f32.abs() <= 100_000.0;

    let p0 = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
    let p1 = u32::from_le_bytes(chunk[4..8].try_into().unwrap_or_default());
    let p2 = u32::from_le_bytes(chunk[8..12].try_into().unwrap_or_default());
    let is_subnormal = (p0 != 0 && ox.is_subnormal()) || (p1 != 0 && oy.is_subnormal());

    let (collision_radius, logic_flags) = if is_clean_float {
        (Some(w_f32), None)
    } else {
        let flags = PhysicsFlagsJson {
            is_equippable: (w_bytes & 0x0002) != 0,
            cast_shadows: (w_bytes & 0x0008) != 0,
            spatial_query_enabled: (w_bytes & 0x0010) != 0,
            is_dynamic_actor: (w_bytes & 0x0100) != 0,
            unmapped_bits_hex: format!("0x{:08X}", w_bytes & !(0x0002 | 0x0008 | 0x0010 | 0x0100)),
        };
        (None, Some(flags))
    };

    Some(EntityPhysicsStateJson {
        offset: [ox, oy, oz],
        collision_radius,
        logic_flags,
        raw_w_hex: format!("0x{:08X}", w_bytes),
        raw_parameters: if is_subnormal {
            Some([p0, p1, p2, w_bytes])
        } else {
            None
        },
    })
}

fn rebuild_physics_state(state: &EntityPhysicsStateJson) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    let mut cur = Cursor::new(&mut out);

    if let Some(params) = state.raw_parameters {
        for val in params {
            let _ = cur.write_u32::<LittleEndian>(val);
        }
        return out;
    }

    let _ = cur.write_f32::<LittleEndian>(state.offset[0]);
    let _ = cur.write_f32::<LittleEndian>(state.offset[1]);
    let _ = cur.write_f32::<LittleEndian>(state.offset[2]);

    let w_u32 = if let Some(ref flags) = state.logic_flags {
        let mut val = 0u32;
        if flags.is_equippable {
            val |= 0x0002;
        }
        if flags.cast_shadows {
            val |= 0x0008;
        }
        if flags.spatial_query_enabled {
            val |= 0x0010;
        }
        if flags.is_dynamic_actor {
            val |= 0x0100;
        }
        if let Ok(unmapped) =
            u32::from_str_radix(flags.unmapped_bits_hex.trim_start_matches("0x"), 16)
        {
            val |= unmapped;
        }
        val
    } else if let Some(r) = state.collision_radius {
        r.to_bits()
    } else {
        u32::from_str_radix(state.raw_w_hex.trim_start_matches("0x"), 16).unwrap_or(0)
    };

    let _ = cur.write_u32::<LittleEndian>(w_u32);
    out
}

fn parse_bounding_box(data: &[u8]) -> Option<BoundingBoxJson> {
    if data.len() < 60 {
        return None;
    }
    let mut cur = Cursor::new(data);
    let mut matrix = [[0.0f32; 3]; 3];
    for row in &mut matrix {
        for val in row {
            *val = cur.read_f32::<LittleEndian>().ok()?;
        }
    }
    let mut half_extents = [0.0f32; 3];
    for val in &mut half_extents {
        *val = cur.read_f32::<LittleEndian>().ok()?;
    }
    let mut center = [0.0f32; 3];
    for val in &mut center {
        *val = cur.read_f32::<LittleEndian>().ok()?;
    }
    Some(BoundingBoxJson {
        center,
        half_extents,
        orientation_matrix: Some(matrix),
    })
}

fn rebuild_bounding_box(bbox: &BoundingBoxJson) -> Vec<u8> {
    let mut out = Vec::with_capacity(60);
    let mut cur = Cursor::new(&mut out);
    let matrix =
        bbox.orientation_matrix
            .unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    for row in matrix {
        for val in row {
            let _ = cur.write_f32::<LittleEndian>(val);
        }
    }
    for val in bbox.half_extents {
        let _ = cur.write_f32::<LittleEndian>(val);
    }
    for val in bbox.center {
        let _ = cur.write_f32::<LittleEndian>(val);
    }
    out
}

fn parse_ragdoll_bone_groups(
    chunk_data: &[u8],
    bones: &[FullObjectBoneJson],
) -> Vec<BoneGroupJson> {
    let mut groups = Vec::new();
    let elements = if let Ok((_, elems)) = parse_chunk_elements(chunk_data) {
        elems
    } else {
        return groups;
    };

    for (group_idx, elem_data) in elements {
        if elem_data.starts_with(b"\xA0\x00\x41\x00") {
            let mut bone_ids = Vec::new();

            if elem_data.len() > 5
                && let Ok((_, sub_parts)) = parse_typed_container(&elem_data)
            {
                for (part_id, part_data) in sub_parts {
                    if part_id == 23 {
                        let mut cur = Cursor::new(&part_data);
                        while (cur.position() as usize) + 4 <= part_data.len() {
                            if let Ok(b_id) = cur.read_u32::<LittleEndian>() {
                                bone_ids.push(b_id);
                            }
                        }
                    }
                }
            }

            let bone_names: Vec<String> = bone_ids
                .iter()
                .filter_map(|&id| bones.get(id as usize).map(|b| b.name.clone()))
                .collect();

            let name = match group_idx {
                0 => Some("Upper_Torso_Arms".into()),
                1 => Some("Pelvis_Legs".into()),
                2 => Some("Head_Neck".into()),
                3 => Some("Right_Arm_Impact".into()),
                4 => Some("Left_Arm_Impact".into()),
                _ => Some(format!("Physics_Group_{}", group_idx)),
            };

            groups.push(BoneGroupJson {
                group_id: group_idx,
                name,
                bone_ids,
                bone_names,
            });
        }
    }
    groups
}

fn rebuild_ragdoll_bone_groups(groups: &[BoneGroupJson]) -> Result<Vec<u8>> {
    let mut group_chunks = Vec::new();

    for g in groups {
        let group_blob = if g.bone_ids.is_empty() {
            let mut b = Vec::with_capacity(5);
            b.extend_from_slice(&0x004100A0u32.to_le_bytes());
            b.push(0);
            b
        } else {
            let count_bytes = (g.bone_ids.len() as u32).to_le_bytes().to_vec();
            let mut ids_bytes = Vec::with_capacity(g.bone_ids.len() * 4);
            let mut cur = Cursor::new(&mut ids_bytes);
            for &b_id in &g.bone_ids {
                let _ = cur.write_u32::<LittleEndian>(b_id);
            }

            let sub_elements = vec![(22, count_bytes), (23, ids_bytes)];
            build_typed_container(0x004100A0, &sub_elements)
        };
        group_chunks.push((g.group_id, group_blob));
    }

    Ok(build_chunk_from_elements(true, &group_chunks))
}

fn parse_mesh_material_bindings(chunk_data: &[u8]) -> Vec<MeshMaterialBindingJson> {
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

fn rebuild_mesh_material_bindings(bindings: &[MeshMaterialBindingJson]) -> Result<Vec<u8>> {
    let mut binding_chunks = Vec::new();

    for (idx, b) in bindings.iter().enumerate() {
        let mesh_elements = vec![
            (20, write_length_prefixed_string(&b.mesh_path)),
            (21, write_length_prefixed_string(&b.mesh_part_name)),
        ];
        let mesh_container = build_chunk_from_elements(false, &mesh_elements);

        let mat_elements = vec![
            (20, write_length_prefixed_string(&b.material_path)),
            (21, write_length_prefixed_string(&b.material_name)),
        ];
        let mat_container = build_chunk_from_elements(false, &mat_elements);

        let binding_sub = vec![(31, mesh_container), (33, mat_container)];
        let binding_chunk = build_typed_container(0x00410067, &binding_sub);
        binding_chunks.push((idx as u32, binding_chunk));
    }

    Ok(build_chunk_from_elements(true, &binding_chunks))
}

fn parse_animation_linkage(chunk_data: &[u8]) -> Option<DefaultAnimationLinkJson> {
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

fn rebuild_animation_linkage(anim: &DefaultAnimationLinkJson) -> Vec<u8> {
    let mut elements = vec![
        (20, write_length_prefixed_string(&anim.anim_group)),
        (21, write_length_prefixed_string(&anim.clip_name)),
    ];
    if let Some(ref track) = anim.track_name {
        elements.push((22, write_length_prefixed_string(track)));
    }
    build_chunk_from_elements(false, &elements)
}

fn parse_attachment_slots(chunk: &[u8]) -> Vec<AttachmentSlotJson> {
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

fn rebuild_attachment_slots(slots: &[AttachmentSlotJson]) -> Vec<u8> {
    if slots.is_empty() {
        return vec![0u8];
    }
    let mut elements = Vec::new();
    for s in slots {
        if let Ok(b) = hex::decode(&s.data_hex) {
            elements.push((s.slot_id, b));
        }
    }
    build_chunk_from_elements(false, &elements)
}

fn read_scale_vector(data: &[u8]) -> Option<[f32; 3]> {
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

fn write_scale_vector(s: [f32; 3]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12);
    let mut cur = Cursor::new(&mut out);
    let _ = cur.write_f32::<LittleEndian>(s[0]);
    let _ = cur.write_f32::<LittleEndian>(s[1]);
    let _ = cur.write_f32::<LittleEndian>(s[2]);
    out
}

fn export_skeleton_from_json(bones: &[FullObjectBoneJson], rig_name: &str) -> Result<Vec<u8>> {
    let object_bones: Vec<ObjectBone> = bones.iter().map(|b| b.to_object_bone()).collect();
    super::animation::export_skeleton_to_glb(&object_bones, rig_name)
}
