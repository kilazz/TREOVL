use std::fs;
use std::path::PathBuf;

use crate::AppWindow;
use crate::engine::service;
use crate::gui::worker::project_ops::{set_ui_error, set_ui_status};
use crate::utils::logger::UiLogger;

/// Generic wrapper to handle direct asset operations, abstracting away the boilerplate
/// of error handling, logging, and UI status updates.
fn run_direct_action<T, F, S>(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    action_desc: &str,
    success_status: &'static str,
    operation: F,
    on_success: S,
) where
    F: FnOnce() -> anyhow::Result<T>,
    S: FnOnce(T, &UiLogger),
{
    logger.log(&format!("[*] {}", action_desc));
    match operation() {
        Ok(res) => {
            on_success(res, logger);
            set_ui_status(ui_handle, success_status, false);
        }
        Err(e) => {
            logger.log(&format!("[!] Error ({}): {:#}", action_desc, e));
            set_ui_error(ui_handle, format!("Error: {}", e));
        }
    }
}

pub fn handle_decompile_8ld_direct(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Extracting XML from .8ld: {:?}", src),
        "XML extracted successfully.",
        || service::decompile_8ld_file(&src, &dst),
        |out_file, log| {
            log.log(&format!(
                "[+] Successfully extracted XML to: {:?}",
                out_file
            ))
        },
    );
}

pub fn handle_compile_8ld_direct(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Compiling XML to .8ld: {:?}", src),
        "XML compiled to .8ld successfully.",
        || service::compile_8ld_file(&src, &dst),
        |out_file, log| {
            log.log(&format!(
                "[+] Successfully compiled to .8ld: {:?}",
                out_file
            ))
        },
    );
}

pub fn handle_decompile_8ld_batch(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src_dir: PathBuf,
    dst_dir: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Batch extracting .8ld files from: {:?}", src_dir),
        "Batch XML extraction complete.",
        || service::batch_decompile_8ld(&src_dir, &dst_dir),
        |count, log| {
            log.log(&format!(
                "[+] Successfully converted {} files to XML in {:?}",
                count, dst_dir
            ))
        },
    );
}

pub fn handle_compile_8ld_batch(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src_dir: PathBuf,
    dst_dir: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Batch compiling XML files from: {:?}", src_dir),
        "Batch 8LD compilation complete.",
        || service::batch_compile_8ld(&src_dir, &dst_dir),
        |count, log| {
            log.log(&format!(
                "[+] Successfully converted {} files to .8ld in {:?}",
                count, dst_dir
            ))
        },
    );
}

pub fn handle_direct_vpk_to_json(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Converting VPK to JSON: {:?}", src),
        "Voice Package converted to JSON successfully.",
        || {
            let data = fs::read(&src)?;
            let json = crate::engine::assets::vpk::export_vpk_to_json(&data)?;
            fs::write(&dst, json.as_bytes())?;
            Ok(dst)
        },
        |out_file, log| {
            log.log(&format!(
                "[+] Converted Voice Package (.debug-vpk) to JSON: {:?}",
                out_file
            ))
        },
    );
}

pub fn handle_direct_json_to_vpk(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src_json: PathBuf,
    baseline_vpk: PathBuf,
    dst: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Compiling JSON to VPK: {:?}", src_json),
        "Voice Package built successfully.",
        || {
            let json = fs::read_to_string(&src_json)?;
            let base = fs::read(&baseline_vpk)?;
            let bin = crate::engine::assets::vpk::import_vpk_from_json(&json, &base)?;
            fs::write(&dst, bin)?;
            Ok(dst)
        },
        |out_file, log| {
            log.log(&format!(
                "[+] Rebuilt Voice Package (.debug-vpk) from JSON: {:?}",
                out_file
            ))
        },
    );
}

pub fn handle_direct_dta_to_json(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Converting DTA to JSON: {:?}", src),
        "Successfully converted DTA to JSON.",
        || {
            let data = fs::read(&src)?;
            let stem = src.file_stem().unwrap_or_default().to_string_lossy();
            let json = crate::engine::assets::dta::export_dta_to_json(&data, &stem)?;
            fs::write(&dst, json.as_bytes())?;
            Ok(dst)
        },
        |out_file, log| log.log(&format!("[+] Converted DTA to JSON: {:?}", out_file)),
    );
}

pub fn handle_direct_json_to_dta(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src_json: PathBuf,
    baseline_dta: PathBuf,
    dst: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Compiling JSON to DTA: {:?}", src_json),
        "Successfully built DTA.",
        || {
            let json = fs::read_to_string(&src_json)?;
            let base = fs::read(&baseline_dta)?;
            let bin = crate::engine::assets::dta::import_dta_from_json(&json, &base)?;
            fs::write(&dst, bin)?;
            Ok(dst)
        },
        |out_file, log| log.log(&format!("[+] Rebuilt DTA from JSON: {:?}", out_file)),
    );
}

pub fn handle_direct_env_to_json(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Converting ENV to JSON: {:?}", src),
        "Successfully converted ENV to JSON.",
        || {
            let data = fs::read(&src)?;
            let json = crate::engine::assets::environment::export_environment_to_json(&data)?;
            fs::write(&dst, json.as_bytes())?;
            Ok(dst)
        },
        |out_file, log| log.log(&format!("[+] Converted ENV to JSON: {:?}", out_file)),
    );
}

pub fn handle_direct_json_to_env(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src_json: PathBuf,
    baseline_env: PathBuf,
    dst: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Compiling JSON to ENV: {:?}", src_json),
        "Successfully built ENV profile.",
        || {
            let json = fs::read_to_string(&src_json)?;
            let base = fs::read(&baseline_env)?;
            let bin =
                crate::engine::assets::environment::import_environment_from_json(&json, &base)?;
            fs::write(&dst, bin)?;
            Ok(dst)
        },
        |out_file, log| log.log(&format!("[+] Rebuilt ENV from JSON: {:?}", out_file)),
    );
}

