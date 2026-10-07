use parking_lot::Mutex;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use slint::{Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

use crate::AppWindow;
use crate::engine::assets::animation::run_deep_skeleton_diagnostics;
use crate::engine::assets::sniffer::AssetKind;
use crate::gui::worker::preview;
use crate::gui::worker::viewport;
use crate::gui::{ActiveMeshPreview, AppState};
use crate::utils::dds_decoder;
use crate::utils::logger::UiLogger;
use crate::utils::renderer::WgpuRenderer;

pub struct InspectorContext<'a> {
    pub ui_handle: &'a slint::Weak<AppWindow>,
    pub state: &'a Arc<Mutex<AppState>>,
    pub logger: &'a UiLogger,
    pub gpu_renderer: &'a mut Option<WgpuRenderer>,
    pub filtered_index: i32,
    pub path: &'a Path,
    pub path_str: &'a str,
    pub bytes: &'a [u8],
}

pub fn handle_select_asset(
    ui_handle: &slint::Weak<AppWindow>,
    state: &Arc<Mutex<AppState>>,
    logger: &UiLogger,
    gpu_renderer: &mut Option<WgpuRenderer>,
    filtered_index: i32,
    path: &Path,
    kind: AssetKind,
) {
    let bytes = fs::read(path).unwrap_or_default();
    let path_str = path.to_string_lossy().to_string();

    if kind != AssetKind::Mesh && kind != AssetKind::Object && kind != AssetKind::Character {
        let mut st = state.lock();
        st.active_mesh = None;
    }

    let mut ctx = InspectorContext {
        ui_handle,
        state,
        logger,
        gpu_renderer,
        filtered_index,
        path,
        path_str: &path_str,
        bytes: &bytes,
    };

    match kind {
        AssetKind::Texture => inspect_texture(&ctx),
        AssetKind::Mesh => inspect_mesh(&mut ctx),
        AssetKind::Object | AssetKind::Character => inspect_object_or_character(&mut ctx, kind),
        AssetKind::Audio => inspect_audio(&ctx),
        AssetKind::Material => inspect_material(&ctx),
        AssetKind::Lua => inspect_lua(&ctx),
        AssetKind::UI => inspect_ui(&ctx),
        AssetKind::Attachment => inspect_attachment(&ctx),
        AssetKind::Animation => inspect_animation(&ctx),
        AssetKind::TerrainPalette => inspect_terrain_palette(&ctx),
        AssetKind::Collision => inspect_collision(&ctx),
        AssetKind::Environment => inspect_environment(&ctx),
        AssetKind::M8ldMap => inspect_m8ld(&ctx),
        AssetKind::UiSprite => inspect_ui_sprite(&ctx),
        AssetKind::Dta => inspect_dta(&ctx),
        AssetKind::VoicePackage => inspect_vpk(&ctx),
        _ => inspect_generic(&ctx),
    }
}

fn inspect_texture(ctx: &InspectorContext) {
    let mut tex_buf = None;
    let mut tex_desc = String::from("Invalid or corrupted texture");
    let mut ok = false;

    if let Ok(tex) = crate::engine::assets::texture::parse_texture_chunk(ctx.bytes) {
        let rgba = dds_decoder::decode_to_rgba(tex.width, tex.height, tex.format, &tex.pixel_data);
        let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(tex.width, tex.height);
        let dest = buf.make_mut_bytes();
        let copy_len = dest.len().min(rgba.len());
        dest[..copy_len].copy_from_slice(&rgba[..copy_len]);
        tex_buf = Some(buf);
        tex_desc = format!(
            "Resolution: {}x{} | Format: {:?}",
            tex.width, tex.height, tex.format
        );
        ok = true;
    }

    let ui_h = ctx.ui_handle.clone();
    let p_str = ctx.path_str.to_string();
    let filtered_index = ctx.filtered_index;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_selected_index(filtered_index);
            ui.set_active_file_path(p_str.into());
            ui.set_active_kind_id(0);
            if let Some(buf) = tex_buf {
                ui.set_tex_preview(Image::from_rgba8(buf));
            }
            ui.set_tex_info(tex_desc.into());
            ui.set_has_texture(ok);
            ui.set_has_animations(false);
        }
    });
}

