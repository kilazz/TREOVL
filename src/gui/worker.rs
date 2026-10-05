use slint::{Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc::Receiver};

use crate::AppWindow;
use crate::engine::assets::sniffer::AssetKind;
use crate::engine::math::Vector3;
use crate::engine::service;
use crate::gui::commands::WorkerCommand;
use crate::gui::{ActiveMeshPreview, AppState, resolve_project_dir, scan_project_folder};
use crate::utils::dds_decoder;
use crate::utils::logger::UiLogger;
use crate::utils::renderer::TextureData;
use crate::utils::renderer::wgpu_backend::WgpuRenderer;

pub struct ResolvedTexture {
    pub filename: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct BackgroundWorker {
    ui_handle: slint::Weak<AppWindow>,
    logger: UiLogger,
    state: Arc<Mutex<AppState>>,
    gpu_renderer: Option<WgpuRenderer>,
}

impl BackgroundWorker {
    pub fn new(
        ui_handle: slint::Weak<AppWindow>,
        logger: UiLogger,
        state: Arc<Mutex<AppState>>,
    ) -> Self {
        let gpu_renderer = Some(WgpuRenderer::new());

        Self {
            ui_handle,
            logger,
            state,
            gpu_renderer,
        }
    }

    pub fn run(mut self, rx: Receiver<WorkerCommand>) {
        while let Ok(cmd) = rx.recv() {
            match cmd {
                WorkerCommand::RotateMeshViewport {
                    mut delta_yaw,
                    mut delta_pitch,
                } => {
                    let mut extra_commands = Vec::new();
                    while let Ok(next_cmd) = rx.try_recv() {
                        match next_cmd {
                            WorkerCommand::RotateMeshViewport {
                                delta_yaw: dy,
                                delta_pitch: dp,
                            } => {
                                delta_yaw += dy;
                                delta_pitch += dp;
                            }
                            other => {
                                extra_commands.push(other);
                                break;
                            }
                        }
                    }

                    self.handle_command(WorkerCommand::RotateMeshViewport {
                        delta_yaw,
                        delta_pitch,
                    });

                    for other_cmd in extra_commands {
                        self.handle_command(other_cmd);
                    }
                }
                other => {
                    self.handle_command(other);
                }
            }
        }
    }

    pub fn handle_command(&mut self, cmd: WorkerCommand) {
        match cmd {
            WorkerCommand::FilterAssets { query, generation } => {
                let q = query.trim().to_lowercase();
                let filtered_ui = {
                    let mut st = self.state.lock().unwrap();
                    if generation < st.filter_generation {
                        return;
                    }
                    st.filter_generation = generation;

                    let mut visible = Vec::new();
                    let mut ui_items = Vec::new();

                    for (i, token) in st.all_search_haystack.iter().enumerate() {
                        if q.is_empty() || token.contains(&q) {
                            visible.push(i);
                            if let Some(item) = st.all_ui_items.get(i) {
                                ui_items.push(item.clone());
                            }
                        }
                    }
                    st.visible_indices = visible;
                    ui_items
                };

                let ui_h = self.ui_handle.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_h.upgrade() {
                        ui.set_asset_list(ModelRc::from(std::rc::Rc::new(VecModel::from(
                            filtered_ui,
                        ))));
                        ui.set_selected_index(-1);
                    }
                });
            }

