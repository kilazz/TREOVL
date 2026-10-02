use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc::Sender};

use crate::AppWindow;
use crate::engine::assets::sniffer::AssetKind;
use crate::gui::AppState;
use crate::gui::commands::WorkerCommand;
use crate::utils::dds_decoder;
use crate::utils::logger::UiLogger;

pub fn register(
    ui: &AppWindow,
    tx: Sender<WorkerCommand>,
    _logger: UiLogger,
    state: Arc<Mutex<AppState>>,
) {
    let ui_weak = ui.as_weak();
    let state_w = state.clone();

    ui.on_select_asset(move |filtered_index| {
        let st = state_w.lock().unwrap();
        if let Some(&real_index) = st.visible_indices.get(filtered_index as usize)
            && let Some(target) = st.all_cached_assets.get(real_index)
        {
            let path = target.path.clone();
            let kind = target.kind;
            let ui_handle = ui_weak.clone();

            let bytes = fs::read(&path).unwrap_or_default();
            let path_str = path.to_string_lossy().to_string();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_handle.upgrade() {
                    ui.set_selected_index(filtered_index);
                    ui.set_active_file_path(path_str.into());

                    match kind {
                        AssetKind::Texture => {
                            ui.set_active_kind_id(0);
                            if let Ok(tex) =
                                crate::engine::assets::texture::parse_texture_chunk(&bytes)
                            {
                                let rgba = dds_decoder::decode_to_rgba(
                                    tex.width,
                                    tex.height,
                                    tex.format,
                                    &tex.pixel_data,
                                );
                                let mut buf =
                                    SharedPixelBuffer::<Rgba8Pixel>::new(tex.width, tex.height);
                                let dest = buf.make_mut_bytes();
                                let copy_len = dest.len().min(rgba.len());
                                dest[..copy_len].copy_from_slice(&rgba[..copy_len]);
                                ui.set_tex_preview(Image::from_rgba8(buf));
                                ui.set_tex_info(
                                    format!(
                                        "Resolution: {}x{} | Format: {:?}",
                                        tex.width, tex.height, tex.format
                                    )
                                    .into(),
                                );
                                ui.set_has_texture(true);
                            } else {
                                ui.set_has_texture(false);
                            }
                        }
                        AssetKind::Audio => ui.set_active_kind_id(1),
                        AssetKind::Material => {
                            ui.set_active_kind_id(2);
                            if let Ok(json) =
                                crate::engine::assets::material::export_material_to_json(&bytes)
                            {
                                ui.set_mat_json_text(json.into());
                            }
                        }
                        AssetKind::Mesh => {
                            ui.set_active_kind_id(3);
                            if let Ok((_, stats)) =
                                crate::engine::assets::mesh::export_mesh_to_glb(&bytes)
                            {
                                ui.set_mesh_info(
                                    format!(
                                        "Vertices: {} | Triangles: {} | Skinned: {}",
                                        stats.vertex_count, stats.triangle_count, stats.is_skinned
                                    )
                                    .into(),
                                );
                            }
                        }
                        AssetKind::Lua => {
                            ui.set_active_kind_id(4);
                            if let Ok(disasm) =
                                crate::engine::assets::lua::disassemble_lua_bytecode(&bytes)
                            {
                                ui.set_mat_json_text(disasm.into());
                            }
                        }
                        AssetKind::UI => {
                            ui.set_active_kind_id(6);
                            if let Ok(json) = crate::engine::assets::ui::export_ui_to_json(&bytes) {
                                ui.set_mat_json_text(json.into());
                            }
                        }
                        AssetKind::Object => {
                            ui.set_active_kind_id(7);
                            if let Ok(json) =
                                crate::engine::assets::object::export_object_to_json(&bytes, None)
                            {
                                ui.set_mat_json_text(json.into());
                            }
                        }
                        AssetKind::Animation => {
                            ui.set_active_kind_id(8);
                            if let Ok(clip) =
                                crate::engine::assets::animation::parse_animation_clip(&bytes)
                            {
                                ui.set_mesh_info(
                                    format!(
                                        "Clip: {} | Rig: {} | {:.1} FPS | Duration: {:.3}s | Tracks: {}",
                                        clip.name,
                                        clip.target_rig,
                                        clip.frame_rate,
                                        clip.duration_seconds,
                                        clip.bone_tracks.len()
                                    )
                                    .into(),
                                );
                            }
                            if let Ok(json_str) =
                                crate::engine::assets::animation::export_animation_to_json(&bytes)
                            {
                                ui.set_mat_json_text(json_str.into());
                            }
                        }
                        AssetKind::TerrainPalette => {
                            ui.set_active_kind_id(9);
                            if let Ok(json) =
                                crate::engine::assets::terrain_palette::export_terrain_palette_to_json(
                                    &bytes,
                                )
                            {
                                ui.set_mat_json_text(json.into());
                            }
                        }
                        _ => ui.set_active_kind_id(5),
                    }
                }
            });
        }
    });

    let tx_revert = tx.clone();
    let state_revert = state;
    ui.on_revert_asset(move |chunk_str| {
        let st = state_revert.lock().unwrap();
        let resolved_proj_dir = st.current_proj_dir.clone().or_else(|| {
            Path::new(chunk_str.as_str())
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf())
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