fn inspect_mesh(ctx: &mut InspectorContext) {
    let (proj_dir, display_name) = {
        let st = ctx.state.lock();
        let dname = st
            .all_ui_items
            .get(ctx.filtered_index as usize)
            .map(|it| it.display_name.to_string())
            .unwrap_or_default();
        (st.current_proj_dir.clone(), dname)
    };

    let stem = ctx.path.file_stem().unwrap_or_default().to_string_lossy();
    if let Some(mut submesh) =
        preview::load_mesh_with_smart_texture(ctx.bytes, &stem, proj_dir.as_deref())
    {
        let (_, has_composite) = preview::build_composite_mesh_assembly(&stem, proj_dir.as_deref());

        if submesh.bones.is_empty()
            && let Some(master_bones) =
                preview::find_master_skeleton_for_mesh(&stem, proj_dir.as_deref())
        {
            submesh.bones = master_bones;
        }

        let stats_lines = preview::build_stats_lines(std::slice::from_ref(&submesh));
        let mesh_info = format!(
            "Verts: {} | Tris: {}",
            submesh.positions.len(),
            submesh.indices.len() / 3
        );

        let clips =
            preview::discover_companion_animations(&display_name, &stem, proj_dir.as_deref());
        let has_clips = !clips.is_empty();

        let mut clip_names: Vec<slint::SharedString> = Vec::new();
        clip_names.push("0: [REST POSE / BIND POSE]".into());
        for (i, c) in clips.iter().enumerate() {
            clip_names.push(format!("{}: {} ({:.2}s)", i + 1, c.name, c.duration_seconds).into());
        }

        let mut preview = ActiveMeshPreview {
            submeshes: vec![submesh],
            is_composite: false,
            composite_name: stem.to_string(),
            available_clips: clips,
            current_clip_index: None,
            current_time_seconds: 0.0,
            is_playing: false,
            playback_speed: 1.0,
        };

        if let Some(first_submesh) = preview.submeshes.first() {
            let active_clip_ref = preview.available_clips.first();
            let diag_logs = run_deep_skeleton_diagnostics(
                &first_submesh.bones,
                active_clip_ref,
                &first_submesh.positions,
                &first_submesh.joints,
                &first_submesh.weights,
            );

            for line in diag_logs {
                ctx.logger.log(&line);
            }
        }

        let cam = viewport::center_camera_for_preview(ctx.state, &preview);
        {
            let mut st = ctx.state.lock();
            st.active_mesh = Some(preview.clone());
        }

        viewport::evaluate_and_render_animated_frame(
            ctx.ui_handle,
            ctx.state,
            ctx.gpu_renderer,
            &mut preview,
            &cam,
        );

        let ui_h = ctx.ui_handle.clone();
        let p_str = ctx.path_str.to_string();
        let filtered_index = ctx.filtered_index;
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_h.upgrade() {
                ui.set_selected_index(filtered_index);
                ui.set_active_file_path(p_str.into());
                ui.set_active_kind_id(3);
                ui.set_mesh_info(mesh_info.into());
                ui.set_has_composite_available(has_composite);
                ui.set_is_composite_active(false);
                ui.set_has_animations(has_clips);
                ui.set_is_anim_playing(false);
                ui.set_available_animations(ModelRc::from(std::rc::Rc::new(VecModel::from(
                    clip_names,
                ))));
                ui.set_selected_anim_idx(0);
                ui.set_anim_duration(0.0);
                ui.set_anim_current_time(0.0);
                ui.set_mesh_stats_lines(ModelRc::from(std::rc::Rc::new(VecModel::from(
                    stats_lines,
                ))));
                ui.set_has_mesh(true);
            }
        });
    }
}

