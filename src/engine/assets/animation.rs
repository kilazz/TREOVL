use super::{parse_chunk_elements, parse_typed_container};
use crate::engine::common::{magic, read_length_prefixed_string};
use crate::engine::math::{BoneRotation, Vector3, Vector4};
use crate::utils::gltf_builder::GltfBuilder;
use crate::utils::renderer::GridVertex;
use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Cursor;

#[inline]
fn sanitize_f32(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v } else { fallback }
}

/// Basis change quaternion from Triumph Engine (Z-Up) to glTF 2.0 / Blender (Y-Up):
/// -90 degrees rotation around the X-axis.
#[inline]
fn gltf_basis_quat() -> Quat {
    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
}

/// Zero-copy C-representation for an Overlord bone (exactly 144 bytes).
/// Layout strictly synchronized with official Triumph Engine / RPK format.
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct RawObjectBone {
    pub name: [u8; 32],
    pub matrix: [f32; 16],       // Local transform matrix relative to parent
    pub rotation: [f32; 4],      // Local rotation quaternion
    pub translation: [f32; 3],   // Local translation vector
    pub bone_id: i32,            // Bone ID / skin index (offset 124)
    pub parent_index: i32,       // True parent bone index (-1 for Root) (offset 128)
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

fn try_parse_bones(slice: &[u8], min_count: usize) -> Option<Vec<ObjectBone>> {
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

pub fn export_skeleton_to_glb(bones: &[ObjectBone], rig_name: &str) -> Result<Vec<u8>> {
    if bones.is_empty() {
        anyhow::bail!("No valid bones found to export skeleton");
    }

    let mut builder = GltfBuilder::new();
    let num_bones = bones.len();

    let mut armature_children = Vec::new();
    let mut bone_nodes = Vec::new();

    let q_basis = gltf_basis_quat();
    let q_basis_inv = q_basis.inverse();

    for (i, bone) in bones.iter().enumerate() {
        let bone_node_id = i + 1;

        let mut children = Vec::new();
        for (j, b2) in bones.iter().enumerate() {
            if b2.parent_index == i as i32 && j != i {
                children.push(j + 1);
            }
        }

        let is_root = bone.parent_index < 0 || bone.parent_index >= num_bones as i32;
        if is_root {
            armature_children.push(bone_node_id);
        }

        let n = if bone.name.is_empty() {
            format!("Bone_{:03}", i)
        } else {
            bone.name.clone()
        };

        let (tx, ty, tz) = if is_root {
            (
                sanitize_f32(bone.translation.x, 0.0),
                sanitize_f32(-bone.translation.z, 0.0),
                sanitize_f32(bone.translation.y, 0.0),
            )
        } else {
            (
                sanitize_f32(bone.translation.x, 0.0),
                sanitize_f32(bone.translation.y, 0.0),
                sanitize_f32(bone.translation.z, 0.0),
            )
        };

        let raw_q = Quat::from_xyzw(
            sanitize_f32(bone.rotation.x, 0.0),
            sanitize_f32(bone.rotation.y, 0.0),
            sanitize_f32(bone.rotation.z, 0.0),
            sanitize_f32(bone.rotation.w, 1.0),
        )
        .normalize();

        let conv_q = if is_root {
            (q_basis * raw_q * q_basis_inv).normalize()
        } else {
            raw_q
        };

        let mut node = json!({
            "name": n,
            "translation": [tx, ty, tz],
            "rotation": [conv_q.x, conv_q.y, conv_q.z, conv_q.w]
        });

        if !children.is_empty() {
            node["children"] = json!(children);
        }
        bone_nodes.push(node);
    }

    if armature_children.is_empty() {
        armature_children.push(1);
    }

    builder.add_node(json!({
        "name": format!("{}_Armature", rig_name),
        "children": armature_children
    }));

    for node in bone_nodes {
        builder.add_node(node);
    }

    let joint_indices: Vec<usize> = (1..=num_bones).collect();
    builder.add_skin(json!({
        "name": rig_name,
        "joints": joint_indices
    }));

    builder.add_scene(vec![0]);
    builder.build(rig_name)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyframeTranslation {
    pub time_seconds: f32,
    pub position: Vector3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyframeRotation {
    pub time_seconds: f32,
    pub rotation_euler: BoneRotation,
    pub rotation_quat: Vector4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoneTrack {
    pub bone_name: String,
    pub translations: Vec<KeyframeTranslation>,
    pub rotations: Vec<KeyframeRotation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationClip {
    pub name: String,
    pub target_rig: String,
    pub frame_rate: f32,
    pub duration_seconds: f32,
    pub bone_tracks: Vec<BoneTrack>,
}

pub fn parse_animation_clip(chunk_data: &[u8]) -> Result<AnimationClip> {
    let payload = if chunk_data.len() > 4 && &chunk_data[0..4] == magic::ANIM_CLIP {
        &chunk_data[4..]
    } else {
        chunk_data
    };

    let (_, elements) = parse_chunk_elements(payload)?;

    let mut name = String::from("Unnamed_Animation");
    let mut target_rig = String::from("Generic_Rig");
    let mut frame_rate = 15.0f32;
    let mut duration_seconds = 1.0f32;
    let mut bone_tracks = Vec::new();

    for (id, chunk) in &elements {
        match *id {
            20 => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    target_rig = s;
                }
            }
            21 => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    name = s;
                }
            }
            30 if chunk.len() >= 4 => {
                let fps = Cursor::new(chunk)
                    .read_f32::<LittleEndian>()
                    .unwrap_or(15.0);
                if fps > 0.0 && fps < 240.0 {
                    frame_rate = fps;
                }
            }
            31 if chunk.len() >= 8 => {
                let duration_micros = Cursor::new(chunk)
                    .read_u64::<LittleEndian>()
                    .unwrap_or(1_000_000);
                duration_seconds = duration_micros as f32 / 1_000_000.0;
            }
            1 => {
                if let Ok((_, data_sub)) = parse_chunk_elements(chunk) {
                    for (sub_id, list_container) in data_sub {
                        if (sub_id == 10 || sub_id == 1)
                            && let Ok((_, bone_entries)) = parse_chunk_elements(&list_container)
                        {
                            for (_, bone_chunk) in bone_entries {
                                if let Ok(track) =
                                    parse_single_bone_track(&bone_chunk, duration_seconds)
                                {
                                    bone_tracks.push(track);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(AnimationClip {
        name,
        target_rig,
        frame_rate,
        duration_seconds,
        bone_tracks,
    })
}

fn parse_single_bone_track(data: &[u8], total_duration: f32) -> Result<BoneTrack> {
    let payload = if data.len() > 4 && &data[0..4] == magic::ANIM_TRACK {
        &data[4..]
    } else {
        data
    };

    let (_, elements) = parse_chunk_elements(payload)?;
    let mut bone_name = String::from("Unnamed_Bone");
    let mut translations = Vec::new();
    let mut rotations = Vec::new();

    for (id, chunk) in elements {
        match id {
            20 => {
                if let Some(s) = read_length_prefixed_string(&chunk) {
                    bone_name = s;
                }
            }
            22 | 24 => {
                if let Ok((_, trans_sub)) = parse_chunk_elements(&chunk) {
                    for (tid, tchunk) in trans_sub {
                        if tid == 22 || tid == 21 || tid == 0 {
                            parse_translation_blob(&tchunk, total_duration, &mut translations);
                        }
                    }
                } else {
                    parse_translation_blob(&chunk, total_duration, &mut translations);
                }
            }
            23 | 25 => {
                parse_rotation_container(&chunk, total_duration, &mut rotations);
            }
            _ => {}
        }
    }

    Ok(BoneTrack {
        bone_name,
        translations,
        rotations,
    })
}

fn parse_rotation_container(
    chunk: &[u8],
    total_duration: f32,
    rotations: &mut Vec<KeyframeRotation>,
) {
    if let Ok((_, rot_sub)) = parse_chunk_elements(chunk) {
        for (rid, rchunk) in rot_sub {
            if rid == 21 {
                if let Ok((_, sub_parts)) = parse_chunk_elements(&rchunk) {
                    let count = sub_parts
                        .iter()
                        .find(|(id, _)| *id == 22)
                        .and_then(|(_, d)| Cursor::new(d).read_u32::<LittleEndian>().ok())
                        .unwrap_or(0) as usize;

                    let keys_data = sub_parts
                        .iter()
                        .find(|(id, _)| *id == 23)
                        .map(|(_, d)| d.as_slice());
                    let signs_data = sub_parts
                        .iter()
                        .find(|(id, _)| *id == 24)
                        .map(|(_, d)| d.as_slice());

                    if let Some(data_blob) = keys_data {
                        parse_rotation_stream(
                            data_blob,
                            signs_data,
                            count,
                            total_duration,
                            rotations,
                        );
                        return;
                    }
                }
                parse_rotation_blob(&rchunk, total_duration, rotations);
            } else if rid == 22 || rid == 23 || rid == 0 {
                if let Ok((_, data_sub)) = parse_chunk_elements(&rchunk) {
                    for (did, dchunk) in data_sub {
                        if did == 23 || did == 22 || did == 0 {
                            parse_rotation_blob(&dchunk, total_duration, rotations);
                        }
                    }
                } else {
                    parse_rotation_blob(&rchunk, total_duration, rotations);
                }
            }
        }
    } else {
        parse_rotation_blob(chunk, total_duration, rotations);
    }
}

fn parse_rotation_stream(
    data: &[u8],
    signs: Option<&[u8]>,
    explicit_count: usize,
    total_duration: f32,
    rotations: &mut Vec<KeyframeRotation>,
) {
    let count = if explicit_count > 0 {
        explicit_count
    } else {
        data.len() / 6
    };

    if count == 0 {
        return;
    }

    if data.len() >= count * 6 {
        let mut previous_q: Option<Quat> = None;

        for i in 0..count {
            let chunk = &data[i * 6..(i + 1) * 6];
            let raw: [i16; 3] = bytemuck::pod_read_unaligned(chunk);

            let x = raw[0] as f32 / 32767.0;
            let y = raw[1] as f32 / 32767.0;
            let z = raw[2] as f32 / 32767.0;

            let w_sq = 1.0 - (x * x + y * y + z * z);
            let mut w = if w_sq > 0.0 { w_sq.sqrt() } else { 0.0 };

            if let Some(sign_array) = signs {
                let byte_idx = i >> 3;
                let bit_idx = i & 7;
                if byte_idx < sign_array.len() && ((sign_array[byte_idx] >> bit_idx) & 1) != 0 {
                    w = -w;
                }
            }

            let mut q = Quat::from_xyzw(x, y, z, w).normalize();

            if let Some(prev) = previous_q
                && prev.dot(q) < 0.0
            {
                q = -q;
            }
            previous_q = Some(q);

            let time_seconds = if count > 1 {
                (i as f32 * total_duration) / (count - 1) as f32
            } else {
                0.0
            };

            rotations.push(KeyframeRotation {
                time_seconds,
                rotation_euler: BoneRotation::default(),
                rotation_quat: Vector4 {
                    x: q.x,
                    y: q.y,
                    z: q.z,
                    w: q.w,
                },
            });
        }
    }
}

#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct RawTranslationKey {
    micros: u32,
    px: f32,
    py: f32,
    pz: f32,
}

fn parse_translation_blob(
    data: &[u8],
    total_duration: f32,
    translations: &mut Vec<KeyframeTranslation>,
) {
    let chunk_len = data.len();
    if chunk_len < 12 {
        return;
    }

    if chunk_len >= 16 && chunk_len.is_multiple_of(16) {
        let count = chunk_len / 16;
        for i in 0..count {
            let chunk = &data[i * 16..(i + 1) * 16];
            let raw: RawTranslationKey = bytemuck::pod_read_unaligned(chunk);
            let time_seconds = (raw.micros as f32 / 1_000_000.0).min(total_duration);
            translations.push(KeyframeTranslation {
                time_seconds,
                position: Vector3 {
                    x: sanitize_f32(raw.px, 0.0),
                    y: sanitize_f32(raw.py, 0.0),
                    z: sanitize_f32(raw.pz, 0.0),
                },
            });
        }
    } else if chunk_len >= 12 {
        let raw: [f32; 3] = bytemuck::pod_read_unaligned(&data[0..12]);
        translations.push(KeyframeTranslation {
            time_seconds: 0.0,
            position: Vector3 {
                x: sanitize_f32(raw[0], 0.0),
                y: sanitize_f32(raw[1], 0.0),
                z: sanitize_f32(raw[2], 0.0),
            },
        });
    }
}

fn parse_rotation_blob(data: &[u8], total_duration: f32, rotations: &mut Vec<KeyframeRotation>) {
    let chunk_len = data.len();
    if chunk_len < 6 {
        return;
    }

    if chunk_len.is_multiple_of(16) {
        let count = chunk_len / 16;
        for i in 0..count {
            let chunk = &data[i * 16..(i + 1) * 16];
            let q: [f32; 4] = bytemuck::pod_read_unaligned(chunk);
            let time_seconds = if count > 1 {
                i as f32 * total_duration / (count - 1) as f32
            } else {
                0.0
            };
            rotations.push(KeyframeRotation {
                time_seconds,
                rotation_euler: BoneRotation::default(),
                rotation_quat: Vector4 {
                    x: sanitize_f32(q[0], 0.0),
                    y: sanitize_f32(q[1], 0.0),
                    z: sanitize_f32(q[2], 0.0),
                    w: sanitize_f32(q[3], 1.0),
                },
            });
        }
    } else if chunk_len.is_multiple_of(12) {
        let count = chunk_len / 12;
        for i in 0..count {
            let chunk = &data[i * 12..(i + 1) * 12];
            let micros = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
            let q: [i16; 4] = bytemuck::pod_read_unaligned(&chunk[4..12]);
            let time_seconds = (micros as f32 / 1_000_000.0).min(total_duration);
            rotations.push(KeyframeRotation {
                time_seconds,
                rotation_euler: BoneRotation::default(),
                rotation_quat: Vector4 {
                    x: q[0] as f32 / 32767.0,
                    y: q[1] as f32 / 32767.0,
                    z: q[2] as f32 / 32767.0,
                    w: q[3] as f32 / 32767.0,
                },
            });
        }
    } else if chunk_len.is_multiple_of(10) {
        let count = chunk_len / 10;
        for i in 0..count {
            let chunk = &data[i * 10..(i + 1) * 10];
            let micros = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
            let raw: [i16; 3] = bytemuck::pod_read_unaligned(&chunk[4..10]);

            let x = raw[0] as f32 / 32767.0;
            let y = raw[1] as f32 / 32767.0;
            let z = raw[2] as f32 / 32767.0;
            let w_sq = 1.0 - (x * x + y * y + z * z);
            let w = if w_sq > 0.0 { w_sq.sqrt() } else { 0.0 };

            let time_seconds = (micros as f32 / 1_000_000.0).min(total_duration);
            rotations.push(KeyframeRotation {
                time_seconds,
                rotation_euler: BoneRotation::default(),
                rotation_quat: Vector4 { x, y, z, w },
            });
        }
    } else if chunk_len.is_multiple_of(8) {
        parse_rotation_stream(data, None, chunk_len / 8, total_duration, rotations);
    } else if chunk_len.is_multiple_of(6) {
        parse_rotation_stream(data, None, chunk_len / 6, total_duration, rotations);
    }
}

pub fn export_animation_to_json(chunk_data: &[u8]) -> Result<String> {
    let clip = parse_animation_clip(chunk_data)?;
    serde_json::to_string_pretty(&clip).map_err(|e| anyhow::anyhow!(e))
}

pub fn export_animation_to_glb(chunk_data: &[u8]) -> Result<Vec<u8>> {
    let clip = parse_animation_clip(chunk_data)?;
    let mut builder = GltfBuilder::new();

    let mut channels = Vec::new();
    let mut samplers = Vec::new();
    let mut bone_indices = Vec::new();

    let q_basis = gltf_basis_quat();
    let q_basis_inv = q_basis.inverse();

    for (i, track) in clip.bone_tracks.iter().enumerate() {
        let bone_node_id = i + 1;
        bone_indices.push(bone_node_id);

        let is_root = track.bone_name.eq_ignore_ascii_case("root")
            || track.bone_name.eq_ignore_ascii_case("bip01")
            || i == 0;

        if !track.translations.is_empty() {
            let mut time_bytes = Vec::with_capacity(track.translations.len() * 4);
            let mut min_time = f32::INFINITY;
            let mut max_time = f32::NEG_INFINITY;

            for t in &track.translations {
                let ts = sanitize_f32(t.time_seconds, 0.0);
                time_bytes.write_f32::<LittleEndian>(ts)?;
                min_time = min_time.min(ts);
                max_time = max_time.max(ts);
            }
            let time_view = builder.add_buffer_view(&time_bytes, None);
            let time_acc = builder.add_accessor(
                time_view,
                track.translations.len(),
                5126,
                "SCALAR",
                Some(vec![min_time]),
                Some(vec![max_time]),
            );

            let mut val_bytes = Vec::with_capacity(track.translations.len() * 12);
            for t in &track.translations {
                let px = sanitize_f32(t.position.x, 0.0);
                let py = sanitize_f32(t.position.y, 0.0);
                let pz = sanitize_f32(t.position.z, 0.0);

                if is_root {
                    val_bytes.write_f32::<LittleEndian>(px)?;
                    val_bytes.write_f32::<LittleEndian>(-pz)?;
                    val_bytes.write_f32::<LittleEndian>(py)?;
                } else {
                    val_bytes.write_f32::<LittleEndian>(px)?;
                    val_bytes.write_f32::<LittleEndian>(py)?;
                    val_bytes.write_f32::<LittleEndian>(pz)?;
                }
            }
            let val_view = builder.add_buffer_view(&val_bytes, None);
            let val_acc =
                builder.add_accessor(val_view, track.translations.len(), 5126, "VEC3", None, None);

            let sampler_idx = samplers.len();
            samplers.push(json!({
                "input": time_acc,
                "output": val_acc,
                "interpolation": "LINEAR"
            }));

            channels.push(json!({
                "sampler": sampler_idx,
                "target": {
                    "node": bone_node_id,
                    "path": "translation"
                }
            }));
        }

        if !track.rotations.is_empty() {
            let mut time_bytes = Vec::with_capacity(track.rotations.len() * 4);
            let mut min_time = f32::INFINITY;
            let mut max_time = f32::NEG_INFINITY;

            for r in &track.rotations {
                let ts = sanitize_f32(r.time_seconds, 0.0);
                time_bytes.write_f32::<LittleEndian>(ts)?;
                min_time = min_time.min(ts);
                max_time = max_time.max(ts);
            }
            let time_view = builder.add_buffer_view(&time_bytes, None);
            let time_acc = builder.add_accessor(
                time_view,
                track.rotations.len(),
                5126,
                "SCALAR",
                Some(vec![min_time]),
                Some(vec![max_time]),
            );

            let mut val_bytes = Vec::with_capacity(track.rotations.len() * 16);
            for r in &track.rotations {
                let raw_q = Quat::from_xyzw(
                    sanitize_f32(r.rotation_quat.x, 0.0),
                    sanitize_f32(r.rotation_quat.y, 0.0),
                    sanitize_f32(r.rotation_quat.z, 0.0),
                    if r.rotation_quat.w.is_finite() && r.rotation_quat.w != 0.0 {
                        r.rotation_quat.w
                    } else {
                        1.0
                    },
                )
                .normalize();

                let conv_q = if is_root {
                    (q_basis * raw_q * q_basis_inv).normalize()
                } else {
                    raw_q
                };

                val_bytes.write_f32::<LittleEndian>(conv_q.x)?;
                val_bytes.write_f32::<LittleEndian>(conv_q.y)?;
                val_bytes.write_f32::<LittleEndian>(conv_q.z)?;
                val_bytes.write_f32::<LittleEndian>(conv_q.w)?;
            }
            let val_view = builder.add_buffer_view(&val_bytes, None);
            let val_acc =
                builder.add_accessor(val_view, track.rotations.len(), 5126, "VEC4", None, None);

            let sampler_idx = samplers.len();
            samplers.push(json!({
                "input": time_acc,
                "output": val_acc,
                "interpolation": "LINEAR"
            }));

            channels.push(json!({
                "sampler": sampler_idx,
                "target": {
                    "node": bone_node_id,
                    "path": "rotation"
                }
            }));
        }
    }

    builder.add_node(json!({
        "name": format!("{}_Armature", clip.name),
        "children": bone_indices
    }));

    for track in &clip.bone_tracks {
        builder.add_node(json!({ "name": track.bone_name }));
    }

    builder.add_skin(json!({
        "name": format!("{}_Skin", clip.target_rig),
        "joints": bone_indices
    }));

    builder.add_animation(json!({
        "name": clip.name,
        "channels": channels,
        "samplers": samplers
    }));

    builder.add_scene(vec![0]);
    builder.build("Overlord Modding Studio Skeletal Animation Exporter")
}

// -----------------------------------------------------------------------------
// FORWARD KINEMATICS & REAL-TIME SKELETAL SKINNING
// -----------------------------------------------------------------------------

pub fn compute_skinning_matrices(
    bones: &[ObjectBone],
    clip: &AnimationClip,
    time_seconds: f32,
) -> (Vec<Mat4>, Vec<GridVertex>) {
    let num_bones = bones.len();
    if num_bones == 0 {
        return (Vec::new(), Vec::new());
    }

    let mut bind_global = vec![None; num_bones];

    fn calc_bind_global(
        idx: usize,
        bones: &[ObjectBone],
        bind_global: &mut [Option<Mat4>],
        visited: &mut [bool],
    ) -> Mat4 {
        if let Some(m) = bind_global[idx] {
            return m;
        }
        if visited[idx] {
            return Mat4::IDENTITY;
        }
        visited[idx] = true;

        let local_m = Mat4::from_cols_array(&bones[idx].matrix).transpose();
        let p_idx = bones[idx].parent_index;

        let global_m = if p_idx >= 0 && (p_idx as usize) < bones.len() && (p_idx as usize) != idx {
            let parent_m = calc_bind_global(p_idx as usize, bones, bind_global, visited);
            parent_m * local_m
        } else {
            local_m
        };

        bind_global[idx] = Some(global_m);
        global_m
    }

    let mut bind_inv_matrices = Vec::with_capacity(num_bones);
    let mut bind_scales = Vec::with_capacity(num_bones);

    for i in 0..num_bones {
        let mut visited = vec![false; num_bones];
        let bg = calc_bind_global(i, bones, &mut bind_global, &mut visited);
        bind_inv_matrices.push(bg.inverse());

        let local_bind = Mat4::from_cols_array(&bones[i].matrix).transpose();
        let (scale, _, _) = local_bind.to_scale_rotation_translation();
        bind_scales.push(scale);
    }

    let mut local_animated = Vec::with_capacity(num_bones);

    for (i, bone) in bones.iter().enumerate() {
        let track_opt = clip.bone_tracks.iter().find(|t| t.bone_name == bone.name);

        let translation = if let Some(track) = track_opt
            && !track.translations.is_empty()
        {
            sample_translation(&track.translations, time_seconds)
        } else {
            Vec3::new(bone.translation.x, bone.translation.y, bone.translation.z)
        };

        let rotation = if let Some(track) = track_opt
            && !track.rotations.is_empty()
        {
            sample_rotation(&track.rotations, time_seconds)
        } else {
            Quat::from_xyzw(
                bone.rotation.x,
                bone.rotation.y,
                bone.rotation.z,
                bone.rotation.w,
            )
        };

        let scale = bind_scales[i];
        let local_m =
            Mat4::from_scale_rotation_translation(scale, rotation.normalize(), translation);
        local_animated.push(local_m);
    }

    let mut global_animated = vec![None; num_bones];

    fn calc_anim_global(
        idx: usize,
        bones: &[ObjectBone],
        local: &[Mat4],
        global: &mut [Option<Mat4>],
        visited: &mut [bool],
    ) -> Mat4 {
        if let Some(cached) = global[idx] {
            return cached;
        }
        if visited[idx] {
            return local[idx];
        }
        visited[idx] = true;

        let p_idx = bones[idx].parent_index;
        let m = if p_idx >= 0 && (p_idx as usize) < bones.len() && (p_idx as usize) != idx {
            let parent_m = calc_anim_global(p_idx as usize, bones, local, global, visited);
            parent_m * local[idx]
        } else {
            local[idx]
        };

        global[idx] = Some(m);
        m
    }

    for i in 0..num_bones {
        let mut visited = vec![false; num_bones];
        calc_anim_global(
            i,
            bones,
            &local_animated,
            &mut global_animated,
            &mut visited,
        );
    }

    let mut debug_lines = Vec::new();
    let magenta = [1.0, 0.0, 1.0, 1.0];
    let cyan = [0.0, 1.0, 1.0, 1.0];

    for i in 0..num_bones {
        let g_mat = global_animated[i].unwrap_or(Mat4::IDENTITY);
        let cp = g_mat.transform_point3(Vec3::ZERO);
        let child_pos = Vec3::new(cp.x, cp.z, -cp.y);

        let p_idx = bones[i].parent_index;
        if p_idx >= 0 && (p_idx as usize) < num_bones {
            let p_mat = global_animated[p_idx as usize].unwrap_or(Mat4::IDENTITY);
            let pp = p_mat.transform_point3(Vec3::ZERO);
            let parent_pos = Vec3::new(pp.x, pp.z, -pp.y);

            debug_lines.push(GridVertex {
                position: parent_pos.into(),
                color: magenta,
            });
            debug_lines.push(GridVertex {
                position: child_pos.into(),
                color: cyan,
            });
        }
    }

    let mut skin_matrices = Vec::with_capacity(num_bones);
    for i in 0..num_bones {
        let g = global_animated[i].unwrap_or(Mat4::IDENTITY);
        skin_matrices.push(g * bind_inv_matrices[i]);
    }

    (skin_matrices, debug_lines)
}

fn sample_translation(keys: &[KeyframeTranslation], time: f32) -> Vec3 {
    if keys.is_empty() {
        return Vec3::ZERO;
    }
    if keys.len() == 1 || time <= keys[0].time_seconds {
        return Vec3::new(keys[0].position.x, keys[0].position.y, keys[0].position.z);
    }
    let last = keys.last().unwrap();
    if time >= last.time_seconds {
        return Vec3::new(last.position.x, last.position.y, last.position.z);
    }

    for i in 0..keys.len() - 1 {
        let k0 = &keys[i];
        let k1 = &keys[i + 1];
        if time >= k0.time_seconds && time <= k1.time_seconds {
            let dt = k1.time_seconds - k0.time_seconds;
            let factor = if dt > 1e-5 {
                (time - k0.time_seconds) / dt
            } else {
                0.0
            };
            let p0 = Vec3::new(k0.position.x, k0.position.y, k0.position.z);
            let p1 = Vec3::new(k1.position.x, k1.position.y, k1.position.z);
            return p0.lerp(p1, factor);
        }
    }

    Vec3::new(keys[0].position.x, keys[0].position.y, keys[0].position.z)
}

fn sample_rotation(keys: &[KeyframeRotation], time: f32) -> Quat {
    if keys.is_empty() {
        return Quat::IDENTITY;
    }
    if keys.len() == 1 || time <= keys[0].time_seconds {
        let q = &keys[0].rotation_quat;
        return Quat::from_xyzw(q.x, q.y, q.z, q.w).normalize();
    }
    let last = keys.last().unwrap();
    if time >= last.time_seconds {
        let q = &last.rotation_quat;
        return Quat::from_xyzw(q.x, q.y, q.z, q.w).normalize();
    }

    for i in 0..keys.len() - 1 {
        let k0 = &keys[i];
        let k1 = &keys[i + 1];
        if time >= k0.time_seconds && time <= k1.time_seconds {
            let dt = k1.time_seconds - k0.time_seconds;
            let factor = if dt > 1e-5 {
                (time - k0.time_seconds) / dt
            } else {
                0.0
            };
            let q0 = Quat::from_xyzw(
                k0.rotation_quat.x,
                k0.rotation_quat.y,
                k0.rotation_quat.z,
                k0.rotation_quat.w,
            )
            .normalize();
            let q1 = Quat::from_xyzw(
                k1.rotation_quat.x,
                k1.rotation_quat.y,
                k1.rotation_quat.z,
                k1.rotation_quat.w,
            )
            .normalize();
            return q0.slerp(q1, factor).normalize();
        }
    }

    let q = &keys[0].rotation_quat;
    Quat::from_xyzw(q.x, q.y, q.z, q.w).normalize()
}

pub fn apply_skeletal_skinning(
    rest_positions: &[Vector3],
    rest_normals: &[Vector3],
    joints: &[[u16; 4]],
    weights: &[Vector4],
    skin_matrices: &[Mat4],
    out_positions: &mut [Vector3],
    out_normals: &mut [Vector3],
) {
    if skin_matrices.is_empty() || joints.is_empty() || weights.is_empty() {
        out_positions.copy_from_slice(rest_positions);
        out_normals.copy_from_slice(rest_normals);
        return;
    }

    let num_matrices = skin_matrices.len();

    let mut scale_factor: u16 = 1;
    for j in joints {
        if j[0] as usize >= num_matrices
            || j[1] as usize >= num_matrices
            || j[2] as usize >= num_matrices
            || j[3] as usize >= num_matrices
        {
            scale_factor = 3;
            break;
        }
    }

    for i in 0..rest_positions.len() {
        let p = rest_positions[i];
        let n = rest_normals.get(i).copied().unwrap_or(Vector3 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        });
        let j = joints[i];
        let w = weights[i];

        let j0 = (j[0] / scale_factor) as usize;
        let j1 = (j[1] / scale_factor) as usize;
        let j2 = (j[2] / scale_factor) as usize;
        let j3 = (j[3] / scale_factor) as usize;

        let m0 = if j0 < num_matrices {
            skin_matrices[j0]
        } else {
            Mat4::IDENTITY
        };
        let m1 = if j1 < num_matrices {
            skin_matrices[j1]
        } else {
            Mat4::IDENTITY
        };
        let m2 = if j2 < num_matrices {
            skin_matrices[j2]
        } else {
            Mat4::IDENTITY
        };
        let m3 = if j3 < num_matrices {
            skin_matrices[j3]
        } else {
            Mat4::IDENTITY
        };

        let v_pos = glam::Vec4::new(p.x, p.y, p.z, 1.0);
        let v_norm = glam::Vec4::new(n.x, n.y, n.z, 0.0);

        let skinned_p =
            (m0 * v_pos) * w.x + (m1 * v_pos) * w.y + (m2 * v_pos) * w.z + (m3 * v_pos) * w.w;

        let skinned_n =
            (m0 * v_norm) * w.x + (m1 * v_norm) * w.y + (m2 * v_norm) * w.z + (m3 * v_norm) * w.w;

        out_positions[i] = Vector3 {
            x: skinned_p.x,
            y: skinned_p.y,
            z: skinned_p.z,
        };

        out_normals[i] = Vector3 {
            x: skinned_n.x,
            y: skinned_n.y,
            z: skinned_n.z,
        };
    }
}
