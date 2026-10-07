use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use slint::{ModelRc, VecModel};

use crate::AppWindow;
use crate::gui::{AppState, resolve_project_dir, scan_project_folder};
use crate::utils::logger::UiLogger;

pub fn set_ui_busy(ui_handle: &slint::Weak<AppWindow>, busy: bool, progress: f32, label: &str) {
    let ui_h = ui_handle.clone();
    let lbl = label.to_string();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_is_busy(busy);
            ui.set_progress_value(progress);
            ui.set_progress_label(lbl.into());
        }
    });
}

pub fn set_ui_status(ui_handle: &slint::Weak<AppWindow>, status_msg: &'static str, is_error: bool) {
    let ui_h = ui_handle.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_is_busy(false);
            ui.set_status_is_error(is_error);
            ui.set_status_msg(status_msg.into());
        }
    });
}

pub fn set_ui_error(ui_handle: &slint::Weak<AppWindow>, err_msg: String) {
    let ui_h = ui_handle.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_is_busy(false);
            ui.set_status_is_error(true);
            ui.set_status_msg(err_msg.into());
        }
    });
}

pub fn refresh_project_state(
    ui_handle: &slint::Weak<AppWindow>,
    state: &Arc<Mutex<AppState>>,
    project_dir: &Path,
    status_msg: &'static str,
) {
    let (items, cached, haystacks) = scan_project_folder(project_dir);
    {
        let mut st = state.lock();
        st.visible_indices = (0..items.len()).collect();
        st.all_cached_assets = cached;
        st.all_ui_items = items.clone();
        st.all_search_haystack = haystacks;
    }
    let ui_h = ui_handle.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_is_busy(false);
            ui.set_asset_list(ModelRc::from(std::rc::Rc::new(VecModel::from(items))));
            ui.set_status_is_error(false);
            ui.set_status_msg(status_msg.into());
        }
    });
}

pub fn handle_unpack_archive(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    state: &Arc<Mutex<AppState>>,
    src: PathBuf,
    dst: PathBuf,
) {
    logger.log(&format!("[*] Unpacking archive: {:?}", src));
    set_ui_busy(ui_handle, true, 0.05, "Opening archive...");

    let ui_prog = ui_handle.clone();
    let progress_cb = move |val: f32, msg: &str| {
        set_ui_busy(&ui_prog, true, val, msg);
    };

    match crate::engine::container::project::unpack_archive_with_progress(
        &src,
        &dst,
        Some(&progress_cb),
    ) {
        Ok((count, info)) => {
            logger.log(&info);
            logger.log(&format!("[+] Unpack complete: {} chunks extracted.", count));
            let (items, cached, haystacks) = scan_project_folder(&dst);
            {
                let mut st = state.lock();
                st.visible_indices = (0..items.len()).collect();
                st.all_cached_assets = cached;
                st.all_ui_items = items.clone();
                st.all_search_haystack = haystacks;
                st.current_proj_dir = Some(dst.clone());
            }

            let ui_h = ui_handle.clone();
            let dst_str = dst.to_string_lossy().to_string();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_h.upgrade() {
                    ui.set_is_busy(false);
                    ui.set_active_project_dir(dst_str.into());
                    ui.set_asset_list(ModelRc::from(std::rc::Rc::new(VecModel::from(items))));
                    ui.set_status_is_error(false);
                    ui.set_status_msg(format!("Extracted {} items into workspace.", count).into());
                }
            });
        }
        Err(e) => {
            logger.log(&format!("[!] Unpack error: {}", e));
            set_ui_error(ui_handle, format!("Unpack Error: {}", e));
        }
    }
}

pub fn handle_load_project(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    state: &Arc<Mutex<AppState>>,
    proj_dir: PathBuf,
) {
    let actual_dir = resolve_project_dir(&proj_dir);
    logger.log(&format!("[*] Loading project from: {:?}", actual_dir));
    set_ui_busy(ui_handle, true, 0.2, "Scanning project resources...");

    if !actual_dir.exists()
        || (!actual_dir.join("chunks").exists()
            && !actual_dir.join("chunks_vanilla").exists()
            && !actual_dir.join("project.json").exists())
    {
        logger.log(&format!(
            "[!] Error: {:?} is not a valid project folder (missing project.json or chunks/)",
            actual_dir
        ));
        set_ui_error(
            ui_handle,
            "Error: Not a valid Overlord project folder!".into(),
        );
        return;
    }

    let (items, cached, haystacks) = scan_project_folder(&actual_dir);
    let total = items.len();
    {
        let mut st = state.lock();
        st.visible_indices = (0..items.len()).collect();
        st.all_cached_assets = cached;
        st.all_ui_items = items.clone();
        st.all_search_haystack = haystacks;
        st.current_proj_dir = Some(actual_dir.clone());
    }

    logger.log(&format!("[+] Project loaded: {} items found.", total));
    let ui_h = ui_handle.clone();
    let actual_dir_str = actual_dir.to_string_lossy().to_string();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_is_busy(false);
            ui.set_active_project_dir(actual_dir_str.into());
            ui.set_selected_index(-1);
            ui.set_active_file_path("".into());
            ui.set_asset_list(ModelRc::from(std::rc::Rc::new(VecModel::from(items))));
            ui.set_status_is_error(false);
            ui.set_status_msg(format!("Loaded {} resources into workspace.", total).into());
        }
    });
}

