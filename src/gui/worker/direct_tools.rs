use std::fs;
use std::path::PathBuf;

use crate::AppWindow;
use crate::engine::service;
use crate::gui::worker::project_ops::{set_ui_error, set_ui_status};
use crate::utils::logger::UiLogger;

pub fn handle_decompile_8ld_direct(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    logger.log(&format!("[*] Extracting XML from .8ld: {:?}", src));
    match service::decompile_8ld_file(&src, &dst) {
        Ok(out_file) => {
            logger.log(&format!(
                "[+] Successfully extracted XML to: {:?}",
                out_file
            ));
            set_ui_status(ui_handle, "XML extracted successfully.", false);
        }
        Err(e) => {
            logger.log(&format!("[!] 8LD Extract Error: {}", e));
            set_ui_error(ui_handle, format!("8LD Error: {}", e));
        }
    }
}

pub fn handle_compile_8ld_direct(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    logger.log(&format!("[*] Compiling XML to .8ld: {:?}", src));
    match service::compile_8ld_file(&src, &dst) {
        Ok(out_file) => {
            logger.log(&format!(
                "[+] Successfully compiled to .8ld: {:?}",
                out_file
            ));
            set_ui_status(ui_handle, "XML compiled to .8ld successfully.", false);
        }
        Err(e) => {
            logger.log(&format!("[!] 8LD Compile Error: {}", e));
            set_ui_error(ui_handle, format!("8LD Error: {}", e));
        }
    }
}

pub fn handle_decompile_8ld_batch(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src_dir: PathBuf,
    dst_dir: PathBuf,
) {
    logger.log(&format!(
        "[*] Batch extracting .8ld files from: {:?}",
        src_dir
    ));
    match service::batch_decompile_8ld(&src_dir, &dst_dir) {
        Ok(count) => {
            logger.log(&format!(
                "[+] Successfully converted {} files to XML in {:?}",
                count, dst_dir
            ));
            set_ui_status(ui_handle, "Batch XML extraction complete.", false);
        }
        Err(e) => {
            logger.log(&format!("[!] Batch 8LD Extract Error: {}", e));
            set_ui_error(ui_handle, format!("Batch 8LD Error: {}", e));
        }
    }
}

pub fn handle_compile_8ld_batch(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src_dir: PathBuf,
    dst_dir: PathBuf,
) {
    logger.log(&format!(
        "[*] Batch compiling XML files from: {:?}",
        src_dir
    ));
    match service::batch_compile_8ld(&src_dir, &dst_dir) {
        Ok(count) => {
            logger.log(&format!(
                "[+] Successfully converted {} files to .8ld in {:?}",
                count, dst_dir
            ));
            set_ui_status(ui_handle, "Batch 8LD compilation complete.", false);
        }
        Err(e) => {
            logger.log(&format!("[!] Batch XML->8LD Error: {}", e));
            set_ui_error(ui_handle, format!("Batch 8LD Error: {}", e));
        }
    }
}

pub fn handle_direct_vpk_to_json(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    match fs::read(&src) {
        Ok(data) => match crate::engine::assets::vpk::export_vpk_to_json(&data) {
            Ok(json) => {
                let _ = fs::write(&dst, json.as_bytes());
                logger.log(&format!(
                    "[+] Converted Voice Package (.debug-vpk) to JSON: {:?}",
                    dst
                ));
                set_ui_status(
                    ui_handle,
                    "Voice Package converted to JSON successfully.",
                    false,
                );
            }
            Err(e) => set_ui_error(ui_handle, format!("VPK Export Error: {}", e)),
        },
        Err(e) => set_ui_error(ui_handle, format!("Failed to read VPK: {}", e)),
    }
}

pub fn handle_direct_json_to_vpk(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src_json: PathBuf,
    baseline_vpk: PathBuf,
    dst: PathBuf,
) {
    match (fs::read_to_string(&src_json), fs::read(&baseline_vpk)) {
        (Ok(json), Ok(base)) => {
            match crate::engine::assets::vpk::import_vpk_from_json(&json, &base) {
                Ok(bin) => {
                    let _ = fs::write(&dst, bin);
                    logger.log(&format!(
                        "[+] Rebuilt Voice Package (.debug-vpk) from JSON: {:?}",
                        dst
                    ));
                    set_ui_status(ui_handle, "Voice Package built successfully.", false);
                }
                Err(e) => set_ui_error(ui_handle, format!("VPK Compile Error: {}", e)),
            }
        }
        (Err(e), _) => set_ui_error(ui_handle, format!("Failed to read JSON: {}", e)),
        (_, Err(e)) => set_ui_error(ui_handle, format!("Failed to read baseline VPK: {}", e)),
    }
}