            WorkerCommand::SelectAsset {
                filtered_index,
                path,
                kind,
            } => {
                let bytes = fs::read(&path).unwrap_or_default();
                let path_str = path.to_string_lossy().to_string();

                if kind != AssetKind::Mesh {
                    let mut st = self.state.lock().unwrap();
                    st.active_mesh = None;
                }

                match kind {
                    AssetKind::Texture => {
                        let mut tex_buf = None;
                        let mut tex_desc = String::from("Invalid or corrupted texture");
                        let mut ok = false;

                        if let Ok(tex) = crate::engine::assets::texture::parse_texture_chunk(&bytes)
                        {
                            let rgba = dds_decoder::decode_to_rgba(
                                tex.width,
                                tex.height,
                                tex.format,
                                &tex.pixel_data,
                            );
                            let mut buf =
                                SharedPixelBuffer::<Rgba8Pixel>::new(tex.width, tex.height);
                            let dest = buf.make_mut_bytes();
                            let copy_len = dest.len().min(rgba.len());
                            dest[..copy_len].copy_from_slice(&rgba[..copy_len]);
                            tex_buf = Some(buf);
                            tex_desc = format!(
                                "Resolution: {}x{} | Format: {:?}",
                                tex.width, tex.height, tex.format
                            );
                            ok = true;
                        }

                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(0);
                                if let Some(buf) = tex_buf {
                                    ui.set_tex_preview(Image::from_rgba8(buf));
                                }
                                ui.set_tex_info(tex_desc.into());
                                ui.set_has_texture(ok);
                            }
                        });
                    }
                    AssetKind::Mesh => {
                        let mut mesh_info = String::from("Failed to parse mesh buffer");
                        let mut mesh_buf = None;
                        let mut has_mesh = false;
                        let mut stats_lines: Vec<slint::SharedString> = Vec::new();

                        if let Ok(parsed) =
                            crate::engine::assets::mesh::extract_mesh_geometry(&bytes)
                        {
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
                            for p in &parsed.positions {
                                min.x = min.x.min(p.x);
                                min.y = min.y.min(p.y);
                                min.z = min.z.min(p.z);
                                max.x = max.x.max(p.x);
                                max.y = max.y.max(p.y);
                                max.z = max.z.max(p.z);
                            }

                            let sx = (max.x - min.x).abs();
                            let sy = (max.y - min.y).abs();
                            let sz = (max.z - min.z).abs();

                            let resolved_tex = {
                                let st = self.state.lock().unwrap();
                                resolve_diffuse_texture(st.current_proj_dir.as_deref())
                            };

                            let tex_desc = if let Some(ref t) = resolved_tex {
                                let clean_name =
                                    t.filename.split("_chunk_").next().unwrap_or(&t.filename);
                                format!("Texture: {} ({}×{})", clean_name, t.width, t.height)
                            } else {
                                "Texture: None (Hardware Unlit)".to_string()
                            };

                            mesh_info = format!(
                                "Verts: {} | Tris: {} | Size: {:.2}m × {:.2}m × {:.2}m",
                                parsed.positions.len(),
                                parsed.indices.len() / 3,
                                sx,
                                sy,
                                sz
                            );

                            stats_lines
                                .push(format!("Vertices: {}", parsed.positions.len()).into());
                            stats_lines
                                .push(format!("Triangles: {}", parsed.indices.len() / 3).into());
                            stats_lines
                                .push(format!("Size: {:.2}m × {:.2}m × {:.2}m", sx, sy, sz).into());
                            stats_lines.push(
                                format!("Bounds Min: [{:.2}, {:.2}, {:.2}]", min.x, min.y, min.z)
                                    .into(),
                            );
                            stats_lines.push(
                                format!("Bounds Max: [{:.2}, {:.2}, {:.2}]", max.x, max.y, max.z)
                                    .into(),
                            );
                            stats_lines.push(format!("Stride: {} bytes", parsed.stride).into());
                            stats_lines.push(
                                format!(
                                    "Skinning: {}",
                                    if parsed.is_skinned {
                                        "Skinned Rig"
                                    } else {
                                        "Static Mesh"
                                    }
                                )
                                .into(),
                            );
                            stats_lines.push(tex_desc.into());

                            let tex_arc =
                                resolved_tex.map(|t| Arc::new((t.width, t.height, t.rgba)));

                            // Auto-fit camera distance based on bounding box
                            let max_dim = sx.max(sy).max(sz).max(1.0);
                            let auto_dist = (max_dim * 1.75).clamp(1.5, 300.0);

                            let cam = {
                                let mut st = self.state.lock().unwrap();
                                st.camera.distance = auto_dist;
                                st.camera.target = Vector3 {
                                    x: (min.x + max.x) * 0.5,
                                    y: (min.y + max.y) * 0.5,
                                    z: (min.z + max.z) * 0.5,
                                };
                                st.active_mesh = Some(ActiveMeshPreview {
                                    positions: parsed.positions.clone(),
                                    indices: parsed.indices.clone(),
                                    normals: parsed.normals.clone(),
                                    uvs: parsed.uvs.clone(),
                                    texture: tex_arc.clone(),
                                });
                                st.camera
                            };

                            let tex_ref = tex_arc.as_ref().map(|t| TextureData {
                                width: t.0,
                                height: t.1,
                                rgba: &t.2,
                            });

                            // --- GPU Accelerated WGPU Render ---
                            let buf = self.gpu_renderer.as_mut().unwrap().render(
                                &parsed.positions,
                                &parsed.indices,
                                &parsed.normals,
                                &parsed.uvs,
                                tex_ref.as_ref(),
                                (1024, 1024),
                                &cam,
                            );

                            mesh_buf = Some(buf);
                            has_mesh = true;
                        }

                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(3);
                                ui.set_mesh_info(mesh_info.into());
                                ui.set_mesh_stats_lines(ModelRc::from(std::rc::Rc::new(
                                    VecModel::from(stats_lines),
                                )));
                                if let Some(buf) = mesh_buf {
                                    ui.set_mesh_preview(Image::from_rgba8(buf));
                                    ui.set_has_mesh(true);
                                } else {
                                    ui.set_has_mesh(has_mesh);
                                }
                            }
                        });
                    }
                    AssetKind::Audio => {
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(1);
                            }
                        });
                    }
                    AssetKind::Material => {
                        let json = crate::engine::assets::material::export_material_to_json(&bytes)
                            .unwrap_or_default();
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(2);
                                ui.set_mat_json_text(json.into());
                            }
                        });
                    }
                    AssetKind::Lua => {
                        let disasm = crate::engine::assets::lua::disassemble_lua_bytecode(&bytes)
                            .unwrap_or_default();
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(4);
                                ui.set_mat_json_text(disasm.into());
                            }
                        });
                    }
                    AssetKind::UI => {
                        let json = crate::engine::assets::ui::export_ui_to_json(&bytes)
                            .unwrap_or_default();
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(6);
                                ui.set_mat_json_text(json.into());
                            }
                        });
                    }
                    AssetKind::Object | AssetKind::Character | AssetKind::Attachment => {
                        let json =
                            crate::engine::assets::object::export_object_to_json(&bytes, None)
                                .unwrap_or_default();
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(7);
                                ui.set_mat_json_text(json.into());
                            }
                        });
                    }
                    AssetKind::Animation => {
                        let mut info = String::new();
                        if let Ok(clip) =
                            crate::engine::assets::animation::parse_animation_clip(&bytes)
                        {
                            info = format!(
                                "Clip: {} | Rig: {} | {:.1} FPS | Duration: {:.3}s | Tracks: {}",
                                clip.name,
                                clip.target_rig,
                                clip.frame_rate,
                                clip.duration_seconds,
                                clip.bone_tracks.len()
                            );
                        }
                        let json =
                            crate::engine::assets::animation::export_animation_to_json(&bytes)
                                .unwrap_or_default();
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(8);
                                ui.set_mesh_info(info.into());
                                ui.set_mat_json_text(json.into());
                            }
                        });
                    }
                    AssetKind::TerrainPalette => {
                        let json =
                            crate::engine::assets::terrain_palette::export_terrain_palette_to_json(
                                &bytes,
                            )
                            .unwrap_or_default();
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(9);
                                ui.set_mat_json_text(json.into());
                            }
                        });
                    }
                    AssetKind::Collision => {
                        let json =
                            crate::engine::assets::collision::export_collision_to_json(&bytes, "")
                                .unwrap_or_default();
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(10);
                                ui.set_mat_json_text(json.into());
                            }
                        });
                    }
                    AssetKind::Environment => {
                        let json =
                            crate::engine::assets::environment::export_environment_to_json(&bytes)
                                .unwrap_or_default();
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(11);
                                ui.set_mat_json_text(json.into());
                            }
                        });
                    }
                    AssetKind::M8ldMap => {
                        let (_seed, xml_str) = crate::engine::assets::m8ld::decompile_8ld_to_xml(&bytes)
                            .unwrap_or_else(|_| (0x91, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Language id=\"English\">\n</Language>".into()));
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(12);
                                ui.set_mat_json_text(xml_str.into());
                            }
                        });
                    }
                    AssetKind::UiSprite => {
                        let json =
                            crate::engine::assets::ui_sprite::export_ui_sprite_collection(&bytes)
                                .unwrap_or_else(|e| {
                                    format!("Error parsing Sprite Collection: {}", e)
                                });
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(13);
                                ui.set_mat_json_text(json.into());
                            }
                        });
                    }
                    AssetKind::Dta => {
                        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                        let json = crate::engine::assets::dta::export_dta_to_json(&bytes, &stem)
                            .unwrap_or_else(|e| format!("Error decoding Lighting Set: {}", e));
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(14);
                                ui.set_mat_json_text(json.into());
                            }
                        });
                    }
                    AssetKind::VoicePackage => {
                        let json = crate::engine::assets::vpk::export_vpk_to_json(&bytes)
                            .unwrap_or_else(|e| format!("Error decoding Voice Package: {}", e));
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(15);
                                ui.set_mat_json_text(json.into());
                            }
                        });
                    }
                    _ => {
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(5);
                            }
                        });
                    }
                }
            }

            WorkerCommand::RotateMeshViewport {
                delta_yaw,
                delta_pitch,
            } => {
                let buf_opt = {
                    let mut st = self.state.lock().unwrap();
                    st.camera.yaw += delta_yaw;
                    st.camera.pitch = (st.camera.pitch + delta_pitch).clamp(-1.45, 1.45);
                    let cam = st.camera;

                    st.active_mesh.as_ref().map(|mesh| {
                        let tex_ref = mesh.texture.as_ref().map(|t| TextureData {
                            width: t.0,
                            height: t.1,
                            rgba: &t.2,
                        });

                        // Fast GPU render using persistent render targets & uniform updates
                        self.gpu_renderer.as_mut().unwrap().render(
                            &mesh.positions,
                            &mesh.indices,
                            &mesh.normals,
                            &mesh.uvs,
                            tex_ref.as_ref(),
                            (1024, 1024),
                            &cam,
                        )
                    })
                };

                if let Some(buf) = buf_opt {
                    let ui_h = self.ui_handle.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_h.upgrade() {
                            ui.set_mesh_preview(Image::from_rgba8(buf));
                            ui.set_has_mesh(true);
                        }
                    });
                }
            }

            WorkerCommand::ZoomMeshViewport { delta_zoom } => {
                let factor = if delta_zoom > 0.0 { 0.88 } else { 1.14 };
                let buf_opt = {
                    let mut st = self.state.lock().unwrap();
                    st.camera.zoom(factor);
                    let cam = st.camera;

                    st.active_mesh.as_ref().map(|mesh| {
                        let tex_ref = mesh.texture.as_ref().map(|t| TextureData {
                            width: t.0,
                            height: t.1,
                            rgba: &t.2,
                        });

                        self.gpu_renderer.as_mut().unwrap().render(
                            &mesh.positions,
                            &mesh.indices,
                            &mesh.normals,
                            &mesh.uvs,
                            tex_ref.as_ref(),
                            (1024, 1024),
                            &cam,
                        )
                    })
                };

                if let Some(buf) = buf_opt {
                    let ui_h = self.ui_handle.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_h.upgrade() {
                            ui.set_mesh_preview(Image::from_rgba8(buf));
                        }
                    });
                }
            }

            WorkerCommand::SetViewportFov { fov_degrees } => {
                let buf_opt = {
                    let mut st = self.state.lock().unwrap();
                    st.camera.fov_degrees = fov_degrees.clamp(20.0, 90.0);
                    let cam = st.camera;

                    st.active_mesh.as_ref().map(|mesh| {
                        let tex_ref = mesh.texture.as_ref().map(|t| TextureData {
                            width: t.0,
                            height: t.1,
                            rgba: &t.2,
                        });

                        self.gpu_renderer.as_mut().unwrap().render(
                            &mesh.positions,
                            &mesh.indices,
                            &mesh.normals,
                            &mesh.uvs,
                            tex_ref.as_ref(),
                            (1024, 1024),
                            &cam,
                        )
                    })
                };

                if let Some(buf) = buf_opt {
                    let ui_h = self.ui_handle.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_h.upgrade() {
                            ui.set_mesh_preview(Image::from_rgba8(buf));
                        }
                    });
                }
            }

            WorkerCommand::SetViewportLighting { mode } => {
                let buf_opt = {
                    let mut st = self.state.lock().unwrap();
                    st.camera.lighting_mode = mode;
                    let cam = st.camera;

                    st.active_mesh.as_ref().map(|mesh| {
                        let tex_ref = mesh.texture.as_ref().map(|t| TextureData {
                            width: t.0,
                            height: t.1,
                            rgba: &t.2,
                        });

                        self.gpu_renderer.as_mut().unwrap().render(
                            &mesh.positions,
                            &mesh.indices,
                            &mesh.normals,
                            &mesh.uvs,
                            tex_ref.as_ref(),
                            (1024, 1024),
                            &cam,
                        )
                    })
                };

                if let Some(buf) = buf_opt {
                    let ui_h = self.ui_handle.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_h.upgrade() {
                            ui.set_mesh_preview(Image::from_rgba8(buf));
                        }
                    });
                }
            }

            WorkerCommand::ResetViewportCamera => {
                let buf_opt = {
                    let mut st = self.state.lock().unwrap();
                    st.camera.yaw = 0.785;
                    st.camera.pitch = 0.35;
                    st.camera.fov_degrees = 45.0;

                    if let Some(ref mesh) = st.active_mesh {
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
                        for p in &mesh.positions {
                            min.x = min.x.min(p.x);
                            min.y = min.y.min(p.y);
                            min.z = min.z.min(p.z);
                            max.x = max.x.max(p.x);
                            max.y = max.y.max(p.y);
                            max.z = max.z.max(p.z);
                        }
                        let sx = (max.x - min.x).abs();
                        let sy = (max.y - min.y).abs();
                        let sz = (max.z - min.z).abs();
                        let max_dim = sx.max(sy).max(sz).max(1.0);
                        st.camera.distance = (max_dim * 1.75).clamp(1.5, 300.0);
                        st.camera.target = Vector3 {
                            x: (min.x + max.x) * 0.5,
                            y: (min.y + max.y) * 0.5,
                            z: (min.z + max.z) * 0.5,
                        };
                    }

                    let cam = st.camera;

                    st.active_mesh.as_ref().map(|mesh| {
                        let tex_ref = mesh.texture.as_ref().map(|t| TextureData {
                            width: t.0,
                            height: t.1,
                            rgba: &t.2,
                        });

                        self.gpu_renderer.as_mut().unwrap().render(
                            &mesh.positions,
                            &mesh.indices,
                            &mesh.normals,
                            &mesh.uvs,
                            tex_ref.as_ref(),
                            (1024, 1024),
                            &cam,
                        )
                    })
                };

                if let Some(buf) = buf_opt {
                    let ui_h = self.ui_handle.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_h.upgrade() {
                            ui.set_mesh_preview(Image::from_rgba8(buf));
                        }
                    });
                }
            }

            // --- STANDALONE DIRECT TOOLBOX OPERATIONS ---
            WorkerCommand::Decompile8ldDirect { src, dst } => {
                self.logger
                    .log(&format!("[*] Extracting XML from .8ld: {:?}", src));
                match service::decompile_8ld_file(&src, &dst) {
                    Ok(out_file) => {
                        self.logger.log(&format!(
                            "[+] Successfully extracted XML to: {:?}",
                            out_file
                        ));
                        self.set_ui_status("XML extracted successfully.", false);
                    }
                    Err(e) => {
                        self.logger.log(&format!("[!] 8LD Extract Error: {}", e));
                        self.set_ui_error(format!("8LD Error: {}", e));
                    }
                }
            }

            WorkerCommand::Compile8ldDirect { src, dst } => {
                self.logger
                    .log(&format!("[*] Compiling XML to .8ld: {:?}", src));
                match service::compile_8ld_file(&src, &dst) {
                    Ok(out_file) => {
                        self.logger.log(&format!(
                            "[+] Successfully compiled to .8ld: {:?}",
                            out_file
                        ));
                        self.set_ui_status("XML compiled to .8ld successfully.", false);
                    }
                    Err(e) => {
                        self.logger.log(&format!("[!] 8LD Compile Error: {}", e));
                        self.set_ui_error(format!("8LD Error: {}", e));
                    }
                }
            }

            WorkerCommand::Decompile8ldBatch { src_dir, dst_dir } => {
                self.logger.log(&format!(
                    "[*] Batch extracting .8ld files from: {:?}",
                    src_dir
                ));
                match service::batch_decompile_8ld(&src_dir, &dst_dir) {
                    Ok(count) => {
                        self.logger.log(&format!(
                            "[+] Successfully converted {} files to XML in {:?}",
                            count, dst_dir
                        ));
                        self.set_ui_status("Batch XML extraction complete.", false);
                    }
                    Err(e) => {
                        self.logger
                            .log(&format!("[!] Batch 8LD Extract Error: {}", e));
                        self.set_ui_error(format!("Batch 8LD Error: {}", e));
                    }
                }
            }

            WorkerCommand::Compile8ldBatch { src_dir, dst_dir } => {
                self.logger.log(&format!(
                    "[*] Batch compiling XML files from: {:?}",
                    src_dir
                ));
                match service::batch_compile_8ld(&src_dir, &dst_dir) {
                    Ok(count) => {
                        self.logger.log(&format!(
                            "[+] Successfully converted {} files to .8ld in {:?}",
                            count, dst_dir
                        ));
                        self.set_ui_status("Batch 8LD compilation complete.", false);
                    }
                    Err(e) => {
                        self.logger.log(&format!("[!] Batch XML->8LD Error: {}", e));
                        self.set_ui_error(format!("Batch 8LD Error: {}", e));
                    }
                }
            }

            WorkerCommand::DirectVpkToJson { src, dst } => match fs::read(&src) {
                Ok(data) => match crate::engine::assets::vpk::export_vpk_to_json(&data) {
                    Ok(json) => {
                        let _ = fs::write(&dst, json.as_bytes());
                        self.logger.log(&format!(
                            "[+] Converted Voice Package (.debug-vpk) to JSON: {:?}",
                            dst
                        ));
                        self.set_ui_status("Voice Package converted to JSON successfully.", false);
                    }
                    Err(e) => self.set_ui_error(format!("VPK Export Error: {}", e)),
                },
                Err(e) => self.set_ui_error(format!("Failed to read VPK: {}", e)),
            },

            WorkerCommand::DirectJsonToVpk {
                src_json,
                baseline_vpk,
                dst,
            } => match (fs::read_to_string(&src_json), fs::read(&baseline_vpk)) {
                (Ok(json), Ok(base)) => {
                    match crate::engine::assets::vpk::import_vpk_from_json(&json, &base) {
                        Ok(bin) => {
                            let _ = fs::write(&dst, bin);
                            self.logger.log(&format!(
                                "[+] Rebuilt Voice Package (.debug-vpk) from JSON: {:?}",
                                dst
                            ));
                            self.set_ui_status("Voice Package built successfully.", false);
                        }
                        Err(e) => self.set_ui_error(format!("VPK Compile Error: {}", e)),
                    }
                }
                (Err(e), _) => self.set_ui_error(format!("Failed to read JSON: {}", e)),
                (_, Err(e)) => self.set_ui_error(format!("Failed to read baseline VPK: {}", e)),
            },

            WorkerCommand::DirectDtaToJson { src, dst } => match fs::read(&src) {
                Ok(data) => {
                    let stem = src.file_stem().unwrap_or_default().to_string_lossy();
                    match crate::engine::assets::dta::export_dta_to_json(&data, &stem) {
                        Ok(json) => {
                            let _ = fs::write(&dst, json.as_bytes());
                            self.logger
                                .log(&format!("[+] Converted DTA to JSON: {:?}", dst));
                            self.set_ui_status("Successfully converted DTA to JSON.", false);
                        }
                        Err(e) => self.set_ui_error(format!("DTA Error: {}", e)),
                    }
                }
                Err(e) => self.set_ui_error(format!("Failed to read DTA: {}", e)),
            },

            WorkerCommand::DirectJsonToDta {
                src_json,
                baseline_dta,
                dst,
            } => match (fs::read_to_string(&src_json), fs::read(&baseline_dta)) {
                (Ok(json), Ok(base)) => {
                    match crate::engine::assets::dta::import_dta_from_json(&json, &base) {
                        Ok(bin) => {
                            let _ = fs::write(&dst, bin);
                            self.logger
                                .log(&format!("[+] Rebuilt DTA from JSON: {:?}", dst));
                            self.set_ui_status("Successfully built DTA.", false);
                        }
                        Err(e) => self.set_ui_error(format!("DTA Compile Error: {}", e)),
                    }
                }
                (Err(e), _) => self.set_ui_error(format!("Failed to read JSON: {}", e)),
                (_, Err(e)) => self.set_ui_error(format!("Failed to read baseline DTA: {}", e)),
            },

            WorkerCommand::DirectEnvToJson { src, dst } => match fs::read(&src) {
                Ok(data) => {
                    match crate::engine::assets::environment::export_environment_to_json(&data) {
                        Ok(json) => {
                            let _ = fs::write(&dst, json.as_bytes());
                            self.logger
                                .log(&format!("[+] Converted ENV to JSON: {:?}", dst));
                            self.set_ui_status("Successfully converted ENV to JSON.", false);
                        }
                        Err(e) => self.set_ui_error(format!("ENV Error: {}", e)),
                    }
                }
                Err(e) => self.set_ui_error(format!("Failed to read ENV: {}", e)),
            },

            WorkerCommand::DirectJsonToEnv {
                src_json,
                baseline_env,
                dst,
            } => match (fs::read_to_string(&src_json), fs::read(&baseline_env)) {
                (Ok(json), Ok(base)) => {
                    match crate::engine::assets::environment::import_environment_from_json(
                        &json, &base,
                    ) {
                        Ok(bin) => {
                            let _ = fs::write(&dst, bin);
                            self.logger
                                .log(&format!("[+] Rebuilt ENV from JSON: {:?}", dst));
                            self.set_ui_status("Successfully built ENV profile.", false);
                        }
                        Err(e) => self.set_ui_error(format!("ENV Compile Error: {}", e)),
                    }
                }
                (Err(e), _) => self.set_ui_error(format!("Failed to read JSON: {}", e)),
                (_, Err(e)) => self.set_ui_error(format!("Failed to read baseline ENV: {}", e)),
            },

            WorkerCommand::DirectMeshExport { src, dst, is_glb } => {
                match service::export_mesh(&src, &dst, is_glb) {
                    Ok(stats) => {
                        self.logger.log(&format!(
                            "[+] Exported 3D mesh: {:?} ({} verts, {} tris)",
                            dst, stats.vertex_count, stats.triangle_count
                        ));
                        self.set_ui_status("Mesh exported successfully.", false);
                    }
                    Err(e) => self.set_ui_error(format!("Mesh Export Error: {}", e)),
                }
            }

            WorkerCommand::DirectMeshImport {
                chunk_target,
                model_src,
                is_glb,
            } => match service::import_mesh(&chunk_target, &model_src, is_glb) {
                Ok(_) => {
                    self.logger.log(&format!(
                        "[+] Injected 3D model {:?} into {:?}",
                        model_src, chunk_target
                    ));
                    self.set_ui_status("Mesh chunk successfully updated.", false);
                }
                Err(e) => self.set_ui_error(format!("Mesh Import Error: {}", e)),
            },

            WorkerCommand::DirectAssembleLevel {
                omp_path,
                assets_dir,
                dst,
            } => {
                self.logger.log(&format!(
                    "[*] Assembling full 3D scene from {:?} using assets {:?}",
                    omp_path, assets_dir
                ));
                match fs::read(&omp_path) {
                    Ok(data) => {
                        match crate::engine::assets::map::assemble_level_scene_glb(
                            &data,
                            &assets_dir,
                        ) {
                            Ok(glb) => {
                                let _ = fs::write(&dst, glb);
                                self.logger
                                    .log(&format!("[+] Level scene assembled into: {:?}", dst));
                                self.set_ui_status(
                                    "Level scene successfully assembled with PBR textures.",
                                    false,
                                );
                            }
                            Err(e) => self.set_ui_error(format!("Level Assembly Error: {}", e)),
                        }
                    }
                    Err(e) => self.set_ui_error(format!("Failed to read OMP: {}", e)),
                }
            }

            WorkerCommand::DirectTerrainExport { src, dst, is_glb } => {
                match service::export_terrain(&src, &dst, is_glb) {
                    Ok((v, t)) => {
                        self.logger.log(&format!(
                            "[+] Exported terrain heightmap: {:?} ({} vertices, {} triangles)",
                            dst, v, t
                        ));
                        self.set_ui_status("Terrain exported successfully.", false);
                    }
                    Err(e) => self.set_ui_error(format!("Terrain Export Error: {}", e)),
                }
            }

            WorkerCommand::DirectCollisionExport { src, dst } => {
                match service::export_collision_glb(&src, &dst) {
                    Ok(size) => {
                        self.logger.log(&format!(
                            "[+] Exported 3D collision boxes: {:?} ({} bytes)",
                            dst, size
                        ));
                        self.set_ui_status("Collision exported to 3D GLB successfully.", false);
                    }
                    Err(e) => self.set_ui_error(format!("Collision Export Error: {}", e)),
                }
            }

            WorkerCommand::DirectCollisionImport {
                chunk_target,
                glb_src,
            } => match service::import_collision_glb(&chunk_target, &glb_src) {
                Ok(_) => {
                    self.logger.log(&format!(
                        "[+] Updated collision chunk {:?} from {:?}",
                        chunk_target, glb_src
                    ));
                    self.set_ui_status("Collision chunk updated.", false);
                }
                Err(e) => self.set_ui_error(format!("Collision Import Error: {}", e)),
            },

            WorkerCommand::DirectFontToJson { src, dst } => match fs::read(&src) {
                Ok(data) => {
                    match crate::engine::assets::ui_sprite::export_ui_sprite_collection(&data) {
                        Ok(json) => {
                            let _ = fs::write(&dst, json.as_bytes());
                            self.logger.log(&format!(
                                "[+] Exported UI Font / Sprite collection to JSON: {:?}",
                                dst
                            ));
                            self.set_ui_status("Font collection exported to JSON.", false);
                        }
                        Err(e) => self.set_ui_error(format!("Font Export Error: {}", e)),
                    }
                }
                Err(e) => self.set_ui_error(format!("Failed to read Font CLB: {}", e)),
            },

            WorkerCommand::DirectJsonToFont { src_json, dst } => {
                match fs::read_to_string(&src_json) {
                    Ok(text) => {
                        match crate::engine::assets::ui_sprite::import_ui_sprite_collection(&text) {
                            Ok(bin) => {
                                let _ = fs::write(&dst, bin);
                                self.logger
                                    .log(&format!("[+] Rebuilt Font CLB collection: {:?}", dst));
                                self.set_ui_status("Font collection built.", false);
                            }
                            Err(e) => self.set_ui_error(format!("Font Compile Error: {}", e)),
                        }
                    }
                    Err(e) => self.set_ui_error(format!("Failed to read JSON: {}", e)),
                }
            }

            WorkerCommand::DirectTextureExport { src, dst } => {
                match service::export_texture(&src, &dst) {
                    Ok(_) => {
                        self.logger
                            .log(&format!("[+] Exported texture chunk to: {:?}", dst));
                        self.set_ui_status("Texture exported successfully.", false);
                    }
                    Err(e) => self.set_ui_error(format!("Texture Export Error: {}", e)),
                }
            }

            WorkerCommand::DirectTextureImport {
                chunk_target,
                img_src,
            } => match service::import_texture(&chunk_target, &img_src) {
                Ok(_) => {
                    self.logger.log(&format!(
                        "[+] Updated texture chunk {:?} from {:?}",
                        chunk_target, img_src
                    ));
                    self.set_ui_status("Texture chunk updated.", false);
                }
                Err(e) => self.set_ui_error(format!("Texture Import Error: {}", e)),
            },

            WorkerCommand::DirectAudioExport { src, dst } => {
                match service::export_audio(&src, &dst) {
                    Ok(_) => {
                        self.logger
                            .log(&format!("[+] Exported audio chunk to WAV: {:?}", dst));
                        self.set_ui_status("Audio exported successfully.", false);
                    }
                    Err(e) => self.set_ui_error(format!("Audio Export Error: {}", e)),
                }
            }

            WorkerCommand::DirectAudioImport {
                chunk_target,
                wav_src,
            } => match service::import_audio(&chunk_target, &wav_src) {
                Ok(_) => {
                    self.logger.log(&format!(
                        "[+] Updated audio chunk {:?} from {:?}",
                        chunk_target, wav_src
                    ));
                    self.set_ui_status("Audio chunk updated.", false);
                }
                Err(e) => self.set_ui_error(format!("Audio Import Error: {}", e)),
            },

            // --- PROJECT & ARCHIVE OPERATIONS ---
            WorkerCommand::UnpackArchive { src, dst } => {
                self.logger
                    .log(&format!("[*] Unpacking archive: {:?}", src));
                match crate::engine::container::project::unpack_archive(&src, &dst) {
                    Ok((count, info)) => {
                        self.logger.log(&info);
                        self.logger
                            .log(&format!("[+] Unpack complete: {} chunks extracted.", count));
                        let (items, cached, haystacks) = scan_project_folder(&dst);
                        {
                            let mut st = self.state.lock().unwrap();
                            st.visible_indices = (0..items.len()).collect();
                            st.all_cached_assets = cached;
                            st.all_ui_items = items.clone();
                            st.all_search_haystack = haystacks;
                            st.current_proj_dir = Some(dst.clone());
                        }

                        let ui_h = self.ui_handle.clone();
                        let dst_str = dst.to_string_lossy().to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_active_project_dir(dst_str.into());
                                ui.set_asset_list(ModelRc::from(std::rc::Rc::new(VecModel::from(
                                    items,
                                ))));
                                ui.set_status_is_error(false);
                                ui.set_status_msg(
                                    format!("Extracted {} items into workspace.", count).into(),
                                );
                            }
                        });
                    }
                    Err(e) => {
                        self.logger.log(&format!("[!] Unpack error: {}", e));
                        self.set_ui_error(format!("Unpack Error: {}", e));
                    }
                }
            }

            WorkerCommand::LoadProject { proj_dir } => {
                let actual_dir = resolve_project_dir(&proj_dir);
                self.logger
                    .log(&format!("[*] Loading project from: {:?}", actual_dir));

                if !actual_dir.exists()
                    || (!actual_dir.join("chunks").exists()
                        && !actual_dir.join("chunks_vanilla").exists()
                        && !actual_dir.join("project.json").exists())
                {
                    self.logger.log(&format!(
                        "[!] Error: {:?} is not a valid project folder (missing project.json or chunks/)",
                        actual_dir
                    ));
                    self.set_ui_error("Error: Not a valid Overlord project folder!".into());
                    return;
                }

                let (items, cached, haystacks) = scan_project_folder(&actual_dir);
                let total = items.len();
                {
                    let mut st = self.state.lock().unwrap();
                    st.visible_indices = (0..items.len()).collect();
                    st.all_cached_assets = cached;
                    st.all_ui_items = items.clone();
                    st.all_search_haystack = haystacks;
                    st.current_proj_dir = Some(actual_dir.clone());
                }

                self.logger
                    .log(&format!("[+] Project loaded: {} items found.", total));
                let ui_h = self.ui_handle.clone();
                let actual_dir_str = actual_dir.to_string_lossy().to_string();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_h.upgrade() {
                        ui.set_active_project_dir(actual_dir_str.into());
                        ui.set_selected_index(-1);
                        ui.set_active_file_path("".into());
                        ui.set_asset_list(ModelRc::from(std::rc::Rc::new(VecModel::from(items))));
                        ui.set_status_is_error(false);
                        ui.set_status_msg(
                            format!("Loaded {} resources into workspace.", total).into(),
                        );
                    }
                });
            }

            WorkerCommand::CleanRebuild { proj_dir } => {
                let actual_dir = resolve_project_dir(&proj_dir);
                self.logger.log(&format!(
                    "[*] Performing clean rebuild from vanilla: {:?}",
                    actual_dir
                ));
                match crate::engine::container::sync::clean_rebuild_project(&actual_dir) {
                    Ok(synced) => {
                        self.logger.log(&format!(
                            "[+] Clean rebuild completed: {} assets re-synced.",
                            synced
                        ));
                        self.refresh_project_state(&actual_dir, "Clean rebuild successful!");
                    }
                    Err(e) => {
                        self.logger.log(&format!("[!] Clean rebuild error: {}", e));
                        self.set_ui_error(format!("Rebuild Error: {}", e));
                    }
                }
            }

            WorkerCommand::RevertAsset {
                proj_dir,
                chunk_path,
            } => {
                let actual_dir = resolve_project_dir(&proj_dir);
                self.logger
                    .log(&format!("[*] Reverting asset to vanilla: {:?}", chunk_path));
                match crate::engine::container::sync::revert_single_asset(&actual_dir, &chunk_path)
                {
                    Ok(_) => {
                        self.logger.log(&format!(
                            "[+] Reverted {:?} to pristine vanilla state.",
                            chunk_path
                        ));
                        self.refresh_project_state(
                            &actual_dir,
                            "Asset restored from vanilla baseline.",
                        );
                    }
                    Err(e) => {
                        self.logger.log(&format!("[!] Revert error: {}", e));
                        self.set_ui_error(format!("Revert Error: {}", e));
                    }
                }
            }

            WorkerCommand::PackArchive { proj_dir } => {
                let actual_dir = resolve_project_dir(&proj_dir);
                self.logger
                    .log(&format!("[*] Packing project: {:?}", actual_dir));
                let out = actual_dir.join("rebuilt.prp");
                match crate::engine::container::project::pack_archive(&actual_dir, &out, 0) {
                    Ok(size) => {
                        self.logger
                            .log(&format!("[+] Pack complete: {:?} ({} bytes)", out, size));
                        self.set_ui_status("Archive successfully packed to rebuilt.prp!", false);
                    }
                    Err(e) => {
                        self.logger.log(&format!("[!] Pack error: {}", e));
                        self.set_ui_error(format!("Pack Error: {}", e));
                    }
                }
            }

            WorkerCommand::CreatePatch {
                base_dir,
                mod_dir,
                out_file,
            } => match crate::utils::diff::create_diff(&base_dir, &mod_dir, &out_file) {
                Ok(count) => self.logger.log(&format!(
                    "[+] Mod patch created: {} modified items tracked.",
                    count
                )),
                Err(e) => self.logger.log(&format!("[!] Patch creation error: {}", e)),
            },

            WorkerCommand::ApplyPatch {
                target_dir,
                patch_file,
            } => match crate::utils::diff::apply_patch(&target_dir, &patch_file) {
                Ok(count) => self
                    .logger
                    .log(&format!("[+] Patch applied: {} files updated.", count)),
                Err(e) => self
                    .logger
                    .log(&format!("[!] Patch application error: {}", e)),
            },

            // --- CONTEXTUAL EXPORT OPERATIONS ---
            WorkerCommand::ExportDds {
                chunk_path,
                out_path,
            } => match service::export_texture(&chunk_path, &out_path) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] Texture exported to: {:?}", out_path)),
                Err(e) => self.logger.log(&format!("[!] DDS export error: {}", e)),
            },

            WorkerCommand::ImportDds {
                chunk_path,
                in_path,
            } => match service::import_texture(&chunk_path, &in_path) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] Chunk {:?} updated with new DDS.", chunk_path)),
                Err(e) => self
                    .logger
                    .log(&format!("[!] DDS replacement error: {}", e)),
            },

            WorkerCommand::ExportWav {
                chunk_path,
                out_path,
            } => match service::export_audio(&chunk_path, &out_path) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] Audio exported to: {:?}", out_path)),
                Err(e) => self.logger.log(&format!("[!] Audio export error: {}", e)),
            },

            WorkerCommand::ImportWav {
                chunk_path,
                in_path,
            } => match service::import_audio(&chunk_path, &in_path) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] Audio chunk {:?} updated.", chunk_path)),
                Err(e) => self.logger.log(&format!("[!] Audio import error: {}", e)),
            },

            WorkerCommand::ExportMesh {
                chunk_path,
                out_path,
                is_glb,
            } => match service::export_mesh(&chunk_path, &out_path, is_glb) {
                Ok(stats) => {
                    let kind = if is_glb { "glTF" } else { "OBJ" };
                    self.logger.log(&format!(
                        "[+] {} exported: {:?} ({} verts, {} tris, skinned: {})",
                        kind, out_path, stats.vertex_count, stats.triangle_count, stats.is_skinned
                    ));
                }
                Err(e) => self.logger.log(&format!("[!] Mesh export error: {}", e)),
            },

            WorkerCommand::ImportMesh {
                chunk_path,
                in_path,
                is_glb,
            } => match service::import_mesh(&chunk_path, &in_path, is_glb) {
                Ok(_) => {
                    let kind = if is_glb { ".glb" } else { "OBJ" };
                    self.logger.log(&format!(
                        "[+] Mesh chunk {:?} rebuilt from {} (Skinning preserved)",
                        chunk_path, kind
                    ));
                }
                Err(e) => self.logger.log(&format!("[!] Mesh import error: {}", e)),
            },

            WorkerCommand::ExportTerrain {
                chunk_path,
                out_path,
                is_glb,
            } => match service::export_terrain(&chunk_path, &out_path, is_glb) {
                Ok((v_count, tri_count)) => {
                    if is_glb {
                        self.logger.log(&format!(
                            "[+] Terrain glTF exported: {:?} ({} vertices, {} triangles)",
                            out_path, v_count, tri_count
                        ));
                    } else {
                        self.logger.log(&format!(
                            "[+] Terrain OBJ exported: {:?} ({} vertices, {} triangles)",
                            out_path, v_count, tri_count
                        ));
                    }
                }
                Err(e) => self.logger.log(&format!("[!] Terrain export error: {}", e)),
            },

            WorkerCommand::ExportCollisionGlb {
                chunk_path,
                out_path,
            } => match service::export_collision_glb(&chunk_path, &out_path) {
                Ok(size) => self.logger.log(&format!(
                    "[+] 3D Collision exported to glTF (.glb): {:?} ({} bytes)",
                    out_path, size
                )),
                Err(e) => self
                    .logger
                    .log(&format!("[!] 3D Collision GLB export error: {}", e)),
            },

            WorkerCommand::ImportCollisionGlb {
                chunk_path,
                in_path,
            } => match service::import_collision_glb(&chunk_path, &in_path) {
                Ok(_) => self.logger.log(&format!(
                    "[+] Collision chunk {:?} updated from 3D .glb boxes.",
                    chunk_path
                )),
                Err(e) => self
                    .logger
                    .log(&format!("[!] 3D Collision GLB import error: {}", e)),
            },

            WorkerCommand::ExportLua {
                chunk_path,
                out_path,
            } => match service::export_lua(&chunk_path, &out_path) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] Lua bytecode exported: {:?}", out_path)),
                Err(e) => self.logger.log(&format!("[!] Lua export error: {}", e)),
            },

            WorkerCommand::ImportLua {
                chunk_path,
                in_path,
            } => match service::import_lua(&chunk_path, &in_path) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] Lua chunk {:?} updated.", chunk_path)),
                Err(e) => self.logger.log(&format!("[!] Lua import error: {}", e)),
            },

            WorkerCommand::ExportAnimGlb {
                chunk_path,
                out_path,
            } => match service::export_anim_glb(&chunk_path, &out_path) {
                Ok(_) => self.logger.log(&format!(
                    "[+] Animation timeline exported to glTF: {:?}",
                    out_path
                )),
                Err(e) => self
                    .logger
                    .log(&format!("[!] Animation GLB export error: {}", e)),
            },

            WorkerCommand::ExportAnimJson {
                chunk_path,
                out_path,
            } => match service::export_anim_json(&chunk_path, &out_path) {
                Ok(_) => self.logger.log(&format!(
                    "[+] Animation keyframes exported to JSON: {:?}",
                    out_path
                )),
                Err(e) => self
                    .logger
                    .log(&format!("[!] Animation JSON export error: {}", e)),
            },

            // --- CONTEXTUAL ASSET SAVE OPERATIONS ---
            WorkerCommand::SaveMaterial {
                chunk_path,
                json_data,
            } => match service::save_material(&chunk_path, &json_data) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] Material chunk {:?} updated.", chunk_path)),
                Err(e) => self.logger.log(&format!("[!] Material save error: {}", e)),
            },

            WorkerCommand::SaveUI {
                chunk_path,
                json_data,
            } => match service::save_ui(&chunk_path, &json_data) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] UI layout chunk {:?} updated.", chunk_path)),
                Err(e) => self.logger.log(&format!("[!] UI save error: {}", e)),
            },

            WorkerCommand::SaveObject {
                chunk_path,
                json_data,
            } => match service::save_object(&chunk_path, &json_data) {
                Ok(_) => self.logger.log(&format!(
                    "[+] 3D Object entity chunk {:?} updated.",
                    chunk_path
                )),
                Err(e) => self.logger.log(&format!("[!] Object save error: {}", e)),
            },

            WorkerCommand::SaveTerrainPalette {
                chunk_path,
                json_data,
            } => match service::save_terrain_palette(&chunk_path, &json_data) {
                Ok(_) => self.logger.log(&format!(
                    "[+] Terrain Palette chunk {:?} updated.",
                    chunk_path
                )),
                Err(e) => self
                    .logger
                    .log(&format!("[!] Terrain Palette save error: {}", e)),
            },

            WorkerCommand::SaveEnvironment {
                chunk_path,
                json_data,
            } => match service::save_environment(&chunk_path, &json_data) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] Environment chunk {:?} updated.", chunk_path)),
                Err(e) => self
                    .logger
                    .log(&format!("[!] Environment save error: {}", e)),
            },

            WorkerCommand::SaveM8ld {
                chunk_path,
                json_data,
            } => match service::save_m8ld(&chunk_path, &json_data) {
                Ok(_) => self.logger.log(&format!(
                    "[+] Level Map Logic (.8ld) {:?} updated.",
                    chunk_path
                )),
                Err(e) => self
                    .logger
                    .log(&format!("[!] Level Map Logic save error: {}", e)),
            },

            WorkerCommand::SaveUiSprite {
                chunk_path,
                json_data,
            } => match service::save_ui_sprite(&chunk_path, &json_data) {
                Ok(_) => self.logger.log(&format!(
                    "[+] UI Sprite Collection {:?} updated.",
                    chunk_path
                )),
                Err(e) => self.logger.log(&format!("[!] UI Sprite save error: {}", e)),
            },

            WorkerCommand::SaveDta {
                chunk_path,
                json_data,
            } => match service::save_dta(&chunk_path, &json_data) {
                Ok(_) => self.logger.log(&format!(
                    "[+] Lighting Set (.dta) {:?} updated.",
                    chunk_path
                )),
                Err(e) => self
                    .logger
                    .log(&format!("[!] Lighting Set save error: {}", e)),
            },

            WorkerCommand::SaveVpk {
                chunk_path,
                json_data,
            } => match service::save_vpk(&chunk_path, &json_data) {
                Ok(_) => self.logger.log(&format!(
                    "[+] Voice Package (.debug-vpk) {:?} updated.",
                    chunk_path
                )),
                Err(e) => self
                    .logger
                    .log(&format!("[!] Voice Package save error: {}", e)),
            },
        }
    }

    fn refresh_project_state(&self, project_dir: &Path, status_msg: &'static str) {
        let (items, cached, haystacks) = scan_project_folder(project_dir);
        {
            let mut st = self.state.lock().unwrap();
            st.visible_indices = (0..items.len()).collect();
            st.all_cached_assets = cached;
            st.all_ui_items = items.clone();
            st.all_search_haystack = haystacks;
        }
        let ui_h = self.ui_handle.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_h.upgrade() {
                ui.set_asset_list(ModelRc::from(std::rc::Rc::new(VecModel::from(items))));
                ui.set_status_is_error(false);
                ui.set_status_msg(status_msg.into());
            }
        });
    }

    fn set_ui_status(&self, status_msg: &'static str, is_error: bool) {
        let ui_h = self.ui_handle.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_h.upgrade() {
                ui.set_status_is_error(is_error);
                ui.set_status_msg(status_msg.into());
            }
        });
    }

    fn set_ui_error(&self, err_msg: String) {
        let ui_h = self.ui_handle.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_h.upgrade() {
                ui.set_status_is_error(true);
                ui.set_status_msg(err_msg.into());
            }
        });
    }
}

fn resolve_diffuse_texture(project_dir: Option<&Path>) -> Option<ResolvedTexture> {
    let base_dir = project_dir?;
    let tex_dir = base_dir.join("assets").join("textures");
    if !tex_dir.exists() {
        return None;
    }

    let mut candidate_file: Option<PathBuf> = None;
    if let Ok(entries) = fs::read_dir(&tex_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().is_some_and(|e| e == "dds") {
                candidate_file = Some(p);
                break;
            }
        }
    }

    let file_path = candidate_file?;
    let dds_bytes = fs::read(&file_path).ok()?;
    let parsed_tex = crate::engine::assets::texture::parse_texture_chunk(&dds_bytes).ok()?;
    let rgba = dds_decoder::decode_to_rgba(
        parsed_tex.width,
        parsed_tex.height,
        parsed_tex.format,
        &parsed_tex.pixel_data,
    );

    let filename = file_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    Some(ResolvedTexture {
        filename,
        width: parsed_tex.width,
        height: parsed_tex.height,
        rgba,
    })
}
