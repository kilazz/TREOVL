pub mod archive;
pub mod assets;
pub mod commands;
pub mod textures;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use crate::engine::assets::sniffer::{AssetKind, sniff_asset};
use crate::engine::container::sync::{AssetSyncCache, calculate_crc32};
use crate::utils::logger::UiLogger;
use crate::{AppWindow, AssetItem};
use commands::WorkerCommand;

#[derive(Clone)]
pub struct CachedAsset {
    pub path: PathBuf,
    pub kind: AssetKind,
}

#[derive(Default)]
pub struct AppState {
    pub all_ui_items: Vec<AssetItem>,
    pub all_cached_assets: Vec<CachedAsset>,
    pub visible_indices: Vec<usize>,
    pub current_proj_dir: Option<PathBuf>,
}

pub fn scan_project_folder(project_dir: &Path) -> (Vec<AssetItem>, Vec<CachedAsset>) {
    let chunks_dir = if project_dir.join("chunks").exists() {
        project_dir.join("chunks")
    } else {
        project_dir.join("chunks_vanilla")
    };

    let mut ui_items = Vec::new();
    let mut cached = Vec::new();

    let cache_map: std::collections::HashMap<String, bool> = {
        let cache_file = project_dir.join(".asset_cache.json");
        if let Ok(content) = fs::read_to_string(cache_file)
            && let Ok(cache) = serde_json::from_str::<AssetSyncCache>(&content)
        {
            cache
                .entries
                .values()
                .map(|e| (e.chunk_rel_path.clone(), e.is_modified))
                .collect()
        } else {
            Default::default()
        }
    };

    let vanilla_dir = project_dir.join("chunks_vanilla");

    if let Ok(entries) = fs::read_dir(chunks_dir) {
        let mut sorted_entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        sorted_entries.sort_by_key(|e| e.file_name());

        for entry in sorted_entries {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "bin") {
                let filename = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let bytes = fs::read(&path).unwrap_or_default();
                let size_str = format!("{:.1} KB", bytes.len() as f64 / 1024.0);
                let sniffed = sniff_asset(&bytes, &filename);

                let kind_id = match sniffed.kind {
                    AssetKind::Texture => 0,
                    AssetKind::Audio => 1,
                    AssetKind::Material => 2,
                    AssetKind::Mesh => 3,
                    AssetKind::Lua => 4,
                    AssetKind::UI => 6,
                    AssetKind::Object => 7,
                    AssetKind::Animation => 8,
                    AssetKind::TerrainPalette => 9,
                    _ => 5,
                };

                let rel_key = format!("chunks/{}", filename);
                let is_modified = cache_map.get(&rel_key).copied().unwrap_or_else(|| {
                    let vanilla_file = vanilla_dir.join(&filename);
                    if vanilla_file.exists()
                        && let Ok(v_bytes) = fs::read(vanilla_file)
                    {
                        calculate_crc32(&v_bytes) != calculate_crc32(&bytes)
                    } else {
                        false
                    }
                });

                ui_items.push(AssetItem {
                    display_name: sniffed.display_name.into(),
                    kind_name: sniffed.kind_name.into(),
                    icon: sniffed.icon.into(),
                    file_path: path.to_string_lossy().to_string().into(),
                    size_str: size_str.into(),
                    kind_id,
                    is_modified,
                });

                cached.push(CachedAsset {
                    path,
                    kind: sniffed.kind,
                });
            }
        }
    }
    (ui_items, cached)
}