pub fn handle_direct_mesh_export(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
    is_glb: bool,
) {
    let kind = if is_glb { "glTF" } else { "OBJ" };
    run_direct_action(
        ui_handle,
        logger,
        &format!("Exporting {} mesh from {:?}", kind, src),
        "Mesh exported successfully.",
        || service::export_mesh(&src, &dst, is_glb),
        |stats, log| {
            log.log(&format!(
                "[+] Exported {} mesh: {:?} ({} verts, {} tris)",
                kind, dst, stats.vertex_count, stats.triangle_count
            ))
        },
    );
}

pub fn handle_direct_mesh_import(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    chunk_target: PathBuf,
    model_src: PathBuf,
    is_glb: bool,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Injecting 3D model {:?} into {:?}", model_src, chunk_target),
        "Mesh chunk successfully updated.",
        || service::import_mesh(&chunk_target, &model_src, is_glb),
        |_, log| log.log(&format!("[+] Injected 3D model into {:?}", chunk_target)),
    );
}

pub fn handle_direct_assemble_level(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    omp_path: PathBuf,
    assets_dir: PathBuf,
    dst: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!(
            "Assembling full 3D scene from {:?} using assets {:?}",
            omp_path, assets_dir
        ),
        "Level scene successfully assembled with PBR textures.",
        || {
            let data = fs::read(&omp_path)?;
            let glb = crate::engine::assets::map::assemble_level_scene_glb(&data, &assets_dir)?;
            fs::write(&dst, glb)?;
            Ok(dst)
        },
        |out_file, log| log.log(&format!("[+] Level scene assembled into: {:?}", out_file)),
    );
}

pub fn handle_direct_terrain_export(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
    is_glb: bool,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Exporting terrain from {:?}", src),
        "Terrain exported successfully.",
        || service::export_terrain(&src, &dst, is_glb),
        |(v, t), log| {
            log.log(&format!(
                "[+] Exported terrain heightmap: {:?} ({} vertices, {} triangles)",
                dst, v, t
            ))
        },
    );
}

pub fn handle_direct_collision_export(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Exporting collision from {:?}", src),
        "Collision exported to 3D GLB successfully.",
        || service::export_collision_glb(&src, &dst),
        |size, log| {
            log.log(&format!(
                "[+] Exported 3D collision boxes: {:?} ({} bytes)",
                dst, size
            ))
        },
    );
}

pub fn handle_direct_collision_import(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    chunk_target: PathBuf,
    glb_src: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!(
            "Injecting GLB collision {:?} into {:?}",
            glb_src, chunk_target
        ),
        "Collision chunk updated.",
        || service::import_collision_glb(&chunk_target, &glb_src),
        |_, log| {
            log.log(&format!(
                "[+] Updated collision chunk {:?} from {:?}",
                chunk_target, glb_src
            ))
        },
    );
}

pub fn handle_direct_font_to_json(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Converting Font CLB to JSON: {:?}", src),
        "Font collection exported to JSON.",
        || {
            let data = fs::read(&src)?;
            let json = crate::engine::assets::ui_sprite::export_ui_sprite_collection(&data)?;
            fs::write(&dst, json.as_bytes())?;
            Ok(dst)
        },
        |out_file, log| {
            log.log(&format!(
                "[+] Exported UI Font / Sprite collection to JSON: {:?}",
                out_file
            ))
        },
    );
}

pub fn handle_direct_json_to_font(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src_json: PathBuf,
    dst: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Compiling JSON to Font CLB: {:?}", src_json),
        "Font collection built.",
        || {
            let json = fs::read_to_string(&src_json)?;
            let bin = crate::engine::assets::ui_sprite::import_ui_sprite_collection(&json)?;
            fs::write(&dst, bin)?;
            Ok(dst)
        },
        |out_file, log| log.log(&format!("[+] Rebuilt Font CLB collection: {:?}", out_file)),
    );
}

pub fn handle_direct_texture_export(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Exporting texture from {:?}", src),
        "Texture exported successfully.",
        || service::export_texture(&src, &dst),
        |_, log| log.log(&format!("[+] Exported texture chunk to: {:?}", dst)),
    );
}

pub fn handle_direct_texture_import(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    chunk_target: PathBuf,
    img_src: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Injecting texture {:?} into {:?}", img_src, chunk_target),
        "Texture chunk updated.",
        || service::import_texture(&chunk_target, &img_src),
        |_, log| {
            log.log(&format!(
                "[+] Updated texture chunk {:?} from {:?}",
                chunk_target, img_src
            ))
        },
    );
}

pub fn handle_direct_audio_export(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    src: PathBuf,
    dst: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Exporting audio from {:?}", src),
        "Audio exported successfully.",
        || service::export_audio(&src, &dst),
        |_, log| log.log(&format!("[+] Exported audio chunk to WAV: {:?}", dst)),
    );
}

pub fn handle_direct_audio_import(
    ui_handle: &slint::Weak<AppWindow>,
    logger: &UiLogger,
    chunk_target: PathBuf,
    wav_src: PathBuf,
) {
    run_direct_action(
        ui_handle,
        logger,
        &format!("Injecting audio {:?} into {:?}", wav_src, chunk_target),
        "Audio chunk updated.",
        || service::import_audio(&chunk_target, &wav_src),
        |_, log| {
            log.log(&format!(
                "[+] Updated audio chunk {:?} from {:?}",
                chunk_target, wav_src
            ))
        },
    );
}
