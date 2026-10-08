use parking_lot::Mutex;
use std::sync::Arc;

use glam::{Mat4, Vec3, Vec4};
use slint::{Image, ModelRc, VecModel};

use crate::AppWindow;
use crate::engine::assets::animation::compute_skinning_matrices;
use crate::engine::math::{GridVertex, Vector3};
use crate::gui::{ActiveMeshPreview, AppState};
use crate::utils::renderer::{
    RenderOptions, SubmeshDrawData, TextureData, ViewportCamera, WgpuRenderer, look_at_rh,
    perspective_rh_zo,
};

#[inline]
fn map_mesh_coords(p: Vec3, up_axis: u32) -> [f32; 3] {
    let aligned = [p.x, -p.z, p.y];
    match up_axis {
        1 => [aligned[0], -aligned[2], aligned[1]],
        2 => [aligned[0], aligned[2], -aligned[1]],
        3 => [aligned[0], -aligned[1], -aligned[2]],
        4 => [-aligned[1], aligned[0], aligned[2]],
        5 => [aligned[1], -aligned[0], aligned[2]],
        _ => aligned,
    }
}

#[inline]
fn map_skeleton_coords(p: Vec3, up_axis: u32) -> [f32; 3] {
    let aligned = [p.x, -p.z, p.y];
    match up_axis {
        1 => [aligned[0], -aligned[2], aligned[1]],
        2 => [aligned[0], aligned[2], -aligned[1]],
        3 => [aligned[0], -aligned[1], -aligned[2]],
        4 => [-aligned[1], aligned[0], aligned[2]],
        5 => [aligned[1], -aligned[0], aligned[2]],
        _ => aligned,
    }
}

pub fn center_camera_for_preview(
    state: &Arc<Mutex<AppState>>,
    preview: &ActiveMeshPreview,
) -> ViewportCamera {
    let mut st = state.lock();
    let up_axis = st.camera.up_axis;

    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);

    for sm in &preview.submeshes {
        for &p in &sm.rest_positions {
            let tp = map_mesh_coords(Vec3::new(p.x, p.y, p.z), up_axis);
            min = min.min(Vec3::from(tp));
            max = max.max(Vec3::from(tp));
        }
    }

    let sx = (max.x - min.x).abs();
    let sy = (max.y - min.y).abs();
    let sz = (max.z - min.z).abs();
    let max_dim = sx.max(sy).max(sz).max(1.0);
    let auto_dist = (max_dim * 1.75).clamp(1.5, 300.0);

    st.camera.distance = auto_dist;
    st.camera.target = Vector3 {
        x: (min.x + max.x) * 0.5,
        y: (min.y + max.y) * 0.5,
        z: (min.z + max.z) * 0.5,
    };
    st.camera
}

