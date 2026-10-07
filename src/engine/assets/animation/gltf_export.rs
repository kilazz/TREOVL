use anyhow::Result;
use byteorder::{LittleEndian, WriteBytesExt};
use glam::Quat;
use serde_json::json;

use super::bone::ObjectBone;
use super::clip::{parse_animation_clip, sanitize_f32};
use crate::utils::gltf_builder::GltfBuilder;

/// Basis change quaternion from Triumph Engine (Z-Up) to glTF 2.0 / Blender (Y-Up):
/// -90 degrees rotation around the X-axis.
#[inline]
pub fn gltf_basis_quat() -> Quat {
    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
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

        // Z-up to Y-up mapping for GLB Export
        let (tx, ty, tz) = if is_root {
            (
                sanitize_f32(bone.translation.x, 0.0),
                sanitize_f32(bone.translation.z, 0.0),
                sanitize_f32(-bone.translation.y, 0.0),
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
                    val_bytes.write_f32::<LittleEndian>(pz)?;
                    val_bytes.write_f32::<LittleEndian>(-py)?;
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