pub fn handle_direct_dta_to_json(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    match fs::read(&src) {
        Ok(data) => {
            let stem = src.file_stem().unwrap_or_default().to_string_lossy();
            match crate::engine::assets::dta::export_dta_to_json(&data, &stem) {
                Ok(json) => {
                    let _ = fs::write(&dst, json.as_bytes());
                    logger.log(&format!("[+] Converted DTA to JSON: {:?}", dst));
                    set_ui_status(ui_handle, "Successfully converted DTA to JSON.", false);
                }
                Err(e) => set_ui_error(ui_handle, format!("DTA Error: {}", e)),
            }
        }
        Err(e) => set_ui_error(ui_handle, format!("Failed to read DTA: {}", e)),
    }
}

pub fn handle_direct_json_to_dta(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src_json: PathBuf,
    baseline_dta: PathBuf,
    dst: PathBuf,
) {
    match (fs::read_to_string(&src_json), fs::read(&baseline_dta)) {
        (Ok(json), Ok(base)) => {
            match crate::engine::assets::dta::import_dta_from_json(&json, &base) {
                Ok(bin) => {
                    let _ = fs::write(&dst, bin);
                    logger.log(&format!("[+] Rebuilt DTA from JSON: {:?}", dst));
                    set_ui_status(ui_handle, "Successfully built DTA.", false);
                }
                Err(e) => set_ui_error(ui_handle, format!("DTA Compile Error: {}", e)),
            }
        }
        (Err(e), _) => set_ui_error(ui_handle, format!("Failed to read JSON: {}", e)),
        (_, Err(e)) => set_ui_error(ui_handle, format!("Failed to read baseline DTA: {}", e)),
    }
}

pub fn handle_direct_env_to_json(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    match fs::read(&src) {
        Ok(data) => match crate::engine::assets::environment::export_environment_to_json(&data) {
            Ok(json) => {
                let _ = fs::write(&dst, json.as_bytes());
                logger.log(&format!("[+] Converted ENV to JSON: {:?}", dst));
                set_ui_status(ui_handle, "Successfully converted ENV to JSON.", false);
            }
            Err(e) => set_ui_error(ui_handle, format!("ENV Error: {}", e)),
        },
        Err(e) => set_ui_error(ui_handle, format!("Failed to read ENV: {}", e)),
    }
}

pub fn handle_direct_json_to_env(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src_json: PathBuf,
    baseline_env: PathBuf,
    dst: PathBuf,
) {
    match (fs::read_to_string(&src_json), fs::read(&baseline_env)) {
        (Ok(json), Ok(base)) => {
            match crate::engine::assets::environment::import_environment_from_json(&json, &base) {
                Ok(bin) => {
                    let _ = fs::write(&dst, bin);
                    logger.log(&format!("[+] Rebuilt ENV from JSON: {:?}", dst));
                    set_ui_status(ui_handle, "Successfully built ENV profile.", false);
                }
                Err(e) => set_ui_error(ui_handle, format!("ENV Compile Error: {}", e)),
            }
        }
        (Err(e), _) => set_ui_error(ui_handle, format!("Failed to read JSON: {}", e)),
        (_, Err(e)) => set_ui_error(ui_handle, format!("Failed to read baseline ENV: {}", e)),
    }
}

pub fn handle_direct_mesh_export(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
    is_glb: bool,
) {
    match service::export_mesh(&src, &dst, is_glb) {
        Ok(stats) => {
            let kind = if is_glb { "glTF" } else { "OBJ" };
            logger.log(&format!(
                "[+] Exported {} mesh: {:?} ({} verts, {} tris)",
                kind, dst, stats.vertex_count, stats.triangle_count
            ));
            set_ui_status(ui_handle, "Mesh exported successfully.", false);
        }
        Err(e) => set_ui_error(ui_handle, format!("Mesh Export Error: {}", e)),
    }
}

pub fn handle_direct_mesh_import(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    chunk_target: PathBuf,
    model_src: PathBuf,
    is_glb: bool,
) {
    match service::import_mesh(&chunk_target, &model_src, is_glb) {
        Ok(_) => {
            logger.log(&format!(
                "[+] Injected 3D model {:?} into {:?}",
                model_src, chunk_target
            ));
            set_ui_status(ui_handle, "Mesh chunk successfully updated.", false);
        }
        Err(e) => set_ui_error(ui_handle, format!("Mesh Import Error: {}", e)),
    }
}

pub fn handle_direct_assemble_level(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    omp_path: PathBuf,
    assets_dir: PathBuf,
    dst: PathBuf,
) {
    logger.log(&format!(
        "[*] Assembling full 3D scene from {:?} using assets {:?}",
        omp_path, assets_dir
    ));
    match fs::read(&omp_path) {
        Ok(data) => {
            match crate::engine::assets::map::assemble_level_scene_glb(&data, &assets_dir) {
                Ok(glb) => {
                    let _ = fs::write(&dst, glb);
                    logger.log(&format!("[+] Level scene assembled into: {:?}", dst));
                    set_ui_status(
                        ui_handle,
                        "Level scene successfully assembled with PBR textures.",
                        false,
                    );
                }
                Err(e) => set_ui_error(ui_handle, format!("Level Assembly Error: {}", e)),
            }
        }
        Err(e) => set_ui_error(ui_handle, format!("Failed to read OMP: {}", e)),
    }
}

