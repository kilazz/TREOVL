use parking_lot::Mutex;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use slint::{Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

use crate::AppWindow;
use crate::engine::assets::animation::run_deep_skeleton_diagnostics;
use crate::engine::assets::character::export_character;
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

    // Resolve and populate interactive Asset Links from dependency graph
    let links: Vec<crate::AssetLinkItem> = {
        let mut st = state.lock();
        if st.dependency_graph.is_none()
            && let Some(ref proj) = st.current_proj_dir
        {
            let graph_path = proj.join("assets").join("dependency_graph.json");
            if graph_path.exists()
                && let Ok(content) = fs::read_to_string(&graph_path)
                && let Ok(g) = serde_json::from_str(&content)
            {
                st.dependency_graph = Some(g);
            }
        }

        if let Some(ref graph) = st.dependency_graph {
            let stem = path.file_stem().unwrap_or_default().to_string_lossy();
            graph
                .find_links_for_asset(&stem)
                .into_iter()
                .map(|l| crate::AssetLinkItem {
                    label: l.label.into(),
                    target_name: l.target_name.into(),
                    category: l.category.into(),
                    icon: l.icon.into(),
                })
                .collect()
        } else {
            Vec::new()
        }
    };

    let ui_links = ui_handle.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_links.upgrade() {
            ui.set_active_asset_links(ModelRc::from(std::rc::Rc::new(VecModel::from(links))));
        }
    });

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
            ui.set_has_rigs(false);
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

        // Discover all project rigs (embedded mesh rig, objects/*.json, and chunks/*.bin)
        let discovered_rigs =
            preview::discover_all_project_rigs(&submesh.bones, proj_dir.as_deref());
        let has_rigs = !discovered_rigs.is_empty();

        if submesh.bones.is_empty()
            && let Some(first_rig) = discovered_rigs.first()
        {
            submesh.bones = first_rig.bones.clone();
        }

        let mut rig_names: Vec<slint::SharedString> = Vec::new();
        for (i, r) in discovered_rigs.iter().enumerate() {
            rig_names.push(format!("{}: {}", i + 1, r.name).into());
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
            available_rigs: discovered_rigs,
            current_rig_index: if has_rigs { Some(0) } else { None },
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
                ui.set_has_rigs(has_rigs);
                ui.set_available_rigs(ModelRc::from(std::rc::Rc::new(VecModel::from(rig_names))));
                ui.set_selected_rig_idx(0);
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

    let mut char_opt = None;
    let json = if kind == AssetKind::Character {
        if let Ok(extracted) = export_character(ctx.bytes, &stem) {
            if let Some(ref handle) = extracted.character.entity_handle {
                ctx.logger.log(&format!(
                    "[*] Recognized Map Entity UID: {}#{} (Handle: {})",
                    handle.domain_tag, handle.uid, handle.raw_hex
                ));
            }
            char_opt = Some(extracted.character.clone());
            serde_json::to_string_pretty(&extracted.character).unwrap_or_default()
        } else {
            String::new()
        }
    } else if let Ok(extracted) = crate::engine::assets::object::export_object(ctx.bytes) {
        if let Some(ref handle) = extracted.entity.entity_handle {
            ctx.logger.log(&format!(
                "[*] Recognized Map Entity UID: {}#{} (Handle: {})",
                handle.domain_tag, handle.uid, handle.raw_hex
            ));
        }
        serde_json::to_string_pretty(&extracted.entity).unwrap_or_default()
    } else {
        String::new()
    };

    let clips = preview::discover_companion_animations(&display_name, &stem, proj_dir.as_deref());
    let has_clips = !clips.is_empty();

    let mut clip_names: Vec<slint::SharedString> = Vec::new();
    clip_names.push("0: [REST POSE / BIND POSE]".into());
    for (i, c) in clips.iter().enumerate() {
        clip_names.push(format!("{}: {} ({:.2}s)", i + 1, c.name, c.duration_seconds).into());
    }

    let discovered_rigs = preview::discover_all_project_rigs(
        composite_submeshes
            .first()
            .map(|s| s.bones.as_slice())
            .unwrap_or(&[]),
        proj_dir.as_deref(),
    );
    let has_rigs = !discovered_rigs.is_empty();

    let mut rig_names: Vec<slint::SharedString> = Vec::new();
    for (i, r) in discovered_rigs.iter().enumerate() {
        rig_names.push(format!("{}: {}", i + 1, r.name).into());
    }

    let active_kind_id = if kind == AssetKind::Character { 18 } else { 7 };

    if has_composite && !composite_submeshes.is_empty() {
        let mut preview = ActiveMeshPreview {
            submeshes: composite_submeshes,
            is_composite: true,
            composite_name: stem.to_string(),
            available_clips: clips,
            current_clip_index: None,
            available_rigs: discovered_rigs,
            current_rig_index: if has_rigs { Some(0) } else { None },
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
                ui.set_active_kind_id(active_kind_id);
                ui.set_mat_json_text(json.into());

                if let Some(ref char_data) = char_opt {
                    ui.set_form_char_name(char_data.character_name.clone().into());
                    ui.set_form_char_tag(char_data.resource_tag.clone().unwrap_or_default().into());
                    if let Some(ref a) = char_data.combat_attributes {
                        ui.set_form_char_health(a.base_health.unwrap_or(100.0));
                        ui.set_form_char_speed(a.move_speed_scale.unwrap_or(1.0));
                        ui.set_form_char_aggro(a.aggro_range.unwrap_or(15.0));
                        ui.set_form_char_perception(a.perception_radius.unwrap_or(20.0));
                        ui.set_form_char_damage_mult(a.damage_multiplier.unwrap_or(1.0));
                        ui.set_form_char_faction_id(a.faction_id.unwrap_or(0) as i32);
                        ui.set_form_char_can_swim(a.can_swim.unwrap_or(false));
                    }
                    if let Some(ref f) = char_data.actor_flags {
                        ui.set_form_char_shadows(f.casts_dynamic_shadows);
                        ui.set_form_char_targetable(f.can_be_targeted);
                        ui.set_form_char_ragdoll(f.ragdoll_on_death);
                    }
                }

                ui.set_has_mesh(true);
                ui.set_has_composite_available(true);
                ui.set_is_composite_active(true);
                ui.set_has_rigs(has_rigs);
                ui.set_available_rigs(ModelRc::from(std::rc::Rc::new(VecModel::from(rig_names))));
                ui.set_selected_rig_idx(0);
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
                ui.set_active_kind_id(active_kind_id);
                ui.set_mat_json_text(json.into());

                if let Some(ref char_data) = char_opt {
                    ui.set_form_char_name(char_data.character_name.clone().into());
                    ui.set_form_char_tag(char_data.resource_tag.clone().unwrap_or_default().into());
                    if let Some(ref a) = char_data.combat_attributes {
                        ui.set_form_char_health(a.base_health.unwrap_or(100.0));
                        ui.set_form_char_speed(a.move_speed_scale.unwrap_or(1.0));
                        ui.set_form_char_aggro(a.aggro_range.unwrap_or(15.0));
                        ui.set_form_char_perception(a.perception_radius.unwrap_or(20.0));
                        ui.set_form_char_damage_mult(a.damage_multiplier.unwrap_or(1.0));
                        ui.set_form_char_faction_id(a.faction_id.unwrap_or(0) as i32);
                        ui.set_form_char_can_swim(a.can_swim.unwrap_or(false));
                    }
                    if let Some(ref f) = char_data.actor_flags {
                        ui.set_form_char_shadows(f.casts_dynamic_shadows);
                        ui.set_form_char_targetable(f.can_be_targeted);
                        ui.set_form_char_ragdoll(f.ragdoll_on_death);
                    }
                }

                ui.set_has_composite_available(false);
                ui.set_has_animations(false);
                ui.set_has_rigs(false);
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
            ui.set_has_rigs(false);
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
            ui.set_has_rigs(false);
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
            ui.set_has_rigs(false);
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
            ui.set_has_rigs(false);
        }
    });
}

