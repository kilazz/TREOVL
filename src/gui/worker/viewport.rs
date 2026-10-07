use parking_lot::Mutex;
use std::sync::Arc;

use glam::{Mat4, Vec3, Vec4};
use slint::{Image, ModelRc, VecModel};

use crate::AppWindow;
use crate::engine::assets::animation::compute_skinning_matrices;
use crate::engine::math::Vector3;
use crate::gui::{ActiveMeshPreview, AppState};
use crate::utils::renderer::{
    RenderOptions, SubmeshDrawData, TextureData, ViewportCamera, WgpuRenderer,
};

pub fn perspective_rh_zo(fov_y_radians: f32, aspect_ratio: f32, z_near: f32, z_far: f32) -> Mat4 {
    let f = 1.0 / (fov_y_radians / 2.0).tan();
    Mat4::from_cols_array(&[
        f / aspect_ratio,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        z_far / (z_near - z_far),
        -1.0,
        0.0,
        0.0,
        (z_far * z_near) / (z_near - z_far),
        0.0,
    ])
}

pub fn look_at_rh(eye: Vec3, center: Vec3, up: Vec3) -> Mat4 {
    let f = (center - eye).normalize();
    let s = f.cross(up).normalize();
    let u = s.cross(f);
    Mat4::from_cols_array(&[
        s.x,
        u.x,
        -f.x,
        0.0,
        s.y,
        u.y,
        -f.y,
        0.0,
        s.z,
        u.z,
        -f.z,
        0.0,
        -eye.dot(s),
        -eye.dot(u),
        eye.dot(f),
        1.0,
    ])
}

/// Mesh mapping: Flipped 180° around X so the model stands upright on feet in Ground mode by default.
/// Base: X -> X, Y -> -Z, Z -> Y
#[inline]
fn map_mesh_coords(p: Vec3, up_axis: u32) -> [f32; 3] {
    let aligned = [p.x, -p.z, p.y];
    match up_axis {
        1 => [aligned[0], -aligned[2], aligned[1]], // Pitch Up (+90°)
        2 => [aligned[0], aligned[2], -aligned[1]], // Pitch Down (-90°)
        3 => [aligned[0], -aligned[1], -aligned[2]], // Inverted (180°)
        _ => aligned,                               // 0: Ground (Default)
    }
}

/// Skeleton mapping: Rotated +90° around X relative to the mesh [p.x, -p.z, p.y]
/// Orthogonal +90° rotation around X: [p.x, p.y, p.z]
#[inline]
fn map_skeleton_coords(p: Vec3, up_axis: u32) -> [f32; 3] {
    let aligned = [p.x, p.y, p.z];
    match up_axis {
        1 => [aligned[0], -aligned[2], aligned[1]], // Pitch Up (+90°)
        2 => [aligned[0], aligned[2], -aligned[1]], // Pitch Down (-90°)
        3 => [aligned[0], -aligned[1], -aligned[2]], // Inverted (180°)
        _ => aligned,                               // 0: Ground (Default)
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
            st.is_interacting || preview.is_playing,
        )
    };

    let (skin_matrices, lines, bone_positions) = if let Some(sm) = preview.submeshes.first() {
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
    if show_skeleton {
        for mut line_vert in lines {
            let p = Vec3::new(
                line_vert.position[0],
                line_vert.position[1],
                line_vert.position[2],
            );
            line_vert.position = map_skeleton_coords(p, cam.up_axis);
            debug_lines.push(line_vert);
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
                uvs: &sm.uvs,
                joints: &sm.joints,
                weights: &sm.weights,
                texture: sm.texture.as_ref().map(|t| TextureData {
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
