use super::parse_chunk_elements;
use crate::engine::common::magic;
use crate::engine::math::{BoneRotation, Vector3, Vector4};
use crate::utils::gltf_builder::GltfBuilder;
use anyhow::Result;
use binrw::{BinRead, BinReaderExt, BinWrite};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Cursor;

#[inline]
fn sanitize_f32(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v } else { fallback }
}

#[derive(Debug, Clone, BinRead, BinWrite)]
#[brw(little)]
pub struct ObjectBone {
    #[br(map = |b: [u8; 32]| {
        let s = String::from_utf8_lossy(&b);
        s.split('\0').next().unwrap_or("").trim().to_string()
    })]
    #[bw(map = |s: &String| {
        let mut b = [0u8; 32];
        let bs = s.as_bytes();
        b[..bs.len().min(32)].copy_from_slice(&bs[..bs.len().min(32)]);
        b
    })]
    pub name: String,

    pub matrix: [f32; 16],
    pub rotation: Vector4,
    pub translation: Vector3,

    pub skin_id: i32,
    pub parent_index: i32,
    pub next_sibling_index: i32,
    pub first_child_index: i32,
    pub reserved: i32,
}

pub fn parse_object_bone_container(data: &[u8]) -> Result<Vec<ObjectBone>> {
    let mut payload = data.to_vec();

    // Блок ID 33 в объектах — это контейнер, где сами кости лежат в блоке ID 22 (DATA_BLOB)
    if let Ok((_, elements)) = parse_chunk_elements(data) {
        if let Some((_, bone_bytes)) = elements.iter().find(|(id, _)| *id == 22) {
            payload = bone_bytes.clone();
        } else if let Some((_, bone_bytes)) = elements.iter().find(|(id, _)| *id == 33) {
            payload = bone_bytes.clone();
        }
    } else if let Ok((_, typed_elements)) = super::parse_typed_container(data) {
        for (id, chunk) in typed_elements {
            if id == 33 || id == 22 {
                payload = chunk;
                break;
            } else if let Ok((_, sub_elem)) = parse_chunk_elements(&chunk)
                && let Some((_, sub_bones)) = sub_elem
                    .into_iter()
                    .find(|(sid, _)| *sid == 33 || *sid == 22)
            {
                payload = sub_bones;
                break;
            }
        }
    } else if data.len() > 15 && data[0] == 0x03 && data[1] == 0x14 {
        payload = data[15..].to_vec();
    }

    if payload.len() < 144 {
        return Ok(Vec::new());
    }

    let mut cur = Cursor::new(payload.as_slice());
    let mut bones = Vec::new();

    while (cur.position() as usize) + 144 <= payload.len() {
        if let Ok(bone) = cur.read_le::<ObjectBone>() {
            let clean_name = bone
                .name
                .chars()
                .filter(|c| c.is_ascii_graphic() || *c == ' ' || *c == '_')
                .collect::<String>();

            let is_valid_name = !clean_name.is_empty()
                && clean_name.len() >= 2
                && !clean_name.starts_with('[')
                && clean_name.is_ascii();

            let is_valid_transform = bone.translation.x.is_finite()
                && bone.translation.y.is_finite()
                && bone.translation.z.is_finite()
                && bone.translation.x.abs() < 50_000.0
                && bone.translation.y.abs() < 50_000.0
                && bone.translation.z.abs() < 50_000.0
                && bone.rotation.w.is_finite();

            let is_valid_hierarchy = bone.parent_index >= -1 && bone.parent_index < 1024;

            if is_valid_name && is_valid_transform && is_valid_hierarchy {
                let mut valid_bone = bone;
                valid_bone.name = clean_name;
                bones.push(valid_bone);
            } else if !bones.is_empty() {
                break;
            }
        } else {
            break;
        }
    }

    Ok(bones)
}

