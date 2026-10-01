use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};
use std::fs;
use std::sync::{Arc, Mutex};

use crate::AppWindow;
use crate::engine::assets::audio::{export_wav, replace_wav};
use crate::engine::assets::lua::{
    disassemble_lua_bytecode, extract_lua_bytecode, replace_lua_bytecode,
};
use crate::engine::assets::material::{export_material_to_json, import_material_from_json};
use crate::engine::assets::mesh::{
    export_mesh_to_glb, export_mesh_to_obj, import_glb_to_mesh, import_obj_to_mesh,
};
use crate::engine::assets::sniffer::AssetKind;
use crate::engine::assets::terrain::{export_terrain_to_glb, export_terrain_to_obj};
use crate::engine::assets::texture::parse_texture_chunk;
use crate::gui::CachedAsset;
use crate::utils::dds_decoder;
use crate::utils::logger::UiLogger;

pub fn register(ui: &AppWindow, logger: UiLogger, cache: Arc<Mutex<Vec<CachedAsset>>>) {
    // SELECTION IN WORKSPACE
    let ui_weak = ui.as_weak();
    let cache_w = cache;
    ui.on_select_asset(move |index| {
        let cache_guard = cache_w.lock().unwrap();
        if let Some(target) = cache_guard.get(index as usize) {
            let path = target.path.clone();
            let kind = target.kind;
            let ui_handle = ui_weak.clone();

            let bytes = fs::read(&path).unwrap_or_default();
            let path_str = path.to_string_lossy().to_string();

            let _ = ui_handle.upgrade_in_event_loop(move |ui| {
                ui.set_selected_index(index);
                ui.set_active_file_path(path_str.into());

                match kind {
                    AssetKind::Texture => {
                        ui.set_active_kind_id(0);
                        if let Ok(tex) = parse_texture_chunk(&bytes) {
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
                    AssetKind::Audio => {
                        ui.set_active_kind_id(1);
                    }
                    AssetKind::Material => {
                        ui.set_active_kind_id(2);
                        if let Ok(json) = export_material_to_json(&bytes) {
                            ui.set_mat_json_text(json.into());
                        }
                    }
                    AssetKind::Mesh => {
                        ui.set_active_kind_id(3);
                        if let Ok((_, stats)) = export_mesh_to_glb(&bytes) {
                            ui.set_mesh_info(
                                format!(
                                    "Vertices: {} | Triangles: {} | Stride: {}b",
                                    stats.vertex_count, stats.triangle_count, stats.stride
                                )
                                .into(),
                            );
                        } else if let Ok((_, stats)) = export_mesh_to_obj(&bytes) {
                            ui.set_mesh_info(
                                format!(
                                    "Vertices: {} | Triangles: {} | Stride: {}b",
                                    stats.vertex_count, stats.triangle_count, stats.stride
                                )
                                .into(),
                            );
                        }
                    }
                    AssetKind::Lua => {
                        ui.set_active_kind_id(4);
                        if let Ok(disasm) = disassemble_lua_bytecode(&bytes) {
                            ui.set_mat_json_text(disasm.into());
                        }
                    }
                    _ => {
                        ui.set_active_kind_id(5);
                    }
                }
            });
        }
    });

    // AUDIO
    let log_aud = logger.clone();
    ui.on_export_wav(move |chunk_str, out_str| {
        let data = fs::read(chunk_str.as_str()).unwrap_or_default();
        match export_wav(&data) {
            Ok(wav) => {
                let _ = fs::write(out_str.as_str(), wav);
                log_aud.log(&format!("[+] Audio exported to WAV: {}", out_str));
            }
            Err(e) => log_aud.log(&format!("[!] Audio export error: {}", e)),
        }
    });

    let log_aud_imp = logger.clone();
    ui.on_import_wav(move |chunk_str, wav_str| {
        let chunk_data = fs::read(chunk_str.as_str()).unwrap_or_default();
        let wav_data = fs::read(wav_str.as_str()).unwrap_or_default();
        match replace_wav(&chunk_data, &wav_data) {
            Ok(new_chunk) => {
                let _ = fs::write(chunk_str.as_str(), new_chunk);
                log_aud_imp.log(&format!("[+] Audio chunk {} updated.", chunk_str));
            }
            Err(e) => log_aud_imp.log(&format!("[!] Audio import error: {}", e)),
        }
    });

    // MATERIAL
    let log_mat = logger.clone();
    ui.on_save_material(move |chunk_str, json_str| {
        match import_material_from_json(json_str.as_str()) {
            Ok(bin) => {
                let _ = fs::write(chunk_str.as_str(), bin);
                log_mat.log(&format!("[+] Material chunk {} updated.", chunk_str));
            }
            Err(e) => log_mat.log(&format!("[!] Material save error: {}", e)),
        }
    });

    // 3D MESH & TERRAIN
    let log_mesh = logger.clone();
    let ui_weak_m = ui.as_weak();
    ui.on_export_mesh_obj(move |chunk_str, out_str| {
        let data = fs::read(chunk_str.as_str()).unwrap_or_default();
        let l = log_mesh.clone();
        if out_str.to_lowercase().ends_with(".glb") {
            match export_mesh_to_glb(&data) {
                Ok((glb, stats)) => {
                    let _ = fs::write(out_str.as_str(), glb);
                    l.log(&format!(
                        "[+] 3D Mesh exported to glTF 2.0 (.glb): {} ({} vertices, {} triangles)",
                        out_str, stats.vertex_count, stats.triangle_count
                    ));
                    let info = format!(
                        "Vertices: {} | Triangles: {} | Stride: {}b",
                        stats.vertex_count, stats.triangle_count, stats.stride
                    );
                    let _ = ui_weak_m.upgrade_in_event_loop(move |ui| {
                        ui.set_mesh_info(info.into());
                    });
                }
                Err(e) => l.log(&format!("[!] glTF export error: {}", e)),
            }
        } else {
            match export_mesh_to_obj(&data) {
                Ok((obj, stats)) => {
                    let _ = fs::write(out_str.as_str(), obj);
                    l.log(&format!(
                        "[+] 3D Mesh exported to OBJ: {} ({} vertices)",
                        out_str, stats.vertex_count
                    ));
                    let info = format!(
                        "Vertices: {} | Triangles: {} | Stride: {}b",
                        stats.vertex_count, stats.triangle_count, stats.stride
                    );
                    let _ = ui_weak_m.upgrade_in_event_loop(move |ui| {
                        ui.set_mesh_info(info.into());
                    });
                }
                Err(e) => l.log(&format!("[!] Mesh export error: {}", e)),
            }
        }
    });

    let log_mesh_imp = logger.clone();
    ui.on_import_mesh_obj(move |chunk_str, obj_str| {
        let chunk_data = fs::read(chunk_str.as_str()).unwrap_or_default();
        if obj_str.to_lowercase().ends_with(".glb") {
            let glb_bytes = fs::read(obj_str.as_str()).unwrap_or_default();
            match import_glb_to_mesh(&chunk_data, &glb_bytes) {
                Ok(bin) => {
                    let _ = fs::write(chunk_str.as_str(), bin);
                    log_mesh_imp.log(&format!(
                        "[+] Mesh chunk {} rebuilt from glTF 2.0 (.glb).",
                        chunk_str
                    ));
                }
                Err(e) => log_mesh_imp.log(&format!("[!] glTF import error: {}", e)),
            }
        } else {
            let obj_text = fs::read_to_string(obj_str.as_str()).unwrap_or_default();
            match import_obj_to_mesh(&chunk_data, &obj_text) {
                Ok(bin) => {
                    let _ = fs::write(chunk_str.as_str(), bin);
                    log_mesh_imp.log(&format!("[+] Mesh chunk {} rebuilt from OBJ.", chunk_str));
                }
                Err(e) => log_mesh_imp.log(&format!("[!] Mesh import error: {}", e)),
            }
        }
    });

    let log_terr = logger.clone();
    ui.on_export_terrain_obj(move |chunk_str, out_str| {
        let data = fs::read(chunk_str.as_str()).unwrap_or_default();
        if out_str.to_lowercase().ends_with(".glb") {
            match export_terrain_to_glb(&data) {
                Ok((glb, v_count, tri_count)) => {
                    let _ = fs::write(out_str.as_str(), glb);
                    log_terr.log(&format!(
                        "[+] Terrain exported to glTF 2.0 (.glb) with vertex colors: {} ({} vertices, {} triangles)",
                        out_str, v_count, tri_count
                    ));
                }
                Err(e) => log_terr.log(&format!("[!] Terrain glTF export error: {}", e)),
            }
        } else {
            match export_terrain_to_obj(&data) {
                Ok(obj) => {
                    let _ = fs::write(out_str.as_str(), obj);
                    log_terr.log(&format!(
                        "[+] Terrain heightmap exported to OBJ: {}",
                        out_str
                    ));
                }
                Err(e) => log_terr.log(&format!("[!] Terrain export error: {}", e)),
            }
        }
    });

    // LUA
    let log_lua = logger.clone();
    ui.on_export_lua(move |chunk_str, out_str| {
        let data = fs::read(chunk_str.as_str()).unwrap_or_default();
        match extract_lua_bytecode(&data) {
            Ok(bytecode) => {
                let _ = fs::write(out_str.as_str(), bytecode);
                log_lua.log(&format!("[+] Lua 5.0.2 bytecode exported: {}", out_str));
            }
            Err(e) => log_lua.log(&format!("[!] Lua export error: {}", e)),
        }
    });

    let log_lua_imp = logger;
    ui.on_import_lua(move |chunk_str, luac_str| {
        let chunk_data = fs::read(chunk_str.as_str()).unwrap_or_default();
        let luac_data = fs::read(luac_str.as_str()).unwrap_or_default();
        match replace_lua_bytecode(&chunk_data, &luac_data) {
            Ok(new_chunk) => {
                let _ = fs::write(chunk_str.as_str(), new_chunk);
                log_lua_imp.log(&format!("[+] Lua chunk {} updated.", chunk_str));
            }
            Err(e) => log_lua_imp.log(&format!("[!] Lua import error: {}", e)),
        }
    });
}
