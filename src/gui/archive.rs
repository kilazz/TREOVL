use std::path::PathBuf;
use std::sync::mpsc::Sender;

use crate::AppWindow;
use crate::gui::commands::WorkerCommand;
use crate::utils::logger::UiLogger;

pub fn register(ui: &AppWindow, tx: Sender<WorkerCommand>, logger: UiLogger) {
    let tx_unpack = tx.clone();
    ui.on_unpack_prp(move |src_str, dst_str| {
        let _ = tx_unpack.send(WorkerCommand::UnpackArchive {
            src: PathBuf::from(src_str.as_str()),
            dst: PathBuf::from(dst_str.as_str()),
        });
    });

    let tx_load = tx.clone();
    ui.on_load_project(move |proj_str| {
        let _ = tx_load.send(WorkerCommand::LoadProject {
            proj_dir: PathBuf::from(proj_str.as_str()),
        });
    });

    let tx_pack = tx.clone();
    ui.on_pack_prp(move |proj_str| {
        let _ = tx_pack.send(WorkerCommand::PackArchive {
            proj_dir: PathBuf::from(proj_str.as_str()),
        });
    });

    let tx_create_patch = tx.clone();
    ui.on_create_patch(move |base_str, mod_str, out_str| {
        let _ = tx_create_patch.send(WorkerCommand::CreatePatch {
            base_dir: PathBuf::from(base_str.as_str()),
            mod_dir: PathBuf::from(mod_str.as_str()),
            out_file: PathBuf::from(out_str.as_str()),
        });
    });

    let tx_apply_patch = tx.clone();
    ui.on_apply_patch(move |target_str, patch_str| {
        let _ = tx_apply_patch.send(WorkerCommand::ApplyPatch {
            target_dir: PathBuf::from(target_str.as_str()),
            patch_file: PathBuf::from(patch_str.as_str()),
        });
    });

    ui.on_open_assets_folder(move |proj_str| {
        let proj = PathBuf::from(proj_str.as_str());
        let target_dir = if proj.join("assets").exists() {
            proj.join("assets")
        } else {
            proj
        };

        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("explorer")
            .arg(&target_dir)
            .spawn();
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&target_dir).spawn();
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open")
            .arg(&target_dir)
            .spawn();

        logger.log(&format!(
            "[*] Opened directory in system explorer: {:?}",
            target_dir
        ));
    });
}