fn inspect_object_or_character(ctx: &mut InspectorContext, kind: AssetKind) {
    let (proj_dir, display_name) = {
        let st = ctx.state.lock();
        let dname = st
            .all_ui_items
            .get(ctx.filtered_index as usize)
            .map(|it| it.display_name.to_string())
            .unwrap_or_default();
        (st.current_proj_dir.clone(), dname)
    };

    let stem = ctx.path.file_stem().unwrap_or_default().to_string_lossy();
    let (composite_submeshes, has_composite) =
        preview::build_composite_mesh_assembly(&stem, proj_dir.as_deref());

    let json = if kind == AssetKind::Character {
        crate::engine::assets::character::export_character_to_json(ctx.bytes, &stem)
            .unwrap_or_default()
    } else {
        crate::engine::assets::object::export_object_to_json(ctx.bytes).unwrap_or_default()
    };

    let clips = preview::discover_companion_animations(&display_name, &stem, proj_dir.as_deref());
    let has_clips = !clips.is_empty();

    let mut clip_names: Vec<slint::SharedString> = Vec::new();
    clip_names.push("0: [REST POSE / BIND POSE]".into());
    for (i, c) in clips.iter().enumerate() {
        clip_names.push(format!("{}: {} ({:.2}s)", i + 1, c.name, c.duration_seconds).into());
    }

    if has_composite && !composite_submeshes.is_empty() {
        let mut preview = ActiveMeshPreview {
            submeshes: composite_submeshes,
            is_composite: true,
            composite_name: stem.to_string(),
            available_clips: clips,
            current_clip_index: None,
            current_time_seconds: 0.0,
            is_playing: false,
            playback_speed: 1.0,
        };
        let cam = viewport::center_camera_for_preview(ctx.state, &preview);

        {
            let mut st = ctx.state.lock();
            st.active_mesh = Some(preview.clone());
        }

        viewport::evaluate_and_render_animated_frame(
            ctx.ui_handle,
            ctx.state,
            ctx.gpu_renderer,
            &mut preview,
            &cam,
        );

        let ui_h = ctx.ui_handle.clone();
        let p_str = ctx.path_str.to_string();
        let filtered_index = ctx.filtered_index;
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_h.upgrade() {
                ui.set_selected_index(filtered_index);
                ui.set_active_file_path(p_str.into());
                ui.set_active_kind_id(7);
                ui.set_mat_json_text(json.into());
                ui.set_has_mesh(true);
                ui.set_has_composite_available(true);
                ui.set_is_composite_active(true);
                ui.set_has_animations(has_clips);
                ui.set_is_anim_playing(false);
                ui.set_available_animations(ModelRc::from(std::rc::Rc::new(VecModel::from(
                    clip_names,
                ))));
                ui.set_selected_anim_idx(0);
                ui.set_anim_duration(0.0);
                ui.set_anim_current_time(0.0);
            }
        });
    } else {
        let ui_h = ctx.ui_handle.clone();
        let p_str = ctx.path_str.to_string();
        let filtered_index = ctx.filtered_index;
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_h.upgrade() {
                ui.set_selected_index(filtered_index);
                ui.set_active_file_path(p_str.into());
                ui.set_active_kind_id(7);
                ui.set_mat_json_text(json.into());
                ui.set_has_composite_available(false);
                ui.set_has_animations(false);
            }
        });
    }
}

fn inspect_audio(ctx: &InspectorContext) {
    let ui_h = ctx.ui_handle.clone();
    let p_str = ctx.path_str.to_string();
    let filtered_index = ctx.filtered_index;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_selected_index(filtered_index);
            ui.set_active_file_path(p_str.into());
            ui.set_active_kind_id(1);
            ui.set_has_animations(false);
        }
    });
}