pub fn run_gui() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let ui_weak = ui.as_weak();
    let app_state = Arc::new(Mutex::new(AppState::default()));

    // 1. Setup Logger Channel
    let (log_tx, log_rx) = mpsc::channel::<String>();
    let logger = UiLogger::new(log_tx);

    let ui_log_handle = ui_weak.clone();
    thread::spawn(move || {
        let mut logs = VecDeque::with_capacity(300);
        while let Ok(msg) = log_rx.recv() {
            logs.push_back(msg);
            while let Ok(m) = log_rx.try_recv() {
                logs.push_back(m);
            }
            while logs.len() > 250 {
                logs.pop_front();
            }

            let combined = logs.iter().cloned().collect::<String>();
            let ui_h = ui_log_handle.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_h.upgrade() {
                    ui.set_log_text(combined.into());
                }
            });
        }
    });

    // 2. Setup Background Worker Channel
    let (worker_tx, worker_rx) = mpsc::channel::<WorkerCommand>();

    let worker_ui_handle = ui_weak.clone();
    let worker_logger = logger.clone();
    let worker_state = app_state.clone();

    thread::spawn(move || {
        while let Ok(cmd) = worker_rx.recv() {
            match cmd {
                WorkerCommand::UnpackArchive { src, dst } => {
                    worker_logger.log(&format!("[*] Unpacking archive: {:?}", src));
                    match crate::engine::container::project::unpack_archive(&src, &dst) {
                        Ok((count, info)) => {
                            worker_logger.log(&info);
                            worker_logger
                                .log(&format!("[+] Unpack complete: {} chunks extracted.", count));
                            let (items, cached) = scan_project_folder(&dst);
                            {
                                let mut st = worker_state.lock().unwrap();
                                st.visible_indices = (0..items.len()).collect();
                                st.all_cached_assets = cached;
                                st.all_ui_items = items.clone();
                                st.current_proj_dir = Some(dst.clone());
                            }

                            let ui_h = worker_ui_handle.clone();
                            let dst_str = dst.to_string_lossy().to_string();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_h.upgrade() {
                                    ui.set_active_project_dir(dst_str.into());
                                    ui.set_asset_list(ModelRc::from(std::rc::Rc::new(
                                        VecModel::from(items),
                                    )));
                                    ui.set_status_is_error(false);
                                    ui.set_status_msg(
                                        format!("Extracted {} items into workspace.", count).into(),
                                    );
                                }
                            });
                        }
                        Err(e) => {
                            worker_logger.log(&format!("[!] Unpack error: {}", e));
                            let ui_h = worker_ui_handle.clone();
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
                    let actual_dir = if proj_dir.is_file() {
                        proj_dir
                            .parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or(proj_dir)
                    } else {
                        proj_dir
                    };

                    worker_logger.log(&format!("[*] Loading project from: {:?}", actual_dir));

                    if !actual_dir.exists()
                        || (!actual_dir.join("chunks").exists()
                            && !actual_dir.join("chunks_vanilla").exists()
                            && !actual_dir.join("project.json").exists())
                    {
                        worker_logger.log(&format!(
                            "[!] Error: {:?} is not a valid project folder (missing project.json or chunks/)",
                            actual_dir
                        ));
                        let ui_h = worker_ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_status_is_error(true);
                                ui.set_status_msg(
                                    "Error: Not a valid Overlord project folder!".into(),
                                );
                            }
                        });
                        continue;
                    }

                    let (items, cached) = scan_project_folder(&actual_dir);
                    let total = items.len();
                    {
                        let mut st = worker_state.lock().unwrap();
                        st.visible_indices = (0..items.len()).collect();
                        st.all_cached_assets = cached;
                        st.all_ui_items = items.clone();
                        st.current_proj_dir = Some(actual_dir.clone());
                    }

                    worker_logger.log(&format!("[+] Project loaded: {} items found.", total));
                    let ui_h = worker_ui_handle.clone();
                    let actual_dir_str = actual_dir.to_string_lossy().to_string();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_h.upgrade() {
                            ui.set_active_project_dir(actual_dir_str.into());
                            ui.set_selected_index(-1);
                            ui.set_active_file_path("".into());
                            ui.set_asset_list(ModelRc::from(std::rc::Rc::new(VecModel::from(
                                items,
                            ))));
                            ui.set_status_is_error(false);
                            ui.set_status_msg(
                                format!("Loaded {} resources into workspace.", total).into(),
                            );
                        }
                    });
                }
                WorkerCommand::CleanRebuild { proj_dir } => {
                    let actual_dir = if proj_dir.is_file() {
                        proj_dir
                            .parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or(proj_dir)
                    } else {
                        proj_dir
                    };

                    worker_logger.log(&format!(
                        "[*] Performing clean rebuild from vanilla: {:?}",
                        actual_dir
                    ));
                    match crate::engine::container::sync::clean_rebuild_project(&actual_dir) {
                        Ok(synced) => {
                            worker_logger.log(&format!(
                                "[+] Clean rebuild completed: {} assets re-synced.",
                                synced
                            ));
                            let (items, cached) = scan_project_folder(&actual_dir);
                            {
                                let mut st = worker_state.lock().unwrap();
                                st.visible_indices = (0..items.len()).collect();
                                st.all_cached_assets = cached;
                                st.all_ui_items = items.clone();
                            }
                            let ui_h = worker_ui_handle.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_h.upgrade() {
                                    ui.set_asset_list(ModelRc::from(std::rc::Rc::new(
                                        VecModel::from(items),
                                    )));
                                    ui.set_status_is_error(false);
                                    ui.set_status_msg("Clean rebuild successful!".into());
                                }
                            });
                        }
                        Err(e) => {
                            worker_logger.log(&format!("[!] Clean rebuild error: {}", e));
                            let ui_h = worker_ui_handle.clone();
                            let err_msg = format!("Rebuild Error: {}", e);
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_h.upgrade() {
                                    ui.set_status_is_error(true);
                                    ui.set_status_msg(err_msg.into());
                                }
                            });
                        }
                    }
                }
                WorkerCommand::RevertAsset {
                    proj_dir,
                    chunk_path,
                } => {
                    let actual_dir = if proj_dir.is_file() {
                        proj_dir
                            .parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or(proj_dir)
                    } else {
                        proj_dir
                    };

                    worker_logger.log(&format!("[*] Reverting asset to vanilla: {:?}", chunk_path));
                    match crate::engine::container::sync::revert_single_asset(
                        &actual_dir,
                        &chunk_path,
                    ) {
                        Ok(_) => {
                            worker_logger.log(&format!(
                                "[+] Reverted {:?} to pristine vanilla state.",
                                chunk_path
                            ));
                            let (items, cached) = scan_project_folder(&actual_dir);
                            {
                                let mut st = worker_state.lock().unwrap();
                                st.visible_indices = (0..items.len()).collect();
                                st.all_cached_assets = cached;
                                st.all_ui_items = items.clone();
                            }
                            let ui_h = worker_ui_handle.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_h.upgrade() {
                                    ui.set_asset_list(ModelRc::from(std::rc::Rc::new(
                                        VecModel::from(items),
                                    )));
                                    ui.set_status_is_error(false);
                                    ui.set_status_msg(
                                        "Asset restored from vanilla baseline.".into(),
                                    );
                                }
                            });
                        }
                        Err(e) => {
                            worker_logger.log(&format!("[!] Revert error: {}", e));
                            let ui_h = worker_ui_handle.clone();
                            let err_msg = format!("Revert Error: {}", e);
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_h.upgrade() {
                                    ui.set_status_is_error(true);
                                    ui.set_status_msg(err_msg.into());
                                }
                            });
                        }
                    }
                }
                WorkerCommand::PackArchive { proj_dir } => {
                    let actual_dir = if proj_dir.is_file() {
                        proj_dir
                            .parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or(proj_dir)
                    } else {
                        proj_dir
                    };

                    worker_logger.log(&format!("[*] Packing project: {:?}", actual_dir));
                    let out = actual_dir.join("rebuilt.prp");
                    match crate::engine::container::project::pack_archive(&actual_dir, &out) {
                        Ok(size) => {
                            worker_logger
                                .log(&format!("[+] Pack complete: {:?} ({} bytes)", out, size));
                            let ui_h = worker_ui_handle.clone();
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
                            worker_logger.log(&format!("[!] Pack error: {}", e));
                            let ui_h = worker_ui_handle.clone();
                            let err_msg = format!("Pack Error: {}", e);
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_h.upgrade() {
                                    ui.set_status_is_error(true);
                                    ui.set_status_msg(err_msg.into());
                                }
                            });
                        }
                    }
                }
                WorkerCommand::CreatePatch {
                    base_dir,
                    mod_dir,
                    out_file,
                } => match crate::utils::diff::create_diff(&base_dir, &mod_dir, &out_file) {
                    Ok(count) => worker_logger.log(&format!(
                        "[+] Mod patch created: {} modified items tracked.",
                        count
                    )),
                    Err(e) => worker_logger.log(&format!("[!] Patch creation error: {}", e)),
                },
                WorkerCommand::ApplyPatch {
                    target_dir,
                    patch_file,
                } => match crate::utils::diff::apply_patch(&target_dir, &patch_file) {
                    Ok(count) => {
                        worker_logger.log(&format!("[+] Patch applied: {} files updated.", count))
                    }
                    Err(e) => worker_logger.log(&format!("[!] Patch application error: {}", e)),
                },
                WorkerCommand::ExportDds {
                    chunk_path,
                    out_path,
                } => {
                    let data = fs::read(&chunk_path).unwrap_or_default();
                    match crate::engine::assets::texture::export_to_dds(&data) {
                        Ok(dds) => {
                            let _ = fs::write(&out_path, dds);
                            worker_logger.log(&format!("[+] Texture exported to: {:?}", out_path));
                        }
                        Err(e) => worker_logger.log(&format!("[!] DDS export error: {}", e)),
                    }
                }
                WorkerCommand::ImportDds {
                    chunk_path,
                    in_path,
                } => {
                    let chunk_data = fs::read(&chunk_path).unwrap_or_default();
                    let dds_data = fs::read(&in_path).unwrap_or_default();
                    match crate::engine::assets::texture::replace_texture_in_chunk(
                        &chunk_data,
                        &dds_data,
                    ) {
                        Ok(new_chunk) => {
                            let _ = fs::write(&chunk_path, new_chunk);
                            worker_logger
                                .log(&format!("[+] Chunk {:?} updated with new DDS.", chunk_path));
                        }
                        Err(e) => worker_logger.log(&format!("[!] DDS replacement error: {}", e)),
                    }
                }
                WorkerCommand::ExportWav {
                    chunk_path,
                    out_path,
                } => {
                    let data = fs::read(&chunk_path).unwrap_or_default();
                    match crate::engine::assets::audio::export_wav(&data) {
                        Ok(wav) => {
                            let _ = fs::write(&out_path, wav);
                            worker_logger.log(&format!("[+] Audio exported to: {:?}", out_path));
                        }
                        Err(e) => worker_logger.log(&format!("[!] Audio export error: {}", e)),
                    }
                }
                WorkerCommand::ImportWav {
                    chunk_path,
                    in_path,
                } => {
                    let chunk_data = fs::read(&chunk_path).unwrap_or_default();
                    let wav_data = fs::read(&in_path).unwrap_or_default();
                    match crate::engine::assets::audio::replace_wav(&chunk_data, &wav_data) {
                        Ok(new_chunk) => {
                            let _ = fs::write(&chunk_path, new_chunk);
                            worker_logger
                                .log(&format!("[+] Audio chunk {:?} updated.", chunk_path));
                        }
                        Err(e) => worker_logger.log(&format!("[!] Audio import error: {}", e)),
                    }
                }
                WorkerCommand::ExportMesh {
                    chunk_path,
                    out_path,
                    is_glb,
                } => {
                    let data = fs::read(&chunk_path).unwrap_or_default();
                    if is_glb {
                        match crate::engine::assets::mesh::export_mesh_to_glb(&data) {
                            Ok((glb, stats)) => {
                                let _ = fs::write(&out_path, glb);
                                worker_logger.log(&format!(
                                    "[+] glTF exported: {:?} ({} verts, {} tris, skinned: {})",
                                    out_path,
                                    stats.vertex_count,
                                    stats.triangle_count,
                                    stats.is_skinned
                                ));
                            }
                            Err(e) => worker_logger.log(&format!("[!] glTF export error: {}", e)),
                        }
                    } else {
                        match crate::engine::assets::mesh::export_mesh_to_obj(&data) {
                            Ok((obj, stats)) => {
                                let _ = fs::write(&out_path, obj);
                                worker_logger.log(&format!(
                                    "[+] OBJ exported: {:?} ({} vertices)",
                                    out_path, stats.vertex_count
                                ));
                            }
                            Err(e) => worker_logger.log(&format!("[!] Mesh export error: {}", e)),
                        }
                    }
                }
                WorkerCommand::ImportMesh {
                    chunk_path,
                    in_path,
                    is_glb,
                } => {
                    let chunk_data = fs::read(&chunk_path).unwrap_or_default();
                    if is_glb {
                        let glb_bytes = fs::read(&in_path).unwrap_or_default();
                        match crate::engine::assets::mesh::import_glb_to_mesh(
                            &chunk_data,
                            &glb_bytes,
                        ) {
                            Ok(bin) => {
                                let _ = fs::write(&chunk_path, bin);
                                worker_logger.log(&format!(
                                    "[+] Mesh chunk {:?} rebuilt from .glb (Skinning preserved)",
                                    chunk_path
                                ));
                            }
                            Err(e) => worker_logger.log(&format!("[!] glTF import error: {}", e)),
                        }
                    } else {
                        let obj_text = fs::read_to_string(&in_path).unwrap_or_default();
                        match crate::engine::assets::mesh::import_obj_to_mesh(
                            &chunk_data,
                            &obj_text,
                        ) {
                            Ok(bin) => {
                                let _ = fs::write(&chunk_path, bin);
                                worker_logger.log(&format!(
                                    "[+] Mesh chunk {:?} rebuilt from OBJ",
                                    chunk_path
                                ));
                            }
                            Err(e) => worker_logger.log(&format!("[!] OBJ import error: {}", e)),
                        }
                    }
                }
                WorkerCommand::ExportTerrain {
                    chunk_path,
                    out_path,
                    is_glb,
                } => {
                    let data = fs::read(&chunk_path).unwrap_or_default();
                    if is_glb {
                        match crate::engine::assets::terrain::export_terrain_to_glb(&data) {
                            Ok((glb, v_count, tri_count)) => {
                                let _ = fs::write(&out_path, glb);
                                worker_logger.log(&format!(
                                    "[+] Terrain glTF exported: {:?} ({} vertices, {} triangles)",
                                    out_path, v_count, tri_count
                                ));
                            }
                            Err(e) => {
                                worker_logger.log(&format!("[!] Terrain export error: {}", e))
                            }
                        }
                    } else {
                        match crate::engine::assets::terrain::export_terrain_to_obj(&data) {
                            Ok(obj) => {
                                let _ = fs::write(&out_path, obj);
                                worker_logger
                                    .log(&format!("[+] Terrain OBJ exported: {:?}", out_path));
                            }
                            Err(e) => {
                                worker_logger.log(&format!("[!] Terrain export error: {}", e))
                            }
                        }
                    }
                }
                WorkerCommand::ExportLua {
                    chunk_path,
                    out_path,
                } => {
                    let data = fs::read(&chunk_path).unwrap_or_default();
                    match crate::engine::assets::lua::extract_lua_bytecode(&data) {
                        Ok(bytecode) => {
                            let _ = fs::write(&out_path, bytecode);
                            worker_logger
                                .log(&format!("[+] Lua bytecode exported: {:?}", out_path));
                        }
                        Err(e) => worker_logger.log(&format!("[!] Lua export error: {}", e)),
                    }
                }
                WorkerCommand::ImportLua {
                    chunk_path,
                    in_path,
                } => {
                    let chunk_data = fs::read(&chunk_path).unwrap_or_default();
                    let luac_data = fs::read(&in_path).unwrap_or_default();
                    match crate::engine::assets::lua::replace_lua_bytecode(&chunk_data, &luac_data)
                    {
                        Ok(new_chunk) => {
                            let _ = fs::write(&chunk_path, new_chunk);
                            worker_logger.log(&format!("[+] Lua chunk {:?} updated.", chunk_path));
                        }
                        Err(e) => worker_logger.log(&format!("[!] Lua import error: {}", e)),
                    }
                }
                WorkerCommand::ExportAnimGlb {
                    chunk_path,
                    out_path,
                } => {
                    let data = fs::read(&chunk_path).unwrap_or_default();
                    match crate::engine::assets::animation::export_animation_to_glb(&data) {
                        Ok(glb) => {
                            let _ = fs::write(&out_path, glb);
                            worker_logger.log(&format!(
                                "[+] Animation timeline exported to glTF: {:?}",
                                out_path
                            ));
                        }
                        Err(e) => {
                            worker_logger.log(&format!("[!] Animation GLB export error: {}", e))
                        }
                    }
                }
                WorkerCommand::ExportAnimJson {
                    chunk_path,
                    out_path,
                } => {
                    let data = fs::read(&chunk_path).unwrap_or_default();
                    match crate::engine::assets::animation::export_animation_to_json(&data) {
                        Ok(json_str) => {
                            let _ = fs::write(&out_path, json_str);
                            worker_logger.log(&format!(
                                "[+] Animation keyframes exported to JSON: {:?}",
                                out_path
                            ));
                        }
                        Err(e) => {
                            worker_logger.log(&format!("[!] Animation JSON export error: {}", e))
                        }
                    }
                }
                WorkerCommand::SaveMaterial {
                    chunk_path,
                    json_data,
                } => match crate::engine::assets::material::import_material_from_json(&json_data) {
                    Ok(bin) => {
                        let _ = fs::write(&chunk_path, bin);
                        worker_logger.log(&format!("[+] Material chunk {:?} updated.", chunk_path));
                    }
                    Err(e) => worker_logger.log(&format!("[!] Material save error: {}", e)),
                },
                WorkerCommand::SaveUI {
                    chunk_path,
                    json_data,
                } => match crate::engine::assets::ui::import_ui_from_json(&json_data) {
                    Ok(bin) => {
                        let _ = fs::write(&chunk_path, bin);
                        worker_logger
                            .log(&format!("[+] UI layout chunk {:?} updated.", chunk_path));
                    }
                    Err(e) => worker_logger.log(&format!("[!] UI save error: {}", e)),
                },
                WorkerCommand::SaveObject {
                    chunk_path,
                    json_data,
                } => match crate::engine::assets::object::import_object_from_json(&json_data) {
                    Ok(bin) => {
                        let _ = fs::write(&chunk_path, bin);
                        worker_logger.log(&format!(
                            "[+] 3D Object entity chunk {:?} updated.",
                            chunk_path
                        ));
                    }
                    Err(e) => worker_logger.log(&format!("[!] Object save error: {}", e)),
                },
                WorkerCommand::SaveTerrainPalette {
                    chunk_path,
                    json_data,
                } => {
                    match crate::engine::assets::terrain_palette::import_terrain_palette_from_json(
                        &json_data,
                    ) {
                        Ok(bin) => {
                            let _ = fs::write(&chunk_path, bin);
                            worker_logger.log(&format!(
                                "[+] Terrain Palette chunk {:?} updated.",
                                chunk_path
                            ));
                        }
                        Err(e) => {
                            worker_logger.log(&format!("[!] Terrain Palette save error: {}", e))
                        }
                    }
                }
            }
        }
    });

    let filter_ui_handle = ui_weak.clone();
    let filter_state = app_state.clone();
    ui.on_filter_changed(move |query| {
        let q = query.trim().to_lowercase();
        let mut st = filter_state.lock().unwrap();
        let mut new_visible = Vec::new();
        let mut filtered_ui = Vec::new();

        for (i, item) in st.all_ui_items.iter().enumerate() {
            if q.is_empty()
                || item.display_name.to_lowercase().contains(&q)
                || item.kind_name.to_lowercase().contains(&q)
                || item.file_path.to_lowercase().contains(&q)
            {
                new_visible.push(i);
                filtered_ui.push(item.clone());
            }
        }

        st.visible_indices = new_visible;

        let ui_h = filter_ui_handle.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_h.upgrade() {
                ui.set_asset_list(ModelRc::from(std::rc::Rc::new(VecModel::from(filtered_ui))));
                ui.set_selected_index(-1);
            }
        });
    });

    ui.on_browse_file(|| {
        rfd::FileDialog::new()
            .pick_file()
            .map(|p| SharedString::from(p.to_string_lossy().into_owned()))
            .unwrap_or_default()
    });
    ui.on_browse_folder(|| {
        rfd::FileDialog::new()
            .pick_folder()
            .map(|p| SharedString::from(p.to_string_lossy().into_owned()))
            .unwrap_or_default()
    });
    ui.on_save_file(|| {
        rfd::FileDialog::new()
            .save_file()
            .map(|p| SharedString::from(p.to_string_lossy().into_owned()))
            .unwrap_or_default()
    });

    archive::register(&ui, worker_tx.clone(), logger.clone());
    textures::register(&ui, worker_tx.clone(), logger.clone());
    assets::register(&ui, worker_tx, logger, app_state);

    ui.run()
}