pub fn handle_direct_terrain_export(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
    is_glb: bool,
) {
    match service::export_terrain(&src, &dst, is_glb) {
        Ok((v, t)) => {
            logger.log(&format!(
                "[+] Exported terrain heightmap: {:?} ({} vertices, {} triangles)",
                dst, v, t
            ));
            set_ui_status(ui_handle, "Terrain exported successfully.", false);
        }
        Err(e) => set_ui_error(ui_handle, format!("Terrain Export Error: {}", e)),
    }
}

pub fn handle_direct_collision_export(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    match service::export_collision_glb(&src, &dst) {
        Ok(size) => {
            logger.log(&format!(
                "[+] Exported 3D collision boxes: {:?} ({} bytes)",
                dst, size
            ));
            set_ui_status(
                ui_handle,
                "Collision exported to 3D GLB successfully.",
                false,
            );
        }
        Err(e) => set_ui_error(ui_handle, format!("Collision Export Error: {}", e)),
    }
}

pub fn handle_direct_collision_import(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    chunk_target: PathBuf,
    glb_src: PathBuf,
) {
    match service::import_collision_glb(&chunk_target, &glb_src) {
        Ok(_) => {
            logger.log(&format!(
                "[+] Updated collision chunk {:?} from {:?}",
                chunk_target, glb_src
            ));
            set_ui_status(ui_handle, "Collision chunk updated.", false);
        }
        Err(e) => set_ui_error(ui_handle, format!("Collision Import Error: {}", e)),
    }
}

pub fn handle_direct_font_to_json(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    match fs::read(&src) {
        Ok(data) => match crate::engine::assets::ui_sprite::export_ui_sprite_collection(&data) {
            Ok(json) => {
                let _ = fs::write(&dst, json.as_bytes());
                logger.log(&format!(
                    "[+] Exported UI Font / Sprite collection to JSON: {:?}",
                    dst
                ));
                set_ui_status(ui_handle, "Font collection exported to JSON.", false);
            }
            Err(e) => set_ui_error(ui_handle, format!("Font Export Error: {}", e)),
        },
        Err(e) => set_ui_error(ui_handle, format!("Failed to read Font CLB: {}", e)),
    }
}

pub fn handle_direct_json_to_font(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src_json: PathBuf,
    dst: PathBuf,
) {
    match fs::read_to_string(&src_json) {
        Ok(text) => match crate::engine::assets::ui_sprite::import_ui_sprite_collection(&text) {
            Ok(bin) => {
                let _ = fs::write(&dst, bin);
                logger.log(&format!("[+] Rebuilt Font CLB collection: {:?}", dst));
                set_ui_status(ui_handle, "Font collection built.", false);
            }
            Err(e) => set_ui_error(ui_handle, format!("Font Compile Error: {}", e)),
        },
        Err(e) => set_ui_error(ui_handle, format!("Failed to read JSON: {}", e)),
    }
}

pub fn handle_direct_texture_export(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    match service::export_texture(&src, &dst) {
        Ok(_) => {
            logger.log(&format!("[+] Exported texture chunk to: {:?}", dst));
            set_ui_status(ui_handle, "Texture exported successfully.", false);
        }
        Err(e) => set_ui_error(ui_handle, format!("Texture Export Error: {}", e)),
    }
}

pub fn handle_direct_texture_import(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    chunk_target: PathBuf,
    img_src: PathBuf,
) {
    match service::import_texture(&chunk_target, &img_src) {
        Ok(_) => {
            logger.log(&format!(
                "[+] Updated texture chunk {:?} from {:?}",
                chunk_target, img_src
            ));
            set_ui_status(ui_handle, "Texture chunk updated.", false);
        }
        Err(e) => set_ui_error(ui_handle, format!("Texture Import Error: {}", e)),
    }
}

pub fn handle_direct_audio_export(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    match service::export_audio(&src, &dst) {
        Ok(_) => {
            logger.log(&format!("[+] Exported audio chunk to WAV: {:?}", dst));
            set_ui_status(ui_handle, "Audio exported successfully.", false);
        }
        Err(e) => set_ui_error(ui_handle, format!("Audio Export Error: {}", e)),
    }
}

pub fn handle_direct_audio_import(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    chunk_target: PathBuf,
    wav_src: PathBuf,
) {
    match service::import_audio(&chunk_target, &wav_src) {
        Ok(_) => {
            logger.log(&format!(
                "[+] Updated audio chunk {:?} from {:?}",
                chunk_target, wav_src
            ));
            set_ui_status(ui_handle, "Audio chunk updated.", false);
        }
        Err(e) => set_ui_error(ui_handle, format!("Audio Import Error: {}", e)),
    }
}