fn inspect_material(ctx: &InspectorContext) {
    let json =
        crate::engine::assets::material::export_material_to_json(ctx.bytes).unwrap_or_default();
    let ui_h = ctx.ui_handle.clone();
    let p_str = ctx.path_str.to_string();
    let filtered_index = ctx.filtered_index;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_selected_index(filtered_index);
            ui.set_active_file_path(p_str.into());
            ui.set_active_kind_id(2);
            ui.set_mat_json_text(json.into());
            ui.set_has_animations(false);
        }
    });
}

fn inspect_lua(ctx: &InspectorContext) {
    let disasm =
        crate::engine::assets::lua::disassemble_lua_bytecode(ctx.bytes).unwrap_or_default();
    let ui_h = ctx.ui_handle.clone();
    let p_str = ctx.path_str.to_string();
    let filtered_index = ctx.filtered_index;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_selected_index(filtered_index);
            ui.set_active_file_path(p_str.into());
            ui.set_active_kind_id(4);
            ui.set_mat_json_text(disasm.into());
            ui.set_has_animations(false);
        }
    });
}

fn inspect_ui(ctx: &InspectorContext) {
    let json = crate::engine::assets::ui::export_ui_to_json(ctx.bytes).unwrap_or_default();
    let ui_h = ctx.ui_handle.clone();
    let p_str = ctx.path_str.to_string();
    let filtered_index = ctx.filtered_index;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_selected_index(filtered_index);
            ui.set_active_file_path(p_str.into());
            ui.set_active_kind_id(6);
            ui.set_mat_json_text(json.into());
            ui.set_has_animations(false);
        }
    });
}

fn inspect_attachment(ctx: &InspectorContext) {
    let json =
        crate::engine::assets::attachment::export_attachment_to_json(ctx.bytes).unwrap_or_default();
    let ui_h = ctx.ui_handle.clone();
    let p_str = ctx.path_str.to_string();
    let filtered_index = ctx.filtered_index;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_selected_index(filtered_index);
            ui.set_active_file_path(p_str.into());
            ui.set_active_kind_id(7);
            ui.set_mat_json_text(json.into());
            ui.set_has_animations(false);
        }
    });
}

fn inspect_animation(ctx: &InspectorContext) {
    let mut info = String::new();
    if let Ok(clip) = crate::engine::assets::animation::parse_animation_clip(ctx.bytes) {
        info = format!(
            "Clip: {} | Rig: {} | {:.1} FPS | Duration: {:.3}s | Tracks: {}",
            clip.name,
            clip.target_rig,
            clip.frame_rate,
            clip.duration_seconds,
            clip.bone_tracks.len()
        );
    }
    let json =
        crate::engine::assets::animation::export_animation_to_json(ctx.bytes).unwrap_or_default();
    let ui_h = ctx.ui_handle.clone();
    let p_str = ctx.path_str.to_string();
    let filtered_index = ctx.filtered_index;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_selected_index(filtered_index);
            ui.set_active_file_path(p_str.into());
            ui.set_active_kind_id(8);
            ui.set_mesh_info(info.into());
            ui.set_mat_json_text(json.into());
            ui.set_has_animations(false);
        }
    });
}

fn inspect_terrain_palette(ctx: &InspectorContext) {
    let json = crate::engine::assets::terrain_palette::export_terrain_palette_to_json(ctx.bytes)
        .unwrap_or_default();
    let ui_h = ctx.ui_handle.clone();
    let p_str = ctx.path_str.to_string();
    let filtered_index = ctx.filtered_index;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_selected_index(filtered_index);
            ui.set_active_file_path(p_str.into());
            ui.set_active_kind_id(9);
            ui.set_mat_json_text(json.into());
            ui.set_has_animations(false);
        }
    });
}

