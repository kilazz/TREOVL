use slint::{Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc::Receiver};

use crate::AppWindow;
use crate::engine::assets::sniffer::AssetKind;
use crate::engine::service;
use crate::gui::commands::WorkerCommand;
use crate::gui::{ActiveMeshPreview, AppState, resolve_project_dir, scan_project_folder};
use crate::utils::logger::UiLogger;
use crate::utils::{dds_decoder, renderer};

pub struct BackgroundWorker {
    ui_handle: slint::Weak<AppWindow>,
    logger: UiLogger,
    state: Arc<Mutex<AppState>>,
}

impl BackgroundWorker {
    pub fn new(
        ui_handle: slint::Weak<AppWindow>,
        logger: UiLogger,
        state: Arc<Mutex<AppState>>,
    ) -> Self {
        Self {
            ui_handle,
            logger,
            state,
        }
    }

    pub fn run(mut self, rx: Receiver<WorkerCommand>) {
        while let Ok(cmd) = rx.recv() {
            self.handle_command(cmd);
        }
    }

    pub fn handle_command(&mut self, cmd: WorkerCommand) {
        match cmd {
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

                        if let Ok(parsed) =
                            crate::engine::assets::mesh::extract_mesh_geometry(&bytes)
                        {
                            mesh_info = format!(
                                "Vertices: {} | Triangles: {} | Skinned: {}",
                                parsed.positions.len(),
                                parsed.indices.len() / 3,
                                parsed.is_skinned
                            );

                            let cam = {
                                let mut st = self.state.lock().unwrap();
                                st.active_mesh = Some(ActiveMeshPreview {
                                    positions: parsed.positions.clone(),
                                    indices: parsed.indices.clone(),
                                    normals: parsed.normals.clone(),
                                });
                                st.camera
                            };

                            let buf = renderer::render_mesh_preview(
                                &parsed.positions,
                                &parsed.indices,
                                &parsed.normals,
                                512,
                                512,
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
                    AssetKind::Object => {
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
                        renderer::render_mesh_preview(
                            &mesh.positions,
                            &mesh.indices,
                            &mesh.normals,
                            512,
                            512,
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

            WorkerCommand::UnpackArchive { src, dst } => {
                self.logger
                    .log(&format!("[*] Unpacking archive: {:?}", src));
                match crate::engine::container::project::unpack_archive(&src, &dst) {
                    Ok((count, info)) => {
                        self.logger.log(&info);
                        self.logger
                            .log(&format!("[+] Unpack complete: {} chunks extracted.", count));
                        let (items, cached) = scan_project_folder(&dst);
                        {
                            let mut st = self.state.lock().unwrap();
                            st.visible_indices = (0..items.len()).collect();
                            st.all_cached_assets = cached;
                            st.all_ui_items = items.clone();
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
                        let ui_h = self.ui_handle.clone();
                        let err_msg = format!("Unpack Error: {}", e);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_status_is_error(true);
                                ui.set_status_msg(err_msg.into());
                            }
                        });
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
                    let ui_h = self.ui_handle.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_h.upgrade() {
                            ui.set_status_is_error(true);
                            ui.set_status_msg("Error: Not a valid Overlord project folder!".into());
                        }
                    });
                    return;
                }

                let (items, cached) = scan_project_folder(&actual_dir);
                let total = items.len();
                {
                    let mut st = self.state.lock().unwrap();
                    st.visible_indices = (0..items.len()).collect();
                    st.all_cached_assets = cached;
                    st.all_ui_items = items.clone();
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
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_status_is_error(false);
                                ui.set_status_msg(
                                    "Archive successfully packed to rebuilt.prp!".into(),
                                );
                            }
                        });
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
        }
    }

    fn refresh_project_state(&self, project_dir: &Path, status_msg: &'static str) {
        let (items, cached) = scan_project_folder(project_dir);
        {
            let mut st = self.state.lock().unwrap();
            st.visible_indices = (0..items.len()).collect();
            st.all_cached_assets = cached;
            st.all_ui_items = items.clone();
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
