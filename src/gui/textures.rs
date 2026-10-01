use crate::AppWindow;
use crate::engine::assets::texture::{export_to_dds, replace_texture_in_chunk};
use crate::utils::logger::UiLogger;
use slint::ComponentHandle;
use std::fs;

pub fn register(ui: &AppWindow, logger: UiLogger) {
    let log = logger.clone();
    ui.on_export_dds(move |chunk_str, out_str| {
        let data = fs::read(chunk_str.as_str()).unwrap_or_default();
        match export_to_dds(&data) {
            Ok(dds) => {
                let _ = fs::write(out_str.as_str(), dds);
                log.log(&format!("[+] Texture exported to DDS: {}", out_str));
            }
            Err(e) => log.log(&format!("[!] DDS export error: {}", e)),
        }
    });

    let ui_weak = ui.as_weak();
    let log = logger;
    ui.on_import_dds(move |chunk_str, dds_str| {
        let chunk_data = fs::read(chunk_str.as_str()).unwrap_or_default();
        let dds_data = fs::read(dds_str.as_str()).unwrap_or_default();

        match replace_texture_in_chunk(&chunk_data, &dds_data) {
            Ok(new_chunk) => {
                let _ = fs::write(chunk_str.as_str(), new_chunk);
                log.log(&format!("[+] Chunk {} updated with new DDS.", chunk_str));
                let _ = ui_weak.upgrade_in_event_loop(move |ui| {
                    ui.invoke_select_asset(ui.get_selected_index());
                });
            }
            Err(e) => log.log(&format!("[!] DDS replacement error: {}", e)),
        }
    });
}