pub fn export_skeleton_to_glb(bones: &[ObjectBone], rig_name: &str) -> Result<Vec<u8>> {
    if bones.is_empty() {
        anyhow::bail!("No valid bones found to export skeleton");
    }

    let mut builder = GltfBuilder::new();
    let num_bones = bones.len();

    let mut armature_children = Vec::new();
    let mut bone_nodes = Vec::new();

    for (i, bone) in bones.iter().enumerate() {
        let bone_node_id = i + 1;

        let mut children = Vec::new();
        for (j, b2) in bones.iter().enumerate() {
            if b2.parent_index == i as i32 && j != i {
                children.push(j + 1);
            }
        }

        if bone.parent_index < 0 || bone.parent_index >= num_bones as i32 {
            armature_children.push(bone_node_id);
        }

        let n = if bone.name.is_empty() {
            format!("Bone_{:03}", i)
        } else {
            bone.name.clone()
        };

        let tx = sanitize_f32(bone.translation.x, 0.0);
        let ty = sanitize_f32(bone.translation.y, 0.0);
        let tz = sanitize_f32(bone.translation.z, 0.0);

        let rx = sanitize_f32(bone.rotation.x, 0.0);
        let ry = sanitize_f32(bone.rotation.y, 0.0);
        let rz = sanitize_f32(bone.rotation.z, 0.0);
        let rw = if bone.rotation.w.is_finite() && bone.rotation.w != 0.0 {
            bone.rotation.w
        } else {
            1.0
        };

        let mut node = json!({
            "name": n,
            "translation": [tx, ty, tz],
            "rotation": [rx, ry, rz, rw]
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

#[derive(Debug, Serialize, Deserialize)]
pub struct KeyframeTranslation {
    pub time_seconds: f32,
    pub position: Vector3,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KeyframeRotation {
    pub time_seconds: f32,
    pub rotation_euler: BoneRotation,
    pub rotation_quat: Vector4,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BoneTrack {
    pub bone_name: String,
    pub translations: Vec<KeyframeTranslation>,
    pub rotations: Vec<KeyframeRotation>,
}

#[derive(Debug, Serialize, Deserialize)]
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
            20 if chunk.len() >= 4 => {
                let len = Cursor::new(&chunk[0..4])
                    .read_u32::<LittleEndian>()
                    .unwrap_or(0) as usize;
                if len + 4 <= chunk.len()
                    && let Ok(s) = std::str::from_utf8(&chunk[4..4 + len])
                {
                    target_rig = s.trim_matches(char::from(0)).to_string();
                }
            }
            21 if chunk.len() >= 4 => {
                let len = Cursor::new(&chunk[0..4])
                    .read_u32::<LittleEndian>()
                    .unwrap_or(0) as usize;
                if len + 4 <= chunk.len()
                    && let Ok(s) = std::str::from_utf8(&chunk[4..4 + len])
                {
                    name = s.trim_matches(char::from(0)).to_string();
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
            20 if chunk.len() >= 4 => {
                let len = Cursor::new(&chunk[0..4])
                    .read_u32::<LittleEndian>()
                    .unwrap_or(0) as usize;
                if len + 4 <= chunk.len()
                    && let Ok(s) = std::str::from_utf8(&chunk[4..4 + len])
                {
                    bone_name = s.trim_matches(char::from(0)).trim().to_string();
                }
            }
            // ID 22 / 24: Translations (Root Motion or static position)
            22 | 24 => {
                if let Ok((_, trans_sub)) = parse_chunk_elements(&chunk) {
                    for (_, tchunk) in trans_sub {
                        parse_translation_blob(&tchunk, total_duration, &mut translations);
                    }
                } else {
                    parse_translation_blob(&chunk, total_duration, &mut translations);
                }
            }
            // ID 23 / 25: Rotations (Packed 6-byte Euler streams)
            23 | 25 => {
                if let Ok((_, rot_sub)) = parse_chunk_elements(&chunk) {
                    for (_, rchunk) in rot_sub {
                        if let Ok((_, data_sub)) = parse_chunk_elements(&rchunk) {
                            for (_, dchunk) in data_sub {
                                parse_rotation_blob(&dchunk, total_duration, &mut rotations);
                            }
                        } else {
                            parse_rotation_blob(&rchunk, total_duration, &mut rotations);
                        }
                    }
                } else {
                    parse_rotation_blob(&chunk, total_duration, &mut rotations);
                }
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

fn parse_translation_blob(
    data: &[u8],
    total_duration: f32,
    translations: &mut Vec<KeyframeTranslation>,
) {
    if data.len() < 12 {
        return;
    }

    // Check if data is array of (u32 timestamp_micros + 3x f32 position) = 16 bytes
    if data.len() >= 16 && data.len().is_multiple_of(16) {
        let mut cur = Cursor::new(data);
        while (cur.position() as usize) + 16 <= data.len() {
            let micros = cur.read_u32::<LittleEndian>().unwrap_or(0);
            let px = sanitize_f32(cur.read_f32::<LittleEndian>().unwrap_or(0.0), 0.0);
            let py = sanitize_f32(cur.read_f32::<LittleEndian>().unwrap_or(0.0), 0.0);
            let pz = sanitize_f32(cur.read_f32::<LittleEndian>().unwrap_or(0.0), 0.0);

            let time_seconds = (micros as f32 / 1_000_000.0).min(total_duration);
            translations.push(KeyframeTranslation {
                time_seconds,
                position: Vector3 {
                    x: px,
                    y: py,
                    z: pz,
                },
            });
        }
    } else if data.len() >= 12 {
        // Single static position keyframe
        let mut cur = Cursor::new(data);
        let px = sanitize_f32(cur.read_f32::<LittleEndian>().unwrap_or(0.0), 0.0);
        let py = sanitize_f32(cur.read_f32::<LittleEndian>().unwrap_or(0.0), 0.0);
        let pz = sanitize_f32(cur.read_f32::<LittleEndian>().unwrap_or(0.0), 0.0);
        translations.push(KeyframeTranslation {
            time_seconds: 0.0,
            position: Vector3 {
                x: px,
                y: py,
                z: pz,
            },
        });
    }
}

fn parse_rotation_blob(data: &[u8], total_duration: f32, rotations: &mut Vec<KeyframeRotation>) {
    if data.len() < 6 {
        return;
    }

    let mut cur = Cursor::new(data);
    let count = data.len() / 6;

    for i in 0..count {
        if let Ok(br) = BoneRotation::read(&mut cur) {
            let time_seconds = if count > 1 {
                i as f32 * total_duration / (count - 1) as f32
            } else {
                0.0
            };
            let q = br.to_quaternion();
            let qx = sanitize_f32(q.x, 0.0);
            let qy = sanitize_f32(q.y, 0.0);
            let qz = sanitize_f32(q.z, 0.0);
            let qw = if q.w.is_finite() && q.w != 0.0 {
                q.w
            } else {
                1.0
            };

            rotations.push(KeyframeRotation {
                time_seconds,
                rotation_euler: br,
                rotation_quat: Vector4 {
                    x: qx,
                    y: qy,
                    z: qz,
                    w: qw,
                },
            });
        }
    }
}

pub fn export_animation_to_json(chunk_data: &[u8]) -> Result<String> {
    let clip = parse_animation_clip(chunk_data)?;
    serde_json::to_string_pretty(&clip).map_err(|e| anyhow::anyhow!(e))
}

pub fn export_animation_to_glb(chunk_data: &[u8]) -> Result<Vec<u8>> {
    let clip = parse_animation_clip(chunk_data)?;

    let mut bin_data = Vec::new();
    let mut nodes = Vec::new();
    let mut channels = Vec::new();
    let mut samplers = Vec::new();
    let mut buffer_views = Vec::new();
    let mut accessors = Vec::new();

    let armature_node_id = 0;
    let mut bone_indices = Vec::new();

    for track in &clip.bone_tracks {
        let bone_node_id = nodes.len() + 1;
        bone_indices.push(bone_node_id);

        nodes.push(json!({
            "name": track.bone_name
        }));

        if !track.translations.is_empty() {
            let time_offset = bin_data.len();
            let mut min_time = f32::INFINITY;
            let mut max_time = f32::NEG_INFINITY;

            for t in &track.translations {
                let ts = sanitize_f32(t.time_seconds, 0.0);
                bin_data.write_f32::<LittleEndian>(ts)?;
                min_time = min_time.min(ts);
                max_time = max_time.max(ts);
            }
            while !bin_data.len().is_multiple_of(4) {
                bin_data.push(0);
            }
            let time_len = track.translations.len() * 4;

            let val_offset = bin_data.len();
            for t in &track.translations {
                bin_data.write_f32::<LittleEndian>(sanitize_f32(t.position.x, 0.0))?;
                bin_data.write_f32::<LittleEndian>(sanitize_f32(t.position.y, 0.0))?;
                bin_data.write_f32::<LittleEndian>(sanitize_f32(t.position.z, 0.0))?;
            }
            while !bin_data.len().is_multiple_of(4) {
                bin_data.push(0);
            }
            let val_len = track.translations.len() * 12;

            let time_bv_idx = buffer_views.len();
            buffer_views
                .push(json!({ "buffer": 0, "byteOffset": time_offset, "byteLength": time_len }));

            let val_bv_idx = buffer_views.len();
            buffer_views
                .push(json!({ "buffer": 0, "byteOffset": val_offset, "byteLength": val_len }));

            let time_acc_idx = accessors.len();
            accessors.push(json!({
                "bufferView": time_bv_idx,
                "byteOffset": 0,
                "componentType": 5126,
                "count": track.translations.len(),
                "type": "SCALAR",
                "min": [min_time],
                "max": [max_time]
            }));

            let val_acc_idx = accessors.len();
            accessors.push(json!({
                "bufferView": val_bv_idx,
                "byteOffset": 0,
                "componentType": 5126,
                "count": track.translations.len(),
                "type": "VEC3"
            }));

            let sampler_idx = samplers.len();
            samplers.push(json!({
                "input": time_acc_idx,
                "output": val_acc_idx,
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
            let time_offset = bin_data.len();
            let mut min_time = f32::INFINITY;
            let mut max_time = f32::NEG_INFINITY;

            for r in &track.rotations {
                let ts = sanitize_f32(r.time_seconds, 0.0);
                bin_data.write_f32::<LittleEndian>(ts)?;
                min_time = min_time.min(ts);
                max_time = max_time.max(ts);
            }
            while !bin_data.len().is_multiple_of(4) {
                bin_data.push(0);
            }
            let time_len = track.rotations.len() * 4;

            let val_offset = bin_data.len();
            for r in &track.rotations {
                bin_data.write_f32::<LittleEndian>(sanitize_f32(r.rotation_quat.x, 0.0))?;
                bin_data.write_f32::<LittleEndian>(sanitize_f32(r.rotation_quat.y, 0.0))?;
                bin_data.write_f32::<LittleEndian>(sanitize_f32(r.rotation_quat.z, 0.0))?;
                let qw = if r.rotation_quat.w.is_finite() && r.rotation_quat.w != 0.0 {
                    r.rotation_quat.w
                } else {
                    1.0
                };
                bin_data.write_f32::<LittleEndian>(qw)?;
            }
            while !bin_data.len().is_multiple_of(4) {
                bin_data.push(0);
            }
            let val_len = track.rotations.len() * 16;

            let time_bv_idx = buffer_views.len();
            buffer_views
                .push(json!({ "buffer": 0, "byteOffset": time_offset, "byteLength": time_len }));

            let val_bv_idx = buffer_views.len();
            buffer_views
                .push(json!({ "buffer": 0, "byteOffset": val_offset, "byteLength": val_len }));

            let time_acc_idx = accessors.len();
            accessors.push(json!({
                "bufferView": time_bv_idx,
                "byteOffset": 0,
                "componentType": 5126,
                "count": track.rotations.len(),
                "type": "SCALAR",
                "min": [min_time],
                "max": [max_time]
            }));

            let val_acc_idx = accessors.len();
            accessors.push(json!({
                "bufferView": val_bv_idx,
                "byteOffset": 0,
                "componentType": 5126,
                "count": track.rotations.len(),
                "type": "VEC4"
            }));

            let sampler_idx = samplers.len();
            samplers.push(json!({
                "input": time_acc_idx,
                "output": val_acc_idx,
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

    let mut all_nodes = vec![json!({
        "name": format!("{}_Armature", clip.name),
        "children": bone_indices
    })];
    all_nodes.extend(nodes);

    let gltf_json = json!({
        "asset": {
            "version": "2.0",
            "generator": "Overlord Modding Studio Skeletal Animation Exporter"
        },
        "scene": 0,
        "scenes": [{ "nodes": [armature_node_id] }],
        "nodes": all_nodes,
        "skins": [{
            "name": format!("{}_Skin", clip.target_rig),
            "joints": bone_indices
        }],
        "animations": [{
            "name": clip.name,
            "channels": channels,
            "samplers": samplers
        }],
        "buffers": [{ "byteLength": bin_data.len() }],
        "bufferViews": buffer_views,
        "accessors": accessors
    });

    let mut json_bytes = serde_json::to_vec(&gltf_json)?;
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }

    let total_length = 12 + 8 + json_bytes.len() + 8 + bin_data.len();
    let mut glb = Vec::with_capacity(total_length);

    glb.extend_from_slice(b"glTF");
    glb.write_u32::<LittleEndian>(2)?;
    glb.write_u32::<LittleEndian>(total_length as u32)?;

    glb.write_u32::<LittleEndian>(json_bytes.len() as u32)?;
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&json_bytes);

    glb.write_u32::<LittleEndian>(bin_data.len() as u32)?;
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&bin_data);

    Ok(glb)
}
