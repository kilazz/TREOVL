use std::sync::{Arc, Mutex};

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

pub fn center_camera_for_preview(
    state: &Arc<Mutex<AppState>>,
    preview: &ActiveMeshPreview,
) -> ViewportCamera {
    let mut st = state.lock().unwrap();
    let up_axis = st.camera.up_axis;

    let mut min = Vector3 {
        x: f32::INFINITY,
        y: f32::INFINITY,
        z: f32::INFINITY,
    };
    let mut max = Vector3 {
        x: f32::NEG_INFINITY,
        y: f32::NEG_INFINITY,
        z: f32::NEG_INFINITY,
    };

    let transform = |p: Vector3| -> Vector3 {
        let aligned = Vector3 {
            x: p.x,
            y: -p.z,
            z: p.y,
        };
        match up_axis {
            1 => Vector3 {
                x: aligned.x,
                y: -aligned.z,
                z: aligned.y,
            },
            2 => Vector3 {
                x: aligned.x,
                y: aligned.z,
                z: -aligned.y,
            },
            3 => Vector3 {
                x: aligned.x,
                y: -aligned.y,
                z: -aligned.z,
            },
            _ => aligned,
        }
    };

    for sm in &preview.submeshes {
        for &p in &sm.rest_positions {
            let tp = transform(p);
            min.x = min.x.min(tp.x);
            min.y = min.y.min(tp.y);
            min.z = min.z.min(tp.z);
            max.x = max.x.max(tp.x);
            max.y = max.y.max(tp.y);
            max.z = max.z.max(tp.z);
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
                ui.set_mesh_info(
                    "3D Viewport unavailable (GPU adapter initialization failed)".into(),
                );
            }
        });
        return;
    }

    let mut bone_labels = Vec::new();

    let (is_skinning, _is_root_motion, show_mesh, show_skeleton, show_names, show_wire, show_grid) = {
        let st = state.lock().unwrap();
        (
            st.is_skinning_enabled,
            st.is_root_motion_enabled,
            st.show_mesh,
            st.show_skeleton,
            st.show_bone_names,
            st.show_wireframe,
            st.show_grid,
        )
    };

    let (skin_matrices, lines, bone_positions) = if let Some(clip_idx) = preview.current_clip_index
        && let Some(clip) = preview.available_clips.get(clip_idx)
    {
        let t = preview.current_time_seconds;
        if let Some(sm) = preview.submeshes.first() {
            if !sm.bones.is_empty() {
                compute_skinning_matrices(&sm.bones, clip, t)
            } else {
                (Vec::new(), Vec::new(), Vec::new())
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
            let p = Vector3 {
                x: line_vert.position[0],
                y: line_vert.position[1],
                z: line_vert.position[2],
            };
            let tp = match cam.up_axis {
                1 => [p.x, -p.z, p.y],
                2 => [p.x, p.z, -p.y],
                3 => [p.x, -p.y, -p.z],
                _ => [p.x, p.y, p.z],
            };
            line_vert.position = tp;
            debug_lines.push(line_vert);
        }
    }

    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for sm in &preview.submeshes {
        for &p in &sm.rest_positions {
            let aligned = Vec3::new(p.x, -p.z, p.y);
            let tp = match cam.up_axis {
                1 => Vec3::new(aligned.x, -aligned.z, aligned.y),
                2 => Vec3::new(aligned.x, aligned.z, -aligned.y),
                3 => Vec3::new(aligned.x, -aligned.y, -aligned.z),
                _ => aligned,
            };
            min = min.min(tp);
            max = max.max(tp);
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
                let raw_p = bone_positions[b_idx];
                let tp = match cam.up_axis {
                    1 => Vec3::new(raw_p.x, -raw_p.z, raw_p.y),
                    2 => Vec3::new(raw_p.x, raw_p.z, -raw_p.y),
                    3 => Vec3::new(raw_p.x, -raw_p.y, -raw_p.z),
                    _ => Vec3::new(raw_p.x, raw_p.y, raw_p.z),
                };
                let world_pos = Vec4::new(tp.x, tp.y, tp.z, 1.0);
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

    let options = RenderOptions {
        is_skinning_enabled: is_skinning,
        show_grid,
        show_wire,
        size: (1024, 1024),
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
            Err(e) => {
                eprintln!("[!] Viewport render error: {:#}", e);
            }
        }
    }
}
