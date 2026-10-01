use std::path::PathBuf;
use std::sync::mpsc::Sender;

use crate::AppWindow;
use crate::gui::commands::WorkerCommand;
use crate::utils::logger::UiLogger;

pub fn register(ui: &AppWindow, tx: Sender<WorkerCommand>, _logger: UiLogger) {
    let tx_exp = tx.clone();
    ui.on_export_dds(move |chunk_str, out_str| {
        let _ = tx_exp.send(WorkerCommand::ExportDds {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            out_path: PathBuf::from(out_str.as_str()),
        });
    });

    let tx_imp = tx;
    ui.on_import_dds(move |chunk_str, dds_str| {
        let _ = tx_imp.send(WorkerCommand::ImportDds {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            in_path: PathBuf::from(dds_str.as_str()),
        });
    });
}
