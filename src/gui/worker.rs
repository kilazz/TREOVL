use slint::{ModelRc, VecModel};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc::Receiver};

use crate::AppWindow;
use crate::engine::service;
use crate::gui::commands::WorkerCommand;
use crate::gui::{AppState, scan_project_folder};
use crate::utils::logger::UiLogger;

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
                let actual_dir = self.resolve_project_dir(proj_dir);
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
                let actual_dir = self.resolve_project_dir(proj_dir);
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
                let actual_dir = self.resolve_project_dir(proj_dir);
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
                let actual_dir = self.resolve_project_dir(proj_dir);
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
                        self.logger
                            .log(&format!("[+] Terrain OBJ exported: {:?}", out_path));
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

    fn resolve_project_dir(&self, path: PathBuf) -> PathBuf {
        if path.is_file() {
            path.parent().map(|p| p.to_path_buf()).unwrap_or(path)
        } else {
            path
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