pub fn evaluate_and_render_animated_frame(
    ui_handle: &slint::Weak<AppWindow>,
    state: &Arc<Mutex<AppState>>,
    gpu_renderer: &mut Option<WgpuRenderer>,
    preview: &mut ActiveMeshPreview,
    cam: &ViewportCamera,
) {
    if gpu_renderer.is_none() {
        let ui_h = ui_handle.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_h.upgrade() {
                ui.set_has_mesh(false);
                ui.set_mesh_info("3D Viewport unavailable".into());
            }
        });
        return;
    }

    let mut bone_labels = Vec::new();

    let (
        is_skinning,
        _is_root_motion,
        show_mesh,
        show_skeleton,
        show_names,
        show_wire,
        show_grid,
        show_xray,
        is_interacting,
    ) = {
        let st = state.lock();
        (
            st.is_skinning_enabled,
            st.is_root_motion_enabled,
            st.show_mesh,
            st.show_skeleton,
            st.show_bone_names,
            st.show_wireframe,
            st.show_grid,
            st.show_xray,
            st.is_interacting || preview.is_playing,
        )
    };

    let (skin_matrices, _, bone_positions) = if let Some(sm) = preview.submeshes.first() {
        if !sm.bones.is_empty() {
            if let Some(clip_idx) = preview.current_clip_index
                && let Some(clip) = preview.available_clips.get(clip_idx)
            {
                compute_skinning_matrices(&sm.bones, clip, preview.current_time_seconds)
            } else {
                let empty_clip = crate::engine::assets::animation::AnimationClip {
                    name: "RestPose".into(),
                    target_rig: "".into(),
                    frame_rate: 30.0,
                    duration_seconds: 0.0,
                    bone_tracks: Vec::new(),
                };
                compute_skinning_matrices(&sm.bones, &empty_clip, 0.0)
            }
        } else {
            (Vec::new(), Vec::new(), Vec::new())
        }
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };

    let mut debug_lines = Vec::new();
    if show_skeleton && let Some(sm) = preview.submeshes.first() {
        let dummy_size = 0.025f32;
        let num_bones = sm.bones.len();

        for i in 0..num_bones {
            if i >= bone_positions.len() {
                continue;
            }

            let p_idx = sm.bones[i].parent_index;
            let child_raw = bone_positions[i];
            let child_pos = map_skeleton_coords(child_raw, cam.up_axis);

            if p_idx >= 0 && (p_idx as usize) < bone_positions.len() {
                let parent_raw = bone_positions[p_idx as usize];
                let parent_pos = map_skeleton_coords(parent_raw, cam.up_axis);

                debug_lines.push(GridVertex {
                    position: parent_pos,
                    color: [1.0, 0.0, 1.0, 0.9],
                });
                debug_lines.push(GridVertex {
                    position: child_pos,
                    color: [0.0, 1.0, 1.0, 0.9],
                });
            }

            let joint_color = [1.0, 0.8, 0.2, 1.0];

            debug_lines.push(GridVertex {
                position: [child_pos[0] - dummy_size, child_pos[1], child_pos[2]],
                color: joint_color,
            });
            debug_lines.push(GridVertex {
                position: [child_pos[0] + dummy_size, child_pos[1], child_pos[2]],
                color: joint_color,
            });

            debug_lines.push(GridVertex {
                position: [child_pos[0], child_pos[1] - dummy_size, child_pos[2]],
                color: joint_color,
            });
            debug_lines.push(GridVertex {
                position: [child_pos[0], child_pos[1] + dummy_size, child_pos[2]],
                color: joint_color,
            });

            debug_lines.push(GridVertex {
                position: [child_pos[0], child_pos[1], child_pos[2] - dummy_size],
                color: joint_color,
            });
            debug_lines.push(GridVertex {
                position: [child_pos[0], child_pos[1], child_pos[2] + dummy_size],
                color: joint_color,
            });
        }
    }

    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);

    for sm in &preview.submeshes {
        for (i, &p) in sm.rest_positions.iter().enumerate() {
            let mut local_p = Vec3::new(p.x, p.y, p.z);

            if is_skinning && !skin_matrices.is_empty() && !sm.joints.is_empty() {
                let j = sm.joints[i];
                let w = sm.weights[i];
                let w_sum = w.x + w.y + w.z + w.w;
                if w_sum > 0.001 {
                    let m0 = skin_matrices
                        .get(j[0] as usize)
                        .copied()
                        .unwrap_or(Mat4::IDENTITY);
                    let m1 = skin_matrices
                        .get(j[1] as usize)
                        .copied()
                        .unwrap_or(Mat4::IDENTITY);
                    let m2 = skin_matrices
                        .get(j[2] as usize)
                        .copied()
                        .unwrap_or(Mat4::IDENTITY);
                    let m3 = skin_matrices
                        .get(j[3] as usize)
                        .copied()
                        .unwrap_or(Mat4::IDENTITY);

                    let blended = m0 * w.x + m1 * w.y + m2 * w.z + m3 * w.w;
                    local_p = blended.transform_point3(local_p);
                }
            }

            let tp = map_mesh_coords(local_p, cam.up_axis);
            min = min.min(Vec3::from(tp));
            max = max.max(Vec3::from(tp));
        }
    }

    let center = if min.x.is_finite() {
        (min + max) * 0.5
    } else {
        Vec3::ZERO
    };

    let aspect = 1.0f32;
    let fov_rad = cam.fov_degrees.to_radians().clamp(0.4, 2.0);
    let proj = perspective_rh_zo(fov_rad, aspect, 0.1, 2000.0);

    let eye = center
        + Vec3::new(
            cam.yaw.sin() * cam.pitch.cos() * cam.distance,
            cam.pitch.sin() * cam.distance,
            cam.yaw.cos() * cam.pitch.cos() * cam.distance,
        );
    let view = look_at_rh(eye, center, Vec3::Y);
    let view_proj = proj * view;

    if show_names
        && !bone_positions.is_empty()
        && let Some(sm) = preview.submeshes.first()
    {
        for (b_idx, bone) in sm.bones.iter().enumerate() {
            if b_idx < bone_positions.len() {
                let tp = map_skeleton_coords(bone_positions[b_idx], cam.up_axis);
                let world_pos = Vec4::new(tp[0], tp[1], tp[2], 1.0);
                let clip_pos = view_proj * world_pos;

                if clip_pos.w > 0.05 {
                    let ndc = clip_pos.truncate() / clip_pos.w;
                    if ndc.x >= -1.05
                        && ndc.x <= 1.05
                        && ndc.y >= -1.05
                        && ndc.y <= 1.05
                        && ndc.z >= 0.0
                        && ndc.z <= 1.0
                    {
                        let sx = (ndc.x * 0.5 + 0.5) * 1024.0;
                        let sy = (1.0 - (ndc.y * 0.5 + 0.5)) * 1024.0;
                        bone_labels.push(crate::BoneLabel {
                            name: bone.name.clone().into(),
                            x: sx,
                            y: sy,
                        });
                    }
                }
            }
        }
    }

    let draw_data: Vec<SubmeshDrawData> = if show_mesh {
        preview
            .submeshes
            .iter()
            .map(|sm| SubmeshDrawData {
                positions: &sm.rest_positions,
                indices: &sm.indices,
                normals: &sm.rest_normals,
                tangents: &sm.rest_tangents,
                uvs: &sm.uvs,
                joints: &sm.joints,
                weights: &sm.weights,
                texture: sm.texture.as_ref().map(|t| TextureData {
                    width: t.0,
                    height: t.1,
                    rgba: &t.2,
                }),
                normal_texture: sm.normal_texture.as_ref().map(|t| TextureData {
                    width: t.0,
                    height: t.1,
                    rgba: &t.2,
                }),
            })
            .collect()
    } else {
        Vec::new()
    };

    let render_size = if is_interacting {
        (512, 512)
    } else {
        (1024, 1024)
    };

    let options = RenderOptions {
        is_skinning_enabled: is_skinning,
        show_grid,
        show_wire,
        show_xray,
        size: render_size,
        bounds_min: min.into(),
        bounds_max: max.into(),
    };

    if let Some(renderer) = gpu_renderer {
        match renderer.render(&draw_data, &debug_lines, &skin_matrices, options, cam) {
            Ok(buf) => {
                let ui_h = ui_handle.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_h.upgrade() {
                        ui.set_mesh_preview(Image::from_rgba8(buf));
                        ui.set_bone_labels(ModelRc::from(std::rc::Rc::new(VecModel::from(
                            bone_labels,
                        ))));
                    }
                });
            }
            Err(e) => eprintln!("[!] Viewport render error: {:#}", e),
        }
    }
}