fn inspect_attachment(ctx: &InspectorContext) {
    let json =
        crate::engine::assets::attachment::export_attachment_to_json(ctx.bytes).unwrap_or_default();
    let item_opt: Option<crate::engine::assets::attachment::ItemAttachmentJson> =
        serde_json::from_str(&json).ok();

    let ui_h = ctx.ui_handle.clone();
    let p_str = ctx.path_str.to_string();
    let filtered_index = ctx.filtered_index;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_h.upgrade() {
            ui.set_selected_index(filtered_index);
            ui.set_active_file_path(p_str.into());
            ui.set_active_kind_id(16);
            ui.set_mat_json_text(json.into());

            if let Some(ref item) = item_opt {
                ui.set_form_item_name(item.item_name.clone().into());
                ui.set_form_item_mesh_pkg(item.mesh_package.clone().into());
                ui.set_form_item_submesh(item.submesh_name.clone().into());
                ui.set_form_item_sound_bank(item.sound_bank.clone().into());
                ui.set_form_item_mount_point(item.socket.mount_point.clone().into());
                ui.set_form_item_pickable(item.flags.is_pickable);
                ui.set_form_item_shadows(item.flags.cast_shadows);
                ui.set_form_item_physics(item.flags.drop_physics);
                if let Some(ref w) = item.weapon_config {
                    ui.set_form_item_damage(w.damage_multiplier.unwrap_or(1.0));
                    ui.set_form_item_range(w.attack_range.unwrap_or(2.5));
                    ui.set_form_item_speed_mod(w.speed_modifier.unwrap_or(1.0));
                }
            }

            ui.set_has_animations(false);
            ui.set_has_rigs(false);
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
            ui.set_has_rigs(false);
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
            ui.set_has_rigs(false);
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
            ui.set_has_rigs(false);
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
            ui.set_has_rigs(false);
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
            ui.set_has_rigs(false);
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
            ui.set_has_rigs(false);
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
            ui.set_has_rigs(false);
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
            ui.set_has_rigs(false);
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
            ui.set_has_rigs(false);
        }
    });
}
