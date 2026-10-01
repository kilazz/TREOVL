use crate::engine::assets::parse_chunk_elements;
use crate::engine::math::{BoneRotation, Vector3, Vector4};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Cursor;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transform {
    pub matrix: [f32; 16],
    pub rotation: Vector4,
    pub translation: Vector3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectBone {
    pub name: String,
    pub transform: Transform,
    pub skin_id: i32,
    pub parent_index: i32,
    pub next_sibling_index: i32,
    pub first_child_index: i32,
    pub reserved: i32,
}

impl ObjectBone {
    /// Reads a 144-byte ObjectBone from the binary cursor
    pub fn read(cur: &mut Cursor<&[u8]>) -> Result<Self, std::io::Error> {
        let mut name_bytes = [0u8; 32];
        std::io::Read::read_exact(cur, &mut name_bytes)?;

        // Find first null terminator to get clean name without trailing garbage
        let str_len = name_bytes.iter().position(|&b| b == 0).unwrap_or(32);
        let name = String::from_utf8_lossy(&name_bytes[..str_len])
            .trim()
            .to_string();

        let mut matrix = [0f32; 16];
        for m in &mut matrix {
            let val = cur.read_f32::<LittleEndian>()?;
            *m = if val.is_finite() { val } else { 0.0 };
        }

        let qx = cur.read_f32::<LittleEndian>()?;
        let qy = cur.read_f32::<LittleEndian>()?;
        let qz = cur.read_f32::<LittleEndian>()?;
        let qw = cur.read_f32::<LittleEndian>()?;

        let rotation = Vector4 {
            x: if qx.is_finite() { qx } else { 0.0 },
            y: if qy.is_finite() { qy } else { 0.0 },
            z: if qz.is_finite() { qz } else { 0.0 },
            w: if qw.is_finite() { qw } else { 1.0 },
        };

        let tx = cur.read_f32::<LittleEndian>()?;
        let ty = cur.read_f32::<LittleEndian>()?;
        let tz = cur.read_f32::<LittleEndian>()?;

        let translation = Vector3 {
            x: if tx.is_finite() { tx } else { 0.0 },
            y: if ty.is_finite() { ty } else { 0.0 },
            z: if tz.is_finite() { tz } else { 0.0 },
        };

        // 5 integers (5 * 4 = 20 bytes) -> total exact 144 bytes!
        let skin_id = cur.read_i32::<LittleEndian>()?;
        let parent_index = cur.read_i32::<LittleEndian>()?;
        let next_sibling_index = cur.read_i32::<LittleEndian>()?;
        let first_child_index = cur.read_i32::<LittleEndian>()?;
        let reserved = cur.read_i32::<LittleEndian>()?;

        Ok(Self {
            name,
            transform: Transform {
                matrix,
                rotation,
                translation,
            },
            skin_id,
            parent_index,
            next_sibling_index,
            first_child_index,
            reserved,
        })
    }
}

/// Parses the ObjectBoneContainer (chunk ID 33) into a clean array of ObjectBones.
pub fn parse_object_bone_container(data: &[u8]) -> Result<Vec<ObjectBone>, String> {
    // Offset table: 7 bytes header + 8 bytes payload start = 15 bytes
    let bones_slice = if data.len() > 15 && data[0] == 0x03 && data[1] == 0x14 {
        &data[15..]
    } else {
        data
    };

    if bones_slice.len() < 144 {
        return Err("Bone buffer is too small to contain an ObjectBone".into());
    }

    let mut cur = Cursor::new(bones_slice);
    let mut bones = Vec::new();

    while (cur.position() as usize) + 144 <= bones_slice.len() {
        if let Ok(bone) = ObjectBone::read(&mut cur) {
            bones.push(bone);
        } else {
            break;
        }
    }

    if bones.is_empty() {
        Err("No valid bones could be parsed".into())
    } else {
        Ok(bones)
    }
}

/// Exports a hierarchical bone skeleton into a valid glTF 2.0 Binary (.glb) Armature for Blender.
pub fn export_skeleton_to_glb(bones: &[ObjectBone], rig_name: &str) -> Result<Vec<u8>, String> {
    if bones.is_empty() {
        return Err("Cannot export empty bone array".into());
    }

    let mut nodes = Vec::new();
    let mut root_indices = Vec::new();
    let total_bones = bones.len();

    for (i, bone) in bones.iter().enumerate() {
        let mut children = Vec::new();
        for (child_idx, other_bone) in bones.iter().enumerate() {
            if other_bone.parent_index == i as i32 && child_idx != i {
                children.push(child_idx);
            }
        }

        // If root bone (parent_index is -1 or out of range), add to scene roots
        if bone.parent_index < 0 || bone.parent_index >= total_bones as i32 {
            root_indices.push(i);
        }

        let bone_name = if bone.name.is_empty() {
            format!("Bone_{:02}", i)
        } else {
            bone.name.clone()
        };

        let mut node_json = json!({
            "name": bone_name,
            "translation": [bone.transform.translation.x, bone.transform.translation.y, bone.transform.translation.z],
            "rotation": [bone.transform.rotation.x, bone.transform.rotation.y, bone.transform.rotation.z, bone.transform.rotation.w]
        });

        if !children.is_empty() {
            node_json["children"] = json!(children);
        }

        nodes.push(node_json);
    }

    if root_indices.is_empty() {
        root_indices.push(0);
    }

    let gltf_json = json!({
        "asset": {
            "version": "2.0",
            "generator": "Overlord Modding Studio Armature Exporter"
        },
        "scene": 0,
        "scenes": [{
            "name": rig_name,
            "nodes": root_indices
        }],
        "nodes": nodes
    });

    let mut json_bytes = serde_json::to_vec(&gltf_json).unwrap();
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }

    let total_length = 12 + 8 + json_bytes.len();
    let mut glb = Vec::with_capacity(total_length);

    // 12-byte glTF Header
    glb.extend_from_slice(b"glTF");
    glb.write_u32::<LittleEndian>(2).unwrap();
    glb.write_u32::<LittleEndian>(total_length as u32).unwrap();

    // Chunk 0: JSON
    glb.write_u32::<LittleEndian>(json_bytes.len() as u32)
        .unwrap();
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&json_bytes);

    Ok(glb)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BonePosition {
    pub timestamp: u32,
    pub pos: Vector3,
}

