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
use crate::utils::logger::UiLogger;
use crate::{AppWindow, AssetItem};
use commands::WorkerCommand;

#[derive(Clone)]
pub struct CachedAsset {
    pub path: PathBuf,
    pub kind: AssetKind,
}

pub fn scan_project_folder(project_dir: &Path) -> (Vec<AssetItem>, Vec<CachedAsset>) {
    let chunks_dir = project_dir.join("chunks");
    let mut ui_items = Vec::new();
    let mut cached = Vec::new();

    if let Ok(entries) = fs::read_dir(chunks_dir) {
        let mut sorted_entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        sorted_entries.sort_by_key(|e| e.file_name());

        for entry in sorted_entries {
            let path = entry.path();
            if path.is_file() {
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
                    _ => 5,
                };

                ui_items.push(AssetItem {
                    display_name: sniffed.display_name.into(),
                    kind_name: sniffed.kind_name.into(),
                    icon: sniffed.icon.into(),
                    file_path: path.to_string_lossy().to_string().into(),
                    size_str: size_str.into(),
                    kind_id,
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
    let cached_assets: Arc<Mutex<Vec<CachedAsset>>> = Arc::new(Mutex::new(Vec::new()));

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
    let worker_cache = cached_assets.clone();

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
                            *worker_cache.lock().unwrap() = cached;

                            let ui_h = worker_ui_handle.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_h.upgrade() {
                                    ui.set_asset_list(ModelRc::from(std::rc::Rc::new(
                                        VecModel::from(items),
                                    )));
                                    ui.set_status_msg(
                                        format!("Extracted {} items into workspace.", count).into(),
                                    );
                                }
                            });
                        }
                        Err(e) => worker_logger.log(&format!("[!] Unpack error: {}", e)),
                    }
                }
                WorkerCommand::LoadProject { proj_dir } => {
                    let (items, cached) = scan_project_folder(&proj_dir);
                    let total = items.len();
                    *worker_cache.lock().unwrap() = cached;

                    worker_logger.log(&format!("[+] Project loaded: {} items found.", total));
                    let ui_h = worker_ui_handle.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_h.upgrade() {
                            ui.set_asset_list(ModelRc::from(std::rc::Rc::new(VecModel::from(
                                items,
                            ))));
                            ui.set_status_msg(
                                format!("Loaded {} resources into workspace.", total).into(),
                            );
                        }
                    });
                }
                WorkerCommand::PackArchive { proj_dir } => {
                    worker_logger.log(&format!("[*] Packing project: {:?}", proj_dir));
                    let out = proj_dir.join("rebuilt.prp");
                    match crate::engine::container::project::pack_archive(&proj_dir, &out) {
                        Ok(size) => {
                            worker_logger
                                .log(&format!("[+] Pack complete: {:?} ({} bytes)", out, size));
                            let ui_h = worker_ui_handle.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_h.upgrade() {
                                    ui.set_status_msg(
                                        "Archive successfully packed to rebuilt.prp!".into(),
                                    );
                                }
                            });
                        }
                        Err(e) => worker_logger.log(&format!("[!] Pack error: {}", e)),
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
                                    "[+] glTF exported to: {:?} ({} vertices, {} triangles)",
                                    out_path, stats.vertex_count, stats.triangle_count
                                ));
                            }
                            Err(e) => worker_logger.log(&format!("[!] glTF export error: {}", e)),
                        }
                    } else {
                        match crate::engine::assets::mesh::export_mesh_to_obj(&data) {
                            Ok((obj, stats)) => {
                                let _ = fs::write(&out_path, obj);
                                worker_logger.log(&format!(
                                    "[+] OBJ exported to: {:?} ({} vertices)",
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
                                    "[+] Mesh chunk {:?} rebuilt from .glb",
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
            }
        }
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
    assets::register(&ui, worker_tx, logger, cached_assets);

    ui.run()
}
