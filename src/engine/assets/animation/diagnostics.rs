use glam::{Mat4, Quat, Vec3};

use super::bone::ObjectBone;
use super::clip::AnimationClip;
use super::kinematics::compute_skinning_matrices;
use crate::engine::math::{Vector3, Vector4};

pub fn run_deep_skeleton_diagnostics(
    bones: &[ObjectBone],
    clip: Option<&AnimationClip>,
    mesh_positions: &[Vector3],
    joints: &[[u16; 4]],
    weights: &[Vector4],
) -> Vec<String> {
    let mut log_lines = Vec::new();
    let num_bones = bones.len();

    log_lines.push(
        "================================================================================".into(),
    );
    log_lines.push("🔬 TREOVL MAXIMUM SKELETON & SKINNING DIAGNOSTICS AUDIT".into());
    log_lines.push(
        "================================================================================".into(),
    );

    let mut mesh_center = Vec3::ZERO;
    if !mesh_positions.is_empty() {
        for p in mesh_positions {
            mesh_center += Vec3::new(p.x, p.y, p.z);
        }
        mesh_center /= mesh_positions.len() as f32;
    }

    let mut max_joint = 0u16;
    for j in joints {
        max_joint = max_joint.max(j[0]).max(j[1]).max(j[2]).max(j[3]);
    }

    log_lines.push(format!(
        "• Mesh Vertices: {} | Mesh Centroid: [{:.3}, {:.3}, {:.3}]",
        mesh_positions.len(),
        mesh_center.x,
        mesh_center.y,
        mesh_center.z
    ));
    log_lines.push(format!(
        "• Bones in Rig: {} | Max Joint Index in Mesh: {}",
        num_bones, max_joint
    ));

    if let Some(c) = clip {
        log_lines.push(format!(
            "• Active Clip: '{}' | Duration: {:.3}s @ {:.1} FPS | Tracks: {}/{}",
            c.name,
            c.duration_seconds,
            c.frame_rate,
            c.bone_tracks.len(),
            num_bones
        ));
    } else {
        log_lines.push("• Active Pose: [REST POSE / BIND POSE]".into());
    }

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

    let root_bg = if !bones.is_empty() {
        let mut vis = vec![false; num_bones];
        let mut cache = vec![None; num_bones];
        fn calc_r(idx: usize, b: &[ObjectBone], c: &mut [Option<Mat4>], v: &mut [bool]) -> Mat4 {
            if let Some(m) = c[idx] {
                return m;
            }
            if v[idx] {
                return Mat4::IDENTITY;
            }
            v[idx] = true;
            let lm = get_bind_local(&b[idx]);
            let p = b[idx].parent_index;
            let gm = if p >= 0 && (p as usize) < b.len() && (p as usize) != idx {
                calc_r(p as usize, b, c, v) * lm
            } else {
                lm
            };
            c[idx] = Some(gm);
            gm
        }
        calc_r(0, bones, &mut cache, &mut vis)
    } else {
        Mat4::IDENTITY
    };

    let skeleton_root_pos = root_bg.transform_point3(Vec3::ZERO);
    let spatial_offset = skeleton_root_pos - mesh_center;
    log_lines.push(format!(
        "• Skeleton Root Pos: [{:.3}, {:.3}, {:.3}] | Offset (Skeleton - Mesh): [{:.3}, {:.3}, {:.3}]",
        skeleton_root_pos.x, skeleton_root_pos.y, skeleton_root_pos.z,
        spatial_offset.x, spatial_offset.y, spatial_offset.z
    ));

    log_lines.push("\n--- [1] COMPLETE BONE HIERARCHY & REST POSE TRANSFORM AUDIT ---".into());
    log_lines.push(format!(
        "{:<4} | {:<18} | {:<7} | {:<7} | {:<22} | {:<24} | {:<16}",
        "Idx",
        "Bone Name",
        "Parent",
        "Bone ID",
        "Rest Translation",
        "Rest Quaternion (XYZW)",
        "Matrix Trans (File)"
    ));
    log_lines.push("-".repeat(110));

    for (i, b) in bones.iter().enumerate() {
        let parent_str = if b.parent_index < 0 {
            "ROOT".to_string()
        } else {
            format!("{:02}", b.parent_index)
        };

        let trans_str = format!(
            "[{:.3}, {:.3}, {:.3}]",
            b.translation.x, b.translation.y, b.translation.z
        );
        let quat_str = format!(
            "[{:.3}, {:.3}, {:.3}, {:.3}]",
            b.rotation.x, b.rotation.y, b.rotation.z, b.rotation.w
        );
        let raw_mat_trans = format!(
            "[{:.2}, {:.2}, {:.2}]",
            b.matrix[12], b.matrix[13], b.matrix[14]
        );

        log_lines.push(format!(
            "{:02}   | {:<18} | {:<7} | {:<7} | {:<22} | {:<24} | {:<16}",
            i, b.name, parent_str, b.bone_id, trans_str, quat_str, raw_mat_trans
        ));
    }

    if let Some(c) = clip {
        log_lines
            .push("\n--- [2] ANIMATION TRACKS DELTA AT T=0 (Rest Pose vs Keyframe 0) ---".into());
        log_lines.push(format!(
            "{:<4} | {:<18} | {:<10} | {:<22} | {:<11} | {:<24} | {:<14}",
            "Idx",
            "Bone Name",
            "Keys (T/R)",
            "Anim Trans (Key 0)",
            "Trans Delta",
            "Anim Quat (Key 0)",
            "Angle Delta"
        ));
        log_lines.push("-".repeat(116));

        for (i, b) in bones.iter().enumerate() {
            let track_opt = c.bone_tracks.iter().find(|t| t.bone_name == b.name);
            if let Some(track) = track_opt {
                let key_counts = format!("{}/{}", track.translations.len(), track.rotations.len());

                let (anim_trans_str, trans_delta_str) = if let Some(t0) = track.translations.first()
                {
                    let d = Vec3::new(
                        t0.position.x - b.translation.x,
                        t0.position.y - b.translation.y,
                        t0.position.z - b.translation.z,
                    )
                    .length();
                    (
                        format!(
                            "[{:.3}, {:.3}, {:.3}]",
                            t0.position.x, t0.position.y, t0.position.z
                        ),
                        format!("{:.4}m", d),
                    )
                } else {
                    ("N/A (Uses Rest)".into(), "0.0000m".into())
                };

                let (anim_quat_str, angle_delta_str) = if let Some(r0) = track.rotations.first() {
                    let q_rest =
                        Quat::from_xyzw(b.rotation.x, b.rotation.y, b.rotation.z, b.rotation.w)
                            .normalize();
                    let q_anim = Quat::from_xyzw(
                        r0.rotation_quat.x,
                        r0.rotation_quat.y,
                        r0.rotation_quat.z,
                        r0.rotation_quat.w,
                    )
                    .normalize();
                    let dot = q_rest.dot(q_anim).abs().clamp(-1.0, 1.0);
                    let angle_deg = 2.0 * dot.acos().to_degrees();

                    (
                        format!(
                            "[{:.3}, {:.3}, {:.3}, {:.3}]",
                            r0.rotation_quat.x,
                            r0.rotation_quat.y,
                            r0.rotation_quat.z,
                            r0.rotation_quat.w
                        ),
                        format!("{:.1}°", angle_deg),
                    )
                } else {
                    ("N/A (Uses Rest)".into(), "0.0°".into())
                };

                log_lines.push(format!(
                    "{:02}   | {:<18} | {:<10} | {:<22} | {:<11} | {:<24} | {:<14}",
                    i,
                    b.name,
                    key_counts,
                    anim_trans_str,
                    trans_delta_str,
                    anim_quat_str,
                    angle_delta_str
                ));
            } else {
                log_lines.push(format!(
                    "{:02}   | {:<18} | [NO TRACK] | {:<22} | {:<11} | {:<24} | {:<14}",
                    i, b.name, "---", "0.0000m", "---", "0.0°"
                ));
            }
        }
    }

    if let Some(c) = clip {
        let (diag_matrices, _, _) = compute_skinning_matrices(bones, c, 0.0);

        let mut displaced_verts = Vec::new();

        for (v_i, &p_rest) in mesh_positions.iter().enumerate() {
            let j = joints.get(v_i).copied().unwrap_or([0; 4]);
            let w = weights.get(v_i).copied().unwrap_or(Vector4::default());
            let w_sum = w.x + w.y + w.z + w.w;

            if w_sum > 0.001 {
                let m0 = diag_matrices
                    .get((j[0] as usize).min(127))
                    .copied()
                    .unwrap_or(Mat4::IDENTITY);
                let m1 = diag_matrices
                    .get((j[1] as usize).min(127))
                    .copied()
                    .unwrap_or(Mat4::IDENTITY);
                let m2 = diag_matrices
                    .get((j[2] as usize).min(127))
                    .copied()
                    .unwrap_or(Mat4::IDENTITY);
                let m3 = diag_matrices
                    .get((j[3] as usize).min(127))
                    .copied()
                    .unwrap_or(Mat4::IDENTITY);

                let blended = m0 * w.x + m1 * w.y + m2 * w.z + m3 * w.w;
                let p_anim = blended.transform_point3(Vec3::new(p_rest.x, p_rest.y, p_rest.z));
                let dist = (p_anim - Vec3::new(p_rest.x, p_rest.y, p_rest.z)).length();

                displaced_verts.push((v_i, p_rest, p_anim, dist, j, w));
            }
        }

        displaced_verts.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));

        log_lines.push("\n--- [3] TOP 10 DISPLACED VERTICES (Rest Pose -> Frame 0) ---".into());
        log_lines.push(format!(
            "{:<5} | {:<7} | {:<22} | {:<22} | {:<9} | {:<16} | {:<18}",
            "Rank",
            "Vert #",
            "Rest Pos [X, Y, Z]",
            "Anim Pos [X, Y, Z]",
            "Disp (m)",
            "Joints [0..3]",
            "Primary Bone"
        ));
        log_lines.push("-".repeat(110));

        for (rank, (v_i, p_rest, p_anim, dist, j, w)) in displaced_verts.iter().take(10).enumerate()
        {
            let p_rest_str = format!("[{:.2}, {:.2}, {:.2}]", p_rest.x, p_rest.y, p_rest.z);
            let p_anim_str = format!("[{:.2}, {:.2}, {:.2}]", p_anim.x, p_anim.y, p_anim.z);
            let joints_str = format!("[{}, {}, {}, {}]", j[0], j[1], j[2], j[3]);
            let bone_name = bones
                .get(j[0] as usize)
                .map(|b| b.name.as_str())
                .unwrap_or("Unknown");

            log_lines.push(format!(
                "#{:<4} | {:<7} | {:<22} | {:<22} | {:<9.3} | {:<16} | #{} '{}' (w={:.2})",
                rank + 1,
                v_i,
                p_rest_str,
                p_anim_str,
                dist,
                joints_str,
                j[0],
                bone_name,
                w.x
            ));
        }
    }

    log_lines.push(
        "================================================================================\n".into(),
    );
    log_lines
}