impl BonePosition {
    pub fn read(cur: &mut Cursor<&[u8]>) -> Result<Self, std::io::Error> {
        Ok(Self {
            timestamp: cur.read_u32::<LittleEndian>()?,
            pos: Vector3 {
                x: cur.read_f32::<LittleEndian>()?,
                y: cur.read_f32::<LittleEndian>()?,
                z: cur.read_f32::<LittleEndian>()?,
            },
        })
    }
}

/// Parses an Overlord Animation container chunk (05 00 41 00).
pub fn parse_animation_clip(chunk_data: &[u8]) -> Result<AnimationClip, String> {
    let payload = if chunk_data.len() > 4 && &chunk_data[0..4] == b"\x05\x00\x41\x00" {
        &chunk_data[4..]
    } else {
        chunk_data
    };

    let (_, elements) = parse_chunk_elements(payload)?;

    let mut name = String::from("Unnamed_Animation");
    let mut target_rig = String::from("Generic_Rig");
    let mut frame_rate = 30.0f32;
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
                frame_rate = Cursor::new(chunk)
                    .read_f32::<LittleEndian>()
                    .unwrap_or(30.0);
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
                        if sub_id == 10
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

fn parse_single_bone_track(data: &[u8], total_duration: f32) -> Result<BoneTrack, String> {
    let payload = if data.len() > 4 && &data[0..4] == b"\x07\x00\x41\x00" {
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
                    bone_name = s.trim_matches(char::from(0)).to_string();
                }
            }
            24 => {
                if let Ok((_, trans_sub)) = parse_chunk_elements(&chunk) {
                    for (tid, tchunk) in trans_sub {
                        if tid == 22 && tchunk.len() >= 16 {
                            let mut c = Cursor::new(tchunk.as_slice());
                            while (c.position() as usize) + 16 <= tchunk.len() {
                                if let Ok(bp) = BonePosition::read(&mut c) {
                                    translations.push(KeyframeTranslation {
                                        time_seconds: bp.timestamp as f32 / 1_000_000.0,
                                        position: bp.pos,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            25 => {
                if let Ok((_, rot_sub)) = parse_chunk_elements(&chunk) {
                    for (rid, rchunk) in rot_sub {
                        if rid == 21
                            && let Ok((_, data_sub)) = parse_chunk_elements(&rchunk)
                        {
                            for (did, dchunk) in data_sub {
                                if did == 23 && dchunk.len() >= 6 {
                                    let mut c = Cursor::new(dchunk.as_slice());
                                    let count = dchunk.len() / 6;
                                    let mut i = 0;
                                    while (c.position() as usize) + 6 <= dchunk.len() {
                                        if let Ok(br) = BoneRotation::read(&mut c) {
                                            let time_seconds = if count > 1 {
                                                i as f32 * total_duration / (count - 1) as f32
                                            } else {
                                                0.0
                                            };
                                            rotations.push(KeyframeRotation {
                                                time_seconds,
                                                rotation_euler: br,
                                                rotation_quat: br.to_quaternion(),
                                            });
                                            i += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
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

pub fn export_animation_to_json(chunk_data: &[u8]) -> Result<String, String> {
    let clip = parse_animation_clip(chunk_data)?;
    serde_json::to_string_pretty(&clip).map_err(|e| e.to_string())
}

pub fn export_animation_to_glb(chunk_data: &[u8]) -> Result<Vec<u8>, String> {
    let clip = parse_animation_clip(chunk_data)?;

    let mut bin_data = Vec::new();
    let mut nodes = Vec::new();
    let mut scene_nodes = Vec::new();
    let mut channels = Vec::new();
    let mut samplers = Vec::new();
    let mut buffer_views = Vec::new();
    let mut accessors = Vec::new();

    for (bone_idx, track) in clip.bone_tracks.iter().enumerate() {
        let node_id = nodes.len();
        scene_nodes.push(node_id);

        nodes.push(json!({
            "name": track.bone_name
        }));

        if !track.translations.is_empty() {
            let time_offset = bin_data.len();
            let mut min_time = f32::INFINITY;
            let mut max_time = f32::NEG_INFINITY;

            for t in &track.translations {
                bin_data.write_f32::<LittleEndian>(t.time_seconds).unwrap();
                min_time = min_time.min(t.time_seconds);
                max_time = max_time.max(t.time_seconds);
            }
            while !bin_data.len().is_multiple_of(4) {
                bin_data.push(0);
            }
            let time_len = track.translations.len() * 4;

            let val_offset = bin_data.len();
            for t in &track.translations {
                bin_data.write_f32::<LittleEndian>(t.position.x).unwrap();
                bin_data.write_f32::<LittleEndian>(t.position.y).unwrap();
                bin_data.write_f32::<LittleEndian>(t.position.z).unwrap();
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
                    "node": bone_idx,
                    "path": "translation"
                }
            }));
        }

        if !track.rotations.is_empty() {
            let time_offset = bin_data.len();
            let mut min_time = f32::INFINITY;
            let mut max_time = f32::NEG_INFINITY;

            for r in &track.rotations {
                bin_data.write_f32::<LittleEndian>(r.time_seconds).unwrap();
                min_time = min_time.min(r.time_seconds);
                max_time = max_time.max(r.time_seconds);
            }
            while !bin_data.len().is_multiple_of(4) {
                bin_data.push(0);
            }
            let time_len = track.rotations.len() * 4;

            let val_offset = bin_data.len();
            for r in &track.rotations {
                bin_data
                    .write_f32::<LittleEndian>(r.rotation_quat.x)
                    .unwrap();
                bin_data
                    .write_f32::<LittleEndian>(r.rotation_quat.y)
                    .unwrap();
                bin_data
                    .write_f32::<LittleEndian>(r.rotation_quat.z)
                    .unwrap();
                bin_data
                    .write_f32::<LittleEndian>(r.rotation_quat.w)
                    .unwrap();
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
                    "node": bone_idx,
                    "path": "rotation"
                }
            }));
        }
    }

    let gltf_json = json!({
        "asset": {
            "version": "2.0",
            "generator": "Overlord Modding Studio Skeletal Animation Exporter"
        },
        "scene": 0,
        "scenes": [{ "nodes": scene_nodes }],
        "nodes": nodes,
        "animations": [{
            "name": clip.name,
            "channels": channels,
            "samplers": samplers
        }],
        "buffers": [{ "byteLength": bin_data.len() }],
        "bufferViews": buffer_views,
        "accessors": accessors
    });

    let mut json_bytes = serde_json::to_vec(&gltf_json).unwrap();
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }

    let total_length = 12 + 8 + json_bytes.len() + 8 + bin_data.len();
    let mut glb = Vec::with_capacity(total_length);

    glb.extend_from_slice(b"glTF");
    glb.write_u32::<LittleEndian>(2).unwrap();
    glb.write_u32::<LittleEndian>(total_length as u32).unwrap();

    glb.write_u32::<LittleEndian>(json_bytes.len() as u32)
        .unwrap();
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&json_bytes);

    glb.write_u32::<LittleEndian>(bin_data.len() as u32)
        .unwrap();
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&bin_data);

    Ok(glb)
}