fn inspect_collision(ctx: &InspectorContext) {
    let json = crate::engine::assets::collision::export_collision_to_json(ctx.bytes, "")
        .unwrap_or_default();
    let ui_h = ctx.ui_handle.clone();
    let p_str = ctx.path_str.to_string();
    let filtered_index = ctx.filtered_index;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_selected_index(filtered_index);
            ui.set_active_file_path(p_str.into());
            ui.set_active_kind_id(10);
            ui.set_mat_json_text(json.into());
            ui.set_has_animations(false);
        }
    });
}

fn inspect_environment(ctx: &InspectorContext) {
    let json = crate::engine::assets::environment::export_environment_to_json(ctx.bytes)
        .unwrap_or_default();
    let ui_h = ctx.ui_handle.clone();
    let p_str = ctx.path_str.to_string();
    let filtered_index = ctx.filtered_index;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_selected_index(filtered_index);
            ui.set_active_file_path(p_str.into());
            ui.set_active_kind_id(11);
            ui.set_mat_json_text(json.into());
            ui.set_has_animations(false);
        }
    });
}

fn inspect_m8ld(ctx: &InspectorContext) {
    let (_seed, xml_str) = crate::engine::assets::m8ld::decompile_8ld_to_xml(ctx.bytes).unwrap_or_else(|_| {
        (0x91, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Language id=\"English\">\n</Language>".into())
    });
    let ui_h = ctx.ui_handle.clone();
    let p_str = ctx.path_str.to_string();
    let filtered_index = ctx.filtered_index;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_selected_index(filtered_index);
            ui.set_active_file_path(p_str.into());
            ui.set_active_kind_id(12);
            ui.set_mat_json_text(xml_str.into());
            ui.set_has_animations(false);
        }
    });
}

fn inspect_ui_sprite(ctx: &InspectorContext) {
    let json = crate::engine::assets::ui_sprite::export_ui_sprite_collection(ctx.bytes)
        .unwrap_or_else(|e| format!("Error parsing Sprite Collection: {}", e));
    let ui_h = ctx.ui_handle.clone();
    let p_str = ctx.path_str.to_string();
    let filtered_index = ctx.filtered_index;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_selected_index(filtered_index);
            ui.set_active_file_path(p_str.into());
            ui.set_active_kind_id(13);
            ui.set_mat_json_text(json.into());
            ui.set_has_animations(false);
        }
    });
}

fn inspect_dta(ctx: &InspectorContext) {
    let stem = ctx.path.file_stem().unwrap_or_default().to_string_lossy();
    let json = crate::engine::assets::dta::export_dta_to_json(ctx.bytes, &stem)
        .unwrap_or_else(|e| format!("Error decoding Lighting Set: {}", e));
    let ui_h = ctx.ui_handle.clone();
    let p_str = ctx.path_str.to_string();
    let filtered_index = ctx.filtered_index;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_selected_index(filtered_index);
            ui.set_active_file_path(p_str.into());
            ui.set_active_kind_id(14);
            ui.set_mat_json_text(json.into());
            ui.set_has_animations(false);
        }
    });
}

fn inspect_vpk(ctx: &InspectorContext) {
    let json = crate::engine::assets::vpk::export_vpk_to_json(ctx.bytes)
        .unwrap_or_else(|e| format!("Error decoding Voice Package: {}", e));
    let ui_h = ctx.ui_handle.clone();
    let p_str = ctx.path_str.to_string();
    let filtered_index = ctx.filtered_index;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_selected_index(filtered_index);
            ui.set_active_file_path(p_str.into());
            ui.set_active_kind_id(15);
            ui.set_mat_json_text(json.into());
            ui.set_has_animations(false);
        }
    });
}

fn inspect_generic(ctx: &InspectorContext) {
    let ui_h = ctx.ui_handle.clone();
    let p_str = ctx.path_str.to_string();
    let filtered_index = ctx.filtered_index;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_selected_index(filtered_index);
            ui.set_active_file_path(p_str.into());
            ui.set_active_kind_id(5);
            ui.set_has_animations(false);
        }
    });
}
