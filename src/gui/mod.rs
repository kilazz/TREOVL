pub mod archive;
pub mod assets;
pub mod textures;

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use slint::{ComponentHandle, SharedString};

use crate::engine::assets::sniffer::{AssetKind, sniff_asset};
use crate::utils::logger::UiLogger;
use crate::{AppWindow, AssetItem};

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

    let (log_tx, log_rx) = mpsc::channel::<String>();
    let logger = UiLogger::new(log_tx);

    let ui_log = ui_weak.clone();
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
            let _ = ui_log.upgrade_in_event_loop(move |ui| {
                ui.set_log_text(combined.into());
            });
            thread::sleep(std::time::Duration::from_millis(50));
        }
    });

    // File Dialogs
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

    archive::register(&ui, logger.clone(), cached_assets.clone());
    textures::register(&ui, logger.clone());
    assets::register(&ui, logger, cached_assets);

    ui.run()
}
