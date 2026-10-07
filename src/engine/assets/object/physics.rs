use anyhow::Result;
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use crate::engine::assets::animation::{ObjectBone, RawObjectBone};
use crate::engine::assets::{
    build_chunk_from_elements_with_endian, build_typed_container_with_endian, parse_chunk_elements,
    parse_typed_container,
};
use crate::engine::common::Endian;
use crate::engine::math::{Vector3, Vector4};

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

pub fn parse_bounding_box(data: &[u8]) -> Option<BoundingBoxJson> {
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

pub fn rebuild_bounding_box(bbox: &BoundingBoxJson, endian: Endian) -> Vec<u8> {
    let mut out = Vec::with_capacity(60);
    let matrix =
        bbox.orientation_matrix
            .unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    for row in matrix {
        for val in row {
            let _ = endian.write_f32(&mut out, val);
        }
    }
    for val in bbox.half_extents {
        let _ = endian.write_f32(&mut out, val);
    }
    for val in bbox.center {
        let _ = endian.write_f32(&mut out, val);
    }
    out
}

pub fn parse_physics_state(chunk: &[u8]) -> Option<EntityPhysicsStateJson> {
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

pub fn rebuild_physics_state(state: &EntityPhysicsStateJson, endian: Endian) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);

    if let Some(params) = state.raw_parameters {
        for val in params {
            let _ = endian.write_u32(&mut out, val);
        }
        return out;
    }

    let _ = endian.write_f32(&mut out, state.offset[0]);
    let _ = endian.write_f32(&mut out, state.offset[1]);
    let _ = endian.write_f32(&mut out, state.offset[2]);

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

    let _ = endian.write_u32(&mut out, w_u32);
    out
}

pub fn parse_ragdoll_bone_groups(
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

pub fn rebuild_ragdoll_bone_groups(groups: &[BoneGroupJson], endian: Endian) -> Result<Vec<u8>> {
    let mut group_chunks = Vec::new();

    for g in groups {
        let group_blob = if g.bone_ids.is_empty() {
            let mut b = Vec::with_capacity(5);
            b.extend_from_slice(&endian.u32_to_bytes(0x004100A0));
            b.push(0);
            b
        } else {
            let count_bytes = endian.u32_to_bytes(g.bone_ids.len() as u32).to_vec();
            let mut ids_bytes = Vec::with_capacity(g.bone_ids.len() * 4);
            for &b_id in &g.bone_ids {
                let _ = endian.write_u32(&mut ids_bytes, b_id);
            }

            let sub_elements = vec![(22, count_bytes), (23, ids_bytes)];
            build_typed_container_with_endian(0x004100A0, &sub_elements, endian)
        };
        group_chunks.push((g.group_id, group_blob));
    }

    Ok(build_chunk_from_elements_with_endian(
        true,
        &group_chunks,
        endian,
    ))
}

pub fn parse_full_bones_container(chunk: &[u8]) -> Vec<FullObjectBoneJson> {
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

pub fn rebuild_full_bones_container(
    bones: &[FullObjectBoneJson],
    endian: Endian,
) -> Result<Vec<u8>> {
    let mut raw_records = Vec::with_capacity(bones.len() * 144);

    for b in bones {
        let bone = b.to_object_bone();
        let raw = bone.to_raw_bytes();
        raw_records.extend_from_slice(&raw);
    }

    let sub_elements = vec![
        (20, vec![0u8, 0, 0, 0]),
        (21, endian.u32_to_bytes(bones.len() as u32).to_vec()),
        (22, raw_records),
    ];
    Ok(build_chunk_from_elements_with_endian(
        false,
        &sub_elements,
        endian,
    ))
}

pub fn export_skeleton_from_json(bones: &[FullObjectBoneJson], rig_name: &str) -> Result<Vec<u8>> {
    let object_bones: Vec<ObjectBone> = bones.iter().map(|b| b.to_object_bone()).collect();
    crate::engine::assets::animation::export_skeleton_to_glb(&object_bones, rig_name)
}
