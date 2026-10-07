use glam::{Mat4, Quat, Vec3};

use super::bone::ObjectBone;
use super::clip::{AnimationClip, KeyframeRotation, KeyframeTranslation};
use crate::engine::math::GridVertex;

#[allow(dead_code)]
fn get_dx9_bone_transform(
    _rot_quat: [f32; 4],
    _trans_vec: [f32; 3],
    raw_matrix: &[f32; 16],
) -> Mat4 {
    let m = raw_matrix;
    let is_valid_matrix = m[15] == 1.0 && m[0].is_finite() && m[5].is_finite() && m[10].is_finite();

    if is_valid_matrix {
        Mat4::from_cols_array(&[
            m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8], m[9], m[10], m[11], m[12], m[13],
            m[14], m[15],
        ])
    } else {
        let rot =
            Quat::from_xyzw(_rot_quat[0], _rot_quat[1], _rot_quat[2], _rot_quat[3]).normalize();
        let trans = Vec3::new(_trans_vec[0], _trans_vec[1], _trans_vec[2]);
        Mat4::from_rotation_translation(rot, trans)
    }
}

pub fn compute_skinning_matrices(
    bones: &[ObjectBone],
    clip: &AnimationClip,
    time_seconds: f32,
) -> (Vec<Mat4>, Vec<GridVertex>, Vec<Vec3>) {
    let num_bones = bones.len();
    if num_bones == 0 {
        return (Vec::new(), Vec::new(), Vec::new());
    }

    // 1. Compute Bind Global Matrices in Native Model Space
    let mut bind_global = Vec::with_capacity(num_bones);
    let mut bind_inv_matrices = Vec::with_capacity(num_bones);

    fn get_bind_local(bone: &ObjectBone) -> Mat4 {
        let rot = Quat::from_xyzw(
            bone.rotation.x,
            bone.rotation.y,
            bone.rotation.z,
            bone.rotation.w,
        )
        .normalize();
        let trans = Vec3::new(bone.translation.x, bone.translation.y, bone.translation.z);
        Mat4::from_rotation_translation(rot, trans)
    }

    fn calc_bind_global(
        idx: usize,
        bones: &[ObjectBone],
        cache: &mut [Option<Mat4>],
        visited: &mut [bool],
    ) -> Mat4 {
        if let Some(m) = cache[idx] {
            return m;
        }
        if visited[idx] {
            return Mat4::IDENTITY;
        }
        visited[idx] = true;

        let local_m = get_bind_local(&bones[idx]);
        let p_idx = bones[idx].parent_index;

        let global_m = if p_idx >= 0 && (p_idx as usize) < bones.len() && (p_idx as usize) != idx {
            let parent_m = calc_bind_global(p_idx as usize, bones, cache, visited);
            parent_m * local_m
        } else {
            local_m
        };

        cache[idx] = Some(global_m);
        global_m
    }

    let mut rest_cache = vec![None; num_bones];
    for i in 0..num_bones {
        let mut visited = vec![false; num_bones];
        let bg = calc_bind_global(i, bones, &mut rest_cache, &mut visited);
        bind_global.push(bg);
        bind_inv_matrices.push(bg.inverse());
    }

    // 2. Compute Animated Local Matrices
    let mut local_animated = Vec::with_capacity(num_bones);

    for bone in bones {
        let track_opt = clip.bone_tracks.iter().find(|t| t.bone_name == bone.name);

        let is_root = bone.parent_index < 0
            || bone.name.eq_ignore_ascii_case("root")
            || bone.name.eq_ignore_ascii_case("bip01");

        // Translations only apply to the root bone to keep child limb lengths/joints stable
        let translation = if is_root
            && let Some(track) = track_opt
            && !track.translations.is_empty()
        {
            sample_translation(&track.translations, time_seconds)
        } else {
            Vec3::new(bone.translation.x, bone.translation.y, bone.translation.z)
        };

        let rest_q = Quat::from_xyzw(
            bone.rotation.x,
            bone.rotation.y,
            bone.rotation.z,
            bone.rotation.w,
        )
        .normalize();

        let rotation = if let Some(track) = track_opt
            && !track.rotations.is_empty()
        {
            let mut anim_q = sample_rotation(&track.rotations, time_seconds);
            if rest_q.dot(anim_q) < 0.0 {
                anim_q = -anim_q;
            }
            anim_q
        } else {
            rest_q
        };

        let local_m = Mat4::from_rotation_translation(rotation, translation);
        local_animated.push(local_m);
    }

    // 3. Compute Animated Global Matrices in Native Model Space
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

    // 4. Extract True Fully-Animated 3D Bone Joint Positions
    let bone_positions: Vec<Vec3> = global_animated
        .iter()
        .map(|g_opt| g_opt.unwrap_or(Mat4::IDENTITY).transform_point3(Vec3::ZERO))
        .collect();

    // 5. Build line segments connecting animated bones
    let mut debug_lines = Vec::new();
    let magenta = [1.0, 0.0, 1.0, 1.0];
    let cyan = [0.0, 1.0, 1.0, 1.0];

    for i in 0..num_bones {
        let p_idx = bones[i].parent_index;
        if p_idx >= 0 && (p_idx as usize) < num_bones {
            let parent_pos = bone_positions[p_idx as usize];
            let child_pos = bone_positions[i];

            debug_lines.push(GridVertex {
                position: [parent_pos.x, parent_pos.y, parent_pos.z],
                color: magenta,
            });
            debug_lines.push(GridVertex {
                position: [child_pos.x, child_pos.y, child_pos.z],
                color: cyan,
            });
        }
    }

    // 6. Compute Final Skinning Matrices: (Global_Anim * Bind_Inv)
    let mut skin_matrices = vec![Mat4::IDENTITY; 128];
    let mut slot_assigned = [false; 128];

    // Check if bone_ids are non-negative, valid, and unique within 0..128
    let has_unique_bone_ids = {
        let mut ids = std::collections::HashSet::new();
        let mut valid = true;
        for b in bones {
            if b.bone_id < 0 || b.bone_id >= 128 || !ids.insert(b.bone_id) {
                valid = false;
                break;
            }
        }
        valid && !bones.is_empty()
    };

    if has_unique_bone_ids {
        // Models indexing directly by distinct bone_id
        for (i, bone) in bones.iter().enumerate() {
            let g = global_animated[i].unwrap_or(Mat4::IDENTITY);
            let mat_skin = g * bind_inv_matrices[i];
            let b_idx = bone.bone_id as usize;
            skin_matrices[b_idx] = mat_skin;
            slot_assigned[b_idx] = true;
        }

        // Fill empty sequential slots for meshes referencing 0..num_bones array index
        for i in 0..num_bones.min(128) {
            if !slot_assigned[i] {
                let g = global_animated[i].unwrap_or(Mat4::IDENTITY);
                skin_matrices[i] = g * bind_inv_matrices[i];
            }
        }
    } else {
        // Models indexing sequentially by array position i
        for i in 0..num_bones.min(128) {
            let g = global_animated[i].unwrap_or(Mat4::IDENTITY);
            skin_matrices[i] = g * bind_inv_matrices[i];
            slot_assigned[i] = true;
        }

        // Map bone_ids without clobbering existing sequential assignments
        for (i, bone) in bones.iter().enumerate() {
            let b_id = bone.bone_id;
            if b_id >= 0 && (b_id as usize) < skin_matrices.len() {
                let b_idx = b_id as usize;
                if !slot_assigned[b_idx] {
                    let g = global_animated[i].unwrap_or(Mat4::IDENTITY);
                    skin_matrices[b_idx] = g * bind_inv_matrices[i];
                    slot_assigned[b_idx] = true;
                }
            }
        }
    }

    (skin_matrices, debug_lines, bone_positions)
}

pub fn sample_translation(keys: &[KeyframeTranslation], time: f32) -> Vec3 {
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

pub fn sample_rotation(keys: &[KeyframeRotation], time: f32) -> Quat {
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
            let mut q1 = Quat::from_xyzw(
                k1.rotation_quat.x,
                k1.rotation_quat.y,
                k1.rotation_quat.z,
                k1.rotation_quat.w,
            )
            .normalize();

            if q0.dot(q1) < 0.0 {
                q1 = -q1;
            }

            return q0.slerp(q1, factor).normalize();
        }
    }

    let q = &keys[0].rotation_quat;
    Quat::from_xyzw(q.x, q.y, q.z, q.w).normalize()
}
