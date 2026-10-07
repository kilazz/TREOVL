use anyhow::Result;
use bytemuck::{Pod, Zeroable};

use crate::engine::assets::{parse_chunk_elements, parse_typed_container};
use crate::engine::math::{Vector3, Vector4};

/// Zero-copy C-representation for an Overlord bone (exactly 144 bytes).
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct RawObjectBone {
    pub name: [u8; 32],
    pub matrix: [f32; 16],       // Local transform matrix
    pub rotation: [f32; 4],      // Local rotation quaternion (x, y, z, w)
    pub translation: [f32; 3],   // Local translation vector (x, y, z)
    pub bone_id: i32,            // Bone ID / skin index (offset 124)
    pub parent_index: i32,       // True parent index (-1 for Root) (offset 128)
    pub next_sibling_index: i32, // Pointer to next sibling (offset 132)
    pub first_child_index: i32,  // Pointer to first child (offset 136)
    pub aux_id: i32,             // Auxiliary flags (offset 140)
}

const _: () = assert!(std::mem::size_of::<RawObjectBone>() == 144);

#[derive(Debug, Clone)]
pub struct ObjectBone {
    pub name: String,
    pub matrix: [f32; 16],
    pub rotation: Vector4,
    pub translation: Vector3,
    pub bone_id: i32,
    pub parent_index: i32,
    pub next_sibling_index: i32,
    pub first_child_index: i32,
    pub aux_id: i32,
}

impl ObjectBone {
    pub fn to_raw_bytes(&self) -> [u8; 144] {
        let mut name_buf = [0u8; 32];
        let bytes = self.name.as_bytes();
        let copy_len = bytes.len().min(31);
        name_buf[..copy_len].copy_from_slice(&bytes[..copy_len]);

        let raw = RawObjectBone {
            name: name_buf,
            matrix: self.matrix,
            rotation: [
                self.rotation.x,
                self.rotation.y,
                self.rotation.z,
                self.rotation.w,
            ],
            translation: [self.translation.x, self.translation.y, self.translation.z],
            bone_id: self.bone_id,
            parent_index: self.parent_index,
            next_sibling_index: self.next_sibling_index,
            first_child_index: self.first_child_index,
            aux_id: self.aux_id,
        };
        bytemuck::cast::<RawObjectBone, [u8; 144]>(raw)
    }
}

pub(crate) fn try_parse_bones(slice: &[u8], min_count: usize) -> Option<Vec<ObjectBone>> {
    if slice.len() < 144 || !slice.len().is_multiple_of(144) {
        return None;
    }

    let count = slice.len() / 144;
    if count < min_count {
        return None;
    }

    let mut bones = Vec::with_capacity(count);

    for chunk in slice.as_chunks::<144>().0 {
        let raw: RawObjectBone = bytemuck::pod_read_unaligned(chunk);

        let clean_name = String::from_utf8_lossy(&raw.name)
            .chars()
            .filter(|c| c.is_ascii_graphic() || *c == ' ' || *c == '_')
            .collect::<String>();

        let is_valid_name = !clean_name.is_empty()
            && clean_name.len() >= 2
            && !clean_name.starts_with('[')
            && clean_name.is_ascii();

        let is_valid_transform = raw.translation[0].is_finite()
            && raw.translation[1].is_finite()
            && raw.translation[2].is_finite()
            && raw.translation[0].abs() < 50_000.0
            && raw.rotation[3].is_finite()
            && raw.rotation[3].abs() <= 2.0;

        let is_valid_hierarchy = raw.parent_index >= -1 && raw.parent_index < count as i32;

        if is_valid_name && is_valid_transform && is_valid_hierarchy {
            bones.push(ObjectBone {
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
                next_sibling_index: raw.next_sibling_index,
                first_child_index: raw.first_child_index,
                aux_id: raw.aux_id,
            });
        } else {
            return None;
        }
    }

    if bones.len() >= min_count {
        Some(bones)
    } else {
        None
    }
}

pub fn parse_object_bone_container(data: &[u8]) -> Result<Vec<ObjectBone>> {
    if let Ok((_type_id, elements)) = parse_typed_container(data) {
        for (id, chunk) in &elements {
            if *id == 33 {
                if let Ok((_, sub_elems)) = parse_chunk_elements(chunk)
                    && let Some((_, blob)) = sub_elems.iter().find(|(sid, _)| *sid == 22)
                    && let Some(bones) = try_parse_bones(blob, 1)
                {
                    return Ok(bones);
                }
                if let Some(bones) = try_parse_bones(chunk, 1) {
                    return Ok(bones);
                }
            } else if *id == 1
                && let Ok((_, sub_elems)) = parse_chunk_elements(chunk)
            {
                for (sid, schunk) in &sub_elems {
                    if (*sid == 13 || *sid == 33) && schunk.len() >= 144 {
                        if let Ok((_, inner_elems)) = parse_chunk_elements(schunk)
                            && let Some((_, blob)) =
                                inner_elems.iter().find(|(isid, _)| *isid == 22)
                            && let Some(bones) = try_parse_bones(blob, 1)
                        {
                            return Ok(bones);
                        }
                        if let Some(bones) = try_parse_bones(schunk, 1) {
                            return Ok(bones);
                        }
                    }
                }
            }
        }
    } else if let Ok((_, elements)) = parse_chunk_elements(data) {
        for (id, chunk) in &elements {
            if *id == 22
                && let Some(bones) = try_parse_bones(chunk, 1)
            {
                return Ok(bones);
            }
        }
    }

    if let Some(bones) = try_parse_bones(data, 2) {
        return Ok(bones);
    }

    Ok(Vec::new())
}