pub fn handle_clean_rebuild(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    state: &Arc<Mutex<AppState>>,
    proj_dir: PathBuf,
) {
    let actual_dir = resolve_project_dir(&proj_dir);
    logger.log(&format!(
        "[*] Performing clean rebuild from vanilla: {:?}",
        actual_dir
    ));
    set_ui_busy(ui_handle, true, 0.1, "Restoring baseline chunks...");

    let ui_prog = ui_handle.clone();
    let progress_cb = move |val: f32, msg: &str| {
        set_ui_busy(&ui_prog, true, val, msg);
    };

    match crate::engine::container::sync::clean_rebuild_project_with_progress(
        &actual_dir,
        Some(&progress_cb),
    ) {
        Ok(synced) => {
            logger.log(&format!(
                "[+] Clean rebuild completed: {} assets re-synced.",
                synced
            ));
            refresh_project_state(ui_handle, state, &actual_dir, "Clean rebuild successful!");
        }
        Err(e) => {
            logger.log(&format!("[!] Clean rebuild error: {}", e));
            set_ui_error(ui_handle, format!("Rebuild Error: {}", e));
        }
    }
}

pub fn handle_revert_asset(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    state: &Arc<Mutex<AppState>>,
    proj_dir: PathBuf,
    chunk_path: String,
) {
    let actual_dir = resolve_project_dir(&proj_dir);
    logger.log(&format!("[*] Reverting asset to vanilla: {:?}", chunk_path));
    match crate::engine::container::sync::revert_single_asset(&actual_dir, &chunk_path) {
        Ok(_) => {
            logger.log(&format!(
                "[+] Reverted {:?} to pristine vanilla state.",
                chunk_path
            ));
            refresh_project_state(
                ui_handle,
                state,
                &actual_dir,
                "Asset restored from vanilla baseline.",
            );
        }
        Err(e) => {
            logger.log(&format!("[!] Revert error: {}", e));
            set_ui_error(ui_handle, format!("Revert Error: {}", e));
        }
    }
}

pub fn handle_pack_archive(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    proj_dir: PathBuf,
) {
    let actual_dir = resolve_project_dir(&proj_dir);
    logger.log(&format!("[*] Packing project: {:?}", actual_dir));
    set_ui_busy(ui_handle, true, 0.05, "Preparing package...");

    let ui_prog = ui_handle.clone();
    let progress_cb = move |val: f32, msg: &str| {
        set_ui_busy(&ui_prog, true, val, msg);
    };

    let out = actual_dir.join("rebuilt.prp");
    match crate::engine::container::project::pack_archive_with_progress(
        &actual_dir,
        &out,
        0,
        Some(&progress_cb),
    ) {
        Ok(size) => {
            logger.log(&format!("[+] Pack complete: {:?} ({} bytes)", out, size));
            set_ui_status(
                ui_handle,
                "Archive successfully packed to rebuilt.prp!",
                false,
            );
        }
        Err(e) => {
            logger.log(&format!("[!] Pack error: {}", e));
            set_ui_error(ui_handle, format!("Pack Error: {}", e));
        }
    }
}

pub fn handle_create_patch(
    logger: &UiLogger,
    base_dir: PathBuf,
    mod_dir: PathBuf,
    out_file: PathBuf,
) {
    match crate::utils::diff::create_diff(&base_dir, &mod_dir, &out_file) {
        Ok(count) => logger.log(&format!(
            "[+] Mod patch created: {} modified items tracked.",
            count
        )),
        Err(e) => logger.log(&format!("[!] Patch creation error: {}", e)),
    }
}

pub fn handle_apply_patch(logger: &UiLogger, target_dir: PathBuf, patch_file: PathBuf) {
    match crate::utils::diff::apply_patch(&target_dir, &patch_file) {
        Ok(count) => logger.log(&format!("[+] Patch applied: {} files updated.", count)),
        Err(e) => logger.log(&format!("[!] Patch application error: {}", e)),
    }
}
