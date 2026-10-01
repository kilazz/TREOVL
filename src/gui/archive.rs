use slint::{ComponentHandle, ModelRc, VecModel};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::AppWindow;
use crate::engine::container::project::{pack_archive, unpack_archive};
use crate::gui::{CachedAsset, scan_project_folder};
use crate::utils::diff::{apply_patch, create_diff};
use crate::utils::logger::UiLogger;

pub fn register(ui: &AppWindow, logger: UiLogger, cache: Arc<Mutex<Vec<CachedAsset>>>) {
    // UNPACK PRP
    let ui_weak = ui.as_weak();
    let log = logger.clone();
    let cache_w = cache.clone();
    ui.on_unpack_prp(move |src_str, dst_str| {
        let src = PathBuf::from(src_str.as_str());
        let dst = PathBuf::from(dst_str.as_str());
        let ui_w = ui_weak.clone();
        let l = log.clone();
        let c_handle = cache_w.clone();

        std::thread::spawn(move || {
            l.log(&format!("[*] Unpacking archive: {:?}", src));
            match unpack_archive(&src, &dst) {
                Ok((count, info)) => {
                    l.log(&info);
                    l.log(&format!("[+] Unpack complete: {} chunks extracted.", count));

                    let (items, cached) = scan_project_folder(&dst);
                    *c_handle.lock().unwrap() = cached;

                    let _ = ui_w.upgrade_in_event_loop(move |ui| {
                        ui.set_asset_list(ModelRc::from(Rc::new(VecModel::from(items))));
                        ui.set_status_msg(
                            format!("Extracted {} items into workspace.", count).into(),
                        );
                    });
                }
                Err(e) => {
                    l.log(&format!("[!] Unpack error: {}", e));
                    let _ = ui_w.upgrade_in_event_loop(move |ui| {
                        ui.set_status_msg(format!("Unpack error: {}", e).into());
                    });
                }
            }
        });
    });

    // LOAD PROJECT
    let ui_weak = ui.as_weak();
    let log = logger.clone();
    let cache_w = cache;
    ui.on_load_project(move |proj_path_str| {
        let proj_path = PathBuf::from(proj_path_str.as_str());
        let proj_dir = proj_path.parent().unwrap_or(&proj_path);
        let ui_w = ui_weak.clone();
        let l = log.clone();
        let c_handle = cache_w.clone();

        let (items, cached) = scan_project_folder(proj_dir);
        let total = items.len();
        *c_handle.lock().unwrap() = cached;

        l.log(&format!("[+] Project loaded: {} items found.", total));
        let _ = ui_w.upgrade_in_event_loop(move |ui| {
            ui.set_asset_list(ModelRc::from(Rc::new(VecModel::from(items))));
            ui.set_status_msg(format!("Loaded {} resources into workspace.", total).into());
        });
    });

    // OPEN ASSETS FOLDER IN SYSTEM EXPLORER
    let log = logger.clone();
    ui.on_open_assets_folder(move |proj_str| {
        let proj = PathBuf::from(proj_str.as_str());
        let target_dir = if proj.join("assets").exists() {
            proj.join("assets")
        } else {
            proj
        };

        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("explorer")
                .arg(&target_dir)
                .spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(&target_dir).spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open")
                .arg(&target_dir)
                .spawn();
        }

        log.log(&format!(
            "[*] Opened directory in system explorer: {:?}",
            target_dir
        ));
    });

    // PACK PRP
    let ui_weak = ui.as_weak();
    let log = logger.clone();
    ui.on_pack_prp(move |proj_str| {
        let proj = PathBuf::from(proj_str.as_str());
        let out = proj.join("rebuilt.prp");
        let ui_w = ui_weak.clone();
        let l = log.clone();

        std::thread::spawn(move || {
            l.log(&format!(
                "[*] Synchronizing workspace and packing project: {:?}",
                proj
            ));
            match pack_archive(&proj, &out) {
                Ok(size) => {
                    l.log(&format!("[+] Pack complete: {:?} ({} bytes)", out, size));
                    let _ = ui_w.upgrade_in_event_loop(move |ui| {
                        ui.set_status_msg("Archive successfully packed to rebuilt.prp!".into());
                    });
                }
                Err(e) => {
                    l.log(&format!("[!] Pack error: {}", e));
                    let _ = ui_w.upgrade_in_event_loop(move |ui| {
                        ui.set_status_msg(format!("Pack error: {}", e).into());
                    });
                }
            }
        });
    });

    // CREATE MOD PATCH
    let log = logger.clone();
    ui.on_create_patch(move |base_str, mod_str, out_str| {
        let b = PathBuf::from(base_str.as_str());
        let m = PathBuf::from(mod_str.as_str());
        let o = PathBuf::from(out_str.as_str());
        let l = log.clone();

        std::thread::spawn(move || match create_diff(&b, &m, &o) {
            Ok(count) => l.log(&format!(
                "[+] Mod patch created: {} modified items tracked.",
                count
            )),
            Err(e) => l.log(&format!("[!] Patch creation error: {}", e)),
        });
    });

    // APPLY MOD PATCH
    let log = logger;
    ui.on_apply_patch(move |target_str, patch_str| {
        let t = PathBuf::from(target_str.as_str());
        let p = PathBuf::from(patch_str.as_str());
        let l = log.clone();

        std::thread::spawn(move || match apply_patch(&t, &p) {
            Ok(count) => l.log(&format!("[+] Patch applied: {} files updated.", count)),
            Err(e) => l.log(&format!("[!] Patch application error: {}", e)),
        });
    });
}
