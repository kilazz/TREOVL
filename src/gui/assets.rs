use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc::Sender};

use crate::AppWindow;
use crate::gui::commands::WorkerCommand;
use crate::gui::{AppState, resolve_project_dir};
use crate::utils::logger::UiLogger;

pub fn register(
    ui: &AppWindow,
    tx: Sender<WorkerCommand>,
    _logger: UiLogger,
    state: Arc<Mutex<AppState>>,
) {
    let tx_select = tx.clone();
    let state_sel = state.clone();

    // Fast non-blocking selection with bounds check to prevent negative index wrap-around
    ui.on_select_asset(move |filtered_index| {
        if filtered_index < 0 {
            return;
        }

        let st = state_sel.lock().unwrap();
        if let Some(&real_index) = st.visible_indices.get(filtered_index as usize)
            && let Some(target) = st.all_cached_assets.get(real_index)
        {
            let path = target.path.clone();
            let kind = target.kind;
            let _ = tx_select.send(WorkerCommand::SelectAsset {
                filtered_index,
                path,
                kind,
            });
        }
    });

    // Interactive 3D viewport orbit dispatched to worker thread
    let tx_rotate = tx.clone();
    ui.on_rotate_mesh_viewport(move |delta_yaw, delta_pitch| {
        let _ = tx_rotate.send(WorkerCommand::RotateMeshViewport {
            delta_yaw,
            delta_pitch,
        });
    });

    let tx_revert = tx.clone();
    let state_revert = state;
    ui.on_revert_asset(move |chunk_str| {
        let st = state_revert.lock().unwrap();
        let resolved_proj_dir = st.current_proj_dir.clone().or_else(|| {
            let p = Path::new(chunk_str.as_str());
            Some(resolve_project_dir(p))
        });

        if let Some(proj_dir) = resolved_proj_dir {
            let _ = tx_revert.send(WorkerCommand::RevertAsset {
                proj_dir,
                chunk_path: chunk_str.to_string(),
            });
        }
    });

    let tx_wav_exp = tx.clone();
    ui.on_export_wav(move |chunk_str, out_str| {
        let _ = tx_wav_exp.send(WorkerCommand::ExportWav {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            out_path: PathBuf::from(out_str.as_str()),
        });
    });

    let tx_wav_imp = tx.clone();
    ui.on_import_wav(move |chunk_str, wav_str| {
        let _ = tx_wav_imp.send(WorkerCommand::ImportWav {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            in_path: PathBuf::from(wav_str.as_str()),
        });
    });

    let tx_mat = tx.clone();
    ui.on_save_material(move |chunk_str, json_str| {
        let _ = tx_mat.send(WorkerCommand::SaveMaterial {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            json_data: json_str.to_string(),
        });
    });

    let tx_ui = tx.clone();
    ui.on_save_ui(move |chunk_str, json_str| {
        let _ = tx_ui.send(WorkerCommand::SaveUI {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            json_data: json_str.to_string(),
        });
    });

    let tx_obj = tx.clone();
    ui.on_save_object(move |chunk_str, json_str| {
        let _ = tx_obj.send(WorkerCommand::SaveObject {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            json_data: json_str.to_string(),
        });
    });

    let tx_tp = tx.clone();
    ui.on_save_terrain_palette(move |chunk_str, json_str| {
        let _ = tx_tp.send(WorkerCommand::SaveTerrainPalette {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            json_data: json_str.to_string(),
        });
    });

    let tx_env = tx.clone();
    ui.on_save_environment(move |chunk_str, json_str| {
        let _ = tx_env.send(WorkerCommand::SaveEnvironment {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            json_data: json_str.to_string(),
        });
    });

    let tx_m8ld = tx.clone();
    ui.on_save_m8ld(move |chunk_str, json_str| {
        let _ = tx_m8ld.send(WorkerCommand::SaveM8ld {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            json_data: json_str.to_string(),
        });
    });

    let tx_uisp = tx.clone();
    ui.on_save_ui_sprite(move |chunk_str, json_str| {
        let _ = tx_uisp.send(WorkerCommand::SaveUiSprite {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            json_data: json_str.to_string(),
        });
    });

    // 8LD -> XML (1 file)
    let tx_dec_8ld = tx.clone();
    ui.on_decompile_8ld(move |src_str, dst_str| {
        let _ = tx_dec_8ld.send(WorkerCommand::Decompile8ldDirect {
            src: PathBuf::from(src_str.as_str()),
            dst: PathBuf::from(dst_str.as_str()),
        });
    });

    // 8LD -> XML (Batch)
    let tx_dec_8ld_batch = tx.clone();
    ui.on_decompile_8ld_batch(move |src_str, dst_str| {
        let _ = tx_dec_8ld_batch.send(WorkerCommand::Decompile8ldBatch {
            src_dir: PathBuf::from(src_str.as_str()),
            dst_dir: PathBuf::from(dst_str.as_str()),
        });
    });

    // XML -> 8LD (1 file)
    let tx_comp_8ld = tx.clone();
    ui.on_compile_8ld(move |src_str, dst_str| {
        let _ = tx_comp_8ld.send(WorkerCommand::Compile8ldDirect {
            src: PathBuf::from(src_str.as_str()),
            dst: PathBuf::from(dst_str.as_str()),
        });
    });

    // XML -> 8LD (Batch)
    let tx_comp_8ld_batch = tx.clone();
    ui.on_compile_8ld_batch(move |src_str, dst_str| {
        let _ = tx_comp_8ld_batch.send(WorkerCommand::Compile8ldBatch {
            src_dir: PathBuf::from(src_str.as_str()),
            dst_dir: PathBuf::from(dst_str.as_str()),
        });
    });

    let tx_mesh_exp = tx.clone();
    ui.on_export_mesh_obj(move |chunk_str, out_str| {
        let out_path = PathBuf::from(out_str.as_str());
        let is_glb = out_path.extension().is_some_and(|ext| ext == "glb");
        let _ = tx_mesh_exp.send(WorkerCommand::ExportMesh {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            out_path,
            is_glb,
        });
    });

    let tx_mesh_imp = tx.clone();
    ui.on_import_mesh_obj(move |chunk_str, obj_str| {
        let in_path = PathBuf::from(obj_str.as_str());
        let is_glb = in_path.extension().is_some_and(|ext| ext == "glb");
        let _ = tx_mesh_imp.send(WorkerCommand::ImportMesh {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            in_path,
            is_glb,
        });
    });

    let tx_terr_exp = tx.clone();
    ui.on_export_terrain_obj(move |chunk_str, out_str| {
        let out_path = PathBuf::from(out_str.as_str());
        let is_glb = out_path.extension().is_some_and(|ext| ext == "glb");
        let _ = tx_terr_exp.send(WorkerCommand::ExportTerrain {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            out_path,
            is_glb,
        });
    });

    let tx_col_exp = tx.clone();
    ui.on_export_collision_glb(move |chunk_str, out_str| {
        let _ = tx_col_exp.send(WorkerCommand::ExportCollisionGlb {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            out_path: PathBuf::from(out_str.as_str()),
        });
    });

    let tx_col_imp = tx.clone();
    ui.on_import_collision_glb(move |chunk_str, in_str| {
        let _ = tx_col_imp.send(WorkerCommand::ImportCollisionGlb {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            in_path: PathBuf::from(in_str.as_str()),
        });
    });

    let tx_lua_exp = tx.clone();
    ui.on_export_lua(move |chunk_str, out_str| {
        let _ = tx_lua_exp.send(WorkerCommand::ExportLua {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            out_path: PathBuf::from(out_str.as_str()),
        });
    });

    let tx_lua_imp = tx.clone();
    ui.on_import_lua(move |chunk_str, lua_str| {
        let _ = tx_lua_imp.send(WorkerCommand::ImportLua {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            in_path: PathBuf::from(lua_str.as_str()),
        });
    });

    let tx_anim_glb = tx.clone();
    ui.on_export_anim_glb(move |chunk_str, out_str| {
        let _ = tx_anim_glb.send(WorkerCommand::ExportAnimGlb {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            out_path: PathBuf::from(out_str.as_str()),
        });
    });

    let tx_anim_json = tx;
    ui.on_export_anim_json(move |chunk_str, out_str| {
        let _ = tx_anim_json.send(WorkerCommand::ExportAnimJson {
            chunk_path: PathBuf::from(chunk_str.as_str()),
            out_path: PathBuf::from(out_str.as_str()),
        });
    });
}
