pub mod archive;
pub mod assets;
pub mod commands;
pub mod textures;
pub mod worker;

use parking_lot::Mutex;
use slint::ComponentHandle;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

use crate::engine::assets::animation::{AnimationClip, ObjectBone};
use crate::engine::assets::sniffer::{AssetKind, sniff_asset};
use crate::engine::container::sync::{AssetSyncCache, calculate_crc32};
use crate::engine::math::{Vector2, Vector3, Vector4};
use crate::utils::logger::UiLogger;
use crate::utils::renderer::ViewportCamera;
use crate::{AppWindow, AssetItem};
use commands::WorkerCommand;
use worker::BackgroundWorker;

#[derive(Clone)]
pub struct CachedAsset {
    pub path: PathBuf,
    pub kind: AssetKind,
}

#[derive(Clone)]
pub struct RenderSubmesh {
    pub name: String,
    pub positions: Vec<Vector3>,
    pub normals: Vec<Vector3>,
    pub rest_positions: Vec<Vector3>,
    pub rest_normals: Vec<Vector3>,
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<Vector4>,
    pub bones: Vec<ObjectBone>,
    pub indices: Vec<u32>,
    pub uvs: Vec<Vector2>,
    pub texture: Option<Arc<(u32, u32, Vec<u8>)>>,
}

#[derive(Clone)]
pub struct ActiveMeshPreview {
    pub submeshes: Vec<RenderSubmesh>,
    pub is_composite: bool,
    pub composite_name: String,
    pub available_clips: Vec<AnimationClip>,
    pub current_clip_index: Option<usize>,
    pub current_time_seconds: f32,
    pub is_playing: bool,
    pub playback_speed: f32,
}

#[derive(Default)]
pub struct AppState {
    pub all_ui_items: Vec<AssetItem>,
    pub all_cached_assets: Vec<CachedAsset>,
    pub all_search_haystack: Vec<String>,
    pub visible_indices: Vec<usize>,
    pub current_proj_dir: Option<PathBuf>,
    pub camera: ViewportCamera,
    pub active_mesh: Option<ActiveMeshPreview>,
    pub filter_generation: u64,
    pub is_interacting: bool,
    pub is_skinning_enabled: bool,
    pub is_root_motion_enabled: bool,
    pub show_mesh: bool,
    pub show_skeleton: bool,
    pub show_xray: bool,
    pub show_bone_names: bool,
    pub show_wireframe: bool,
    pub show_grid: bool,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            is_skinning_enabled: true,
            is_root_motion_enabled: false,
            show_mesh: true,
            show_skeleton: true,
            show_xray: true,
            show_bone_names: false,
            show_wireframe: false,
            show_grid: true,
            is_interacting: false,
            ..Default::default()
        }
    }
}

pub fn resolve_project_dir(path: &Path) -> PathBuf {
    if path.is_file() {
        path.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| path.to_path_buf())
    } else {
        path.to_path_buf()
    }
}

pub fn scan_project_folder(project_dir: &Path) -> (Vec<AssetItem>, Vec<CachedAsset>, Vec<String>) {
    let chunks_dir = if project_dir.join("chunks").exists() {
        project_dir.join("chunks")
    } else {
        project_dir.join("chunks_vanilla")
    };

    let mut ui_items = Vec::new();
    let mut cached = Vec::new();
    let mut haystacks = Vec::new();

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
                let kind_id = sniffed.kind.to_ui_kind_id();

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

                let path_str = path.to_string_lossy().to_string();
                let search_token = format!(
                    "{} {} {}",
                    sniffed.display_name, sniffed.kind_name, path_str
                )
                .to_lowercase();

                ui_items.push(AssetItem {
                    display_name: sniffed.display_name.into(),
                    kind_name: sniffed.kind_name.into(),
                    icon: sniffed.icon.into(),
                    file_path: path_str.into(),
                    size_str: size_str.into(),
                    kind_id,
                    is_modified,
                });

                cached.push(CachedAsset {
                    path,
                    kind: sniffed.kind,
                });

                haystacks.push(search_token);
            }
        }
    }
    (ui_items, cached, haystacks)
}

pub fn run_gui() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let ui_weak = ui.as_weak();
    let app_state = Arc::new(Mutex::new(AppState::new()));

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

    let (worker_tx, worker_rx) = mpsc::channel::<WorkerCommand>();
    let worker = BackgroundWorker::new(ui_weak.clone(), logger.clone(), app_state.clone());
    thread::spawn(move || worker.run(worker_rx));

    let filter_tx = worker_tx.clone();
    static FILTER_GEN: AtomicU64 = AtomicU64::new(0);

    ui.on_filter_changed(move |query| {
        let generation_id = FILTER_GEN.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = filter_tx.send(WorkerCommand::FilterAssets {
            query: query.to_string(),
            generation: generation_id,
        });
    });

    ui.on_browse_file(|| {
        rfd::FileDialog::new()
            .pick_file()
            .map(|p| slint::SharedString::from(p.to_string_lossy().into_owned()))
            .unwrap_or_default()
    });
    ui.on_browse_folder(|| {
        rfd::FileDialog::new()
            .pick_folder()
            .map(|p| slint::SharedString::from(p.to_string_lossy().into_owned()))
            .unwrap_or_default()
    });
    ui.on_save_file(|| {
        rfd::FileDialog::new()
            .save_file()
            .map(|p| slint::SharedString::from(p.to_string_lossy().into_owned()))
            .unwrap_or_default()
    });

    archive::register(&ui, worker_tx.clone(), logger.clone());
    textures::register(&ui, worker_tx.clone(), logger.clone());
    assets::register(&ui, worker_tx, logger, app_state);

    ui.run()
}
