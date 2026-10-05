use slint::{Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc::Receiver};

use crate::AppWindow;
use crate::engine::assets::animation::{
    AnimationClip, ObjectBone, apply_skeletal_skinning, compute_skinning_matrices,
    parse_animation_clip, parse_object_bone_container,
};
use crate::engine::assets::sniffer::AssetKind;
use crate::engine::math::Vector3;
use crate::engine::service;
use crate::gui::commands::WorkerCommand;
use crate::gui::{
    ActiveMeshPreview, AppState, RenderSubmesh, resolve_project_dir, scan_project_folder,
};
use crate::utils::dds_decoder;
use crate::utils::logger::UiLogger;
use crate::utils::renderer::{SubmeshDrawData, TextureData, ViewportCamera, WgpuRenderer};

pub struct ResolvedTexture {
    pub filename: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct BackgroundWorker {
    ui_handle: slint::Weak<AppWindow>,
    logger: UiLogger,
    state: Arc<Mutex<AppState>>,
    gpu_renderer: Option<WgpuRenderer>,
}

impl BackgroundWorker {
    pub fn new(
        ui_handle: slint::Weak<AppWindow>,
        logger: UiLogger,
        state: Arc<Mutex<AppState>>,
    ) -> Self {
        let gpu_renderer = Some(WgpuRenderer::new());

        Self {
            ui_handle,
            logger,
            state,
            gpu_renderer,
        }
    }

    pub fn run(mut self, rx: Receiver<WorkerCommand>) {
        while let Ok(cmd) = rx.recv() {
            match cmd {
                WorkerCommand::RotateMeshViewport {
                    mut delta_yaw,
                    mut delta_pitch,
                } => {
                    let mut extra_commands = Vec::new();
                    while let Ok(next_cmd) = rx.try_recv() {
                        match next_cmd {
                            WorkerCommand::RotateMeshViewport {
                                delta_yaw: dy,
                                delta_pitch: dp,
                            } => {
                                delta_yaw += dy;
                                delta_pitch += dp;
                            }
                            other => {
                                extra_commands.push(other);
                                break;
                            }
                        }
                    }

                    self.handle_command(WorkerCommand::RotateMeshViewport {
                        delta_yaw,
                        delta_pitch,
                    });

                    for other_cmd in extra_commands {
                        self.handle_command(other_cmd);
                    }
                }
                other => {
                    self.handle_command(other);
                }
            }
        }
    }

    pub fn handle_command(&mut self, cmd: WorkerCommand) {
        match cmd {
            WorkerCommand::FilterAssets { query, generation } => {
                let q = query.trim().to_lowercase();
                let filtered_ui = {
                    let mut st = self.state.lock().unwrap();
                    if generation < st.filter_generation {
                        return;
                    }
                    st.filter_generation = generation;

                    let mut visible = Vec::new();
                    let mut ui_items = Vec::new();

                    for (i, token) in st.all_search_haystack.iter().enumerate() {
                        if q.is_empty() || token.contains(&q) {
                            visible.push(i);
                            if let Some(item) = st.all_ui_items.get(i) {
                                ui_items.push(item.clone());
                            }
                        }
                    }
                    st.visible_indices = visible;
                    ui_items
                };

                let ui_h = self.ui_handle.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_h.upgrade() {
                        ui.set_asset_list(ModelRc::from(std::rc::Rc::new(VecModel::from(
                            filtered_ui,
                        ))));
                        ui.set_selected_index(-1);
                    }
                });
            }

            WorkerCommand::SelectAsset {
                filtered_index,
                path,
                kind,
            } => {
                let bytes = fs::read(&path).unwrap_or_default();
                let path_str = path.to_string_lossy().to_string();

                if kind != AssetKind::Mesh
                    && kind != AssetKind::Object
                    && kind != AssetKind::Character
                {
                    let mut st = self.state.lock().unwrap();
                    st.active_mesh = None;
                }

                match kind {
                    AssetKind::Texture => {
                        let mut tex_buf = None;
                        let mut tex_desc = String::from("Invalid or corrupted texture");
                        let mut ok = false;

                        if let Ok(tex) = crate::engine::assets::texture::parse_texture_chunk(&bytes)
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
                            tex_buf = Some(buf);
                            tex_desc = format!(
                                "Resolution: {}x{} | Format: {:?}",
                                tex.width, tex.height, tex.format
                            );
                            ok = true;
                        }

                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
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
                    AssetKind::Mesh => {
                        let (proj_dir, display_name) = {
                            let st = self.state.lock().unwrap();
                            let dname = st
                                .all_ui_items
                                .get(filtered_index as usize)
                                .map(|it| it.display_name.to_string())
                                .unwrap_or_default();
                            (st.current_proj_dir.clone(), dname)
                        };

                        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                        if let Some(mut submesh) =
                            load_mesh_with_smart_texture(&bytes, &stem, proj_dir.as_deref())
                        {
                            let (_, has_composite) =
                                build_composite_mesh_assembly(&stem, proj_dir.as_deref());

                            // If submesh has no bones, find master skeleton in the project
                            if submesh.bones.is_empty()
                                && let Some(master_bones) =
                                    find_master_skeleton(proj_dir.as_deref())
                            {
                                submesh.bones = master_bones;
                            }

                            let stats_lines = build_stats_lines(std::slice::from_ref(&submesh));
                            let mesh_info = format!(
                                "Verts: {} | Tris: {}",
                                submesh.positions.len(),
                                submesh.indices.len() / 3
                            );

                            let clips = discover_companion_animations(
                                &display_name,
                                &stem,
                                proj_dir.as_deref(),
                            );
                            let has_clips = !clips.is_empty();
                            let clip_names: Vec<slint::SharedString> = clips
                                .iter()
                                .enumerate()
                                .map(|(i, c)| {
                                    format!("{}: {} ({:.2}s)", i + 1, c.name, c.duration_seconds)
                                        .into()
                                })
                                .collect();

                            let mut preview = ActiveMeshPreview {
                                submeshes: vec![submesh],
                                is_composite: false,
                                composite_name: stem.to_string(),
                                available_clips: clips,
                                current_clip_index: if has_clips { Some(0) } else { None },
                                current_time_seconds: 0.0,
                                is_playing: false,
                                playback_speed: 1.0,
                            };

                            let cam = self.center_camera_for_preview(&preview);
                            let initial_duration = preview
                                .available_clips
                                .first()
                                .map(|c| c.duration_seconds)
                                .unwrap_or(1.0);

                            {
                                let mut st = self.state.lock().unwrap();
                                st.active_mesh = Some(preview.clone());
                            }

                            self.evaluate_and_render_animated_frame(&mut preview, &cam);

                            let ui_h = self.ui_handle.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_h.upgrade() {
                                    ui.set_selected_index(filtered_index);
                                    ui.set_active_file_path(path_str.into());
                                    ui.set_active_kind_id(3);
                                    ui.set_mesh_info(mesh_info.into());
                                    ui.set_has_composite_available(has_composite);
                                    ui.set_is_composite_active(false);
                                    ui.set_has_animations(has_clips);
                                    ui.set_is_anim_playing(false);
                                    ui.set_available_animations(ModelRc::from(std::rc::Rc::new(
                                        VecModel::from(clip_names),
                                    )));
                                    ui.set_selected_anim_idx(0);
                                    ui.set_anim_duration(initial_duration);
                                    ui.set_anim_current_time(0.0);
                                    ui.set_mesh_stats_lines(ModelRc::from(std::rc::Rc::new(
                                        VecModel::from(stats_lines),
                                    )));
                                    ui.set_has_mesh(true);
                                }
                            });
                        }
                    }
                    AssetKind::Object | AssetKind::Character => {
                        let (proj_dir, display_name) = {
                            let st = self.state.lock().unwrap();
                            let dname = st
                                .all_ui_items
                                .get(filtered_index as usize)
                                .map(|it| it.display_name.to_string())
                                .unwrap_or_default();
                            (st.current_proj_dir.clone(), dname)
                        };

                        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                        let (composite_submeshes, has_composite) =
                            build_composite_mesh_assembly(&stem, proj_dir.as_deref());

                        let json = if kind == AssetKind::Character {
                            crate::engine::assets::character::export_character_to_json(
                                &bytes, None, &stem,
                            )
                            .unwrap_or_default()
                        } else {
                            crate::engine::assets::object::export_object_to_json(&bytes, None)
                                .unwrap_or_default()
                        };

                        let clips = discover_companion_animations(
                            &display_name,
                            &stem,
                            proj_dir.as_deref(),
                        );
                        let has_clips = !clips.is_empty();
                        let clip_names: Vec<slint::SharedString> = clips
                            .iter()
                            .enumerate()
                            .map(|(i, c)| {
                                format!("{}: {} ({:.2}s)", i + 1, c.name, c.duration_seconds).into()
                            })
                            .collect();

                        if has_composite && !composite_submeshes.is_empty() {
                            let mut preview = ActiveMeshPreview {
                                submeshes: composite_submeshes,
                                is_composite: true,
                                composite_name: stem.to_string(),
                                available_clips: clips,
                                current_clip_index: if has_clips { Some(0) } else { None },
                                current_time_seconds: 0.0,
                                is_playing: false,
                                playback_speed: 1.0,
                            };
                            let cam = self.center_camera_for_preview(&preview);
                            let initial_duration = preview
                                .available_clips
                                .first()
                                .map(|c| c.duration_seconds)
                                .unwrap_or(1.0);

                            {
                                let mut st = self.state.lock().unwrap();
                                st.active_mesh = Some(preview.clone());
                            }

                            self.evaluate_and_render_animated_frame(&mut preview, &cam);

                            let ui_h = self.ui_handle.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_h.upgrade() {
                                    ui.set_selected_index(filtered_index);
                                    ui.set_active_file_path(path_str.into());
                                    ui.set_active_kind_id(7);
                                    ui.set_mat_json_text(json.into());
                                    ui.set_has_mesh(true);
                                    ui.set_has_composite_available(true);
                                    ui.set_is_composite_active(true);
                                    ui.set_has_animations(has_clips);
                                    ui.set_is_anim_playing(false);
                                    ui.set_available_animations(ModelRc::from(std::rc::Rc::new(
                                        VecModel::from(clip_names),
                                    )));
                                    ui.set_selected_anim_idx(0);
                                    ui.set_anim_duration(initial_duration);
                                    ui.set_anim_current_time(0.0);
                                }
                            });
                        } else {
                            let ui_h = self.ui_handle.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_h.upgrade() {
                                    ui.set_selected_index(filtered_index);
                                    ui.set_active_file_path(path_str.into());
                                    ui.set_active_kind_id(7);
                                    ui.set_mat_json_text(json.into());
                                    ui.set_has_composite_available(false);
                                    ui.set_has_animations(false);
                                }
                            });
                        }
                    }
                    AssetKind::Audio => {
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(1);
                                ui.set_has_animations(false);
                            }
                        });
                    }
                    AssetKind::Material => {
                        let json = crate::engine::assets::material::export_material_to_json(&bytes)
                            .unwrap_or_default();
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(2);
                                ui.set_mat_json_text(json.into());
                                ui.set_has_animations(false);
                            }
                        });
                    }
                    AssetKind::Lua => {
                        let disasm = crate::engine::assets::lua::disassemble_lua_bytecode(&bytes)
                            .unwrap_or_default();
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(4);
                                ui.set_mat_json_text(disasm.into());
                                ui.set_has_animations(false);
                            }
                        });
                    }
                    AssetKind::UI => {
                        let json = crate::engine::assets::ui::export_ui_to_json(&bytes)
                            .unwrap_or_default();
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(6);
                                ui.set_mat_json_text(json.into());
                                ui.set_has_animations(false);
                            }
                        });
                    }
                    AssetKind::Attachment => {
                        let json =
                            crate::engine::assets::attachment::export_attachment_to_json(&bytes)
                                .unwrap_or_default();
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(7);
                                ui.set_mat_json_text(json.into());
                                ui.set_has_animations(false);
                            }
                        });
                    }
                    AssetKind::Animation => {
                        let mut info = String::new();
                        if let Ok(clip) =
                            crate::engine::assets::animation::parse_animation_clip(&bytes)
                        {
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
                            crate::engine::assets::animation::export_animation_to_json(&bytes)
                                .unwrap_or_default();
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(8);
                                ui.set_mesh_info(info.into());
                                ui.set_mat_json_text(json.into());
                                ui.set_has_animations(false);
                            }
                        });
                    }
                    AssetKind::TerrainPalette => {
                        let json =
                            crate::engine::assets::terrain_palette::export_terrain_palette_to_json(
                                &bytes,
                            )
                            .unwrap_or_default();
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(9);
                                ui.set_mat_json_text(json.into());
                                ui.set_has_animations(false);
                            }
                        });
                    }
                    AssetKind::Collision => {
                        let json =
                            crate::engine::assets::collision::export_collision_to_json(&bytes, "")
                                .unwrap_or_default();
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(10);
                                ui.set_mat_json_text(json.into());
                                ui.set_has_animations(false);
                            }
                        });
                    }
                    AssetKind::Environment => {
                        let json =
                            crate::engine::assets::environment::export_environment_to_json(&bytes)
                                .unwrap_or_default();
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(11);
                                ui.set_mat_json_text(json.into());
                                ui.set_has_animations(false);
                            }
                        });
                    }
                    AssetKind::M8ldMap => {
                        let (_seed, xml_str) = crate::engine::assets::m8ld::decompile_8ld_to_xml(&bytes)
                            .unwrap_or_else(|_| (0x91, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Language id=\"English\">\n</Language>".into()));
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(12);
                                ui.set_mat_json_text(xml_str.into());
                                ui.set_has_animations(false);
                            }
                        });
                    }
                    AssetKind::UiSprite => {
                        let json =
                            crate::engine::assets::ui_sprite::export_ui_sprite_collection(&bytes)
                                .unwrap_or_else(|e| {
                                    format!("Error parsing Sprite Collection: {}", e)
                                });
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(13);
                                ui.set_mat_json_text(json.into());
                                ui.set_has_animations(false);
                            }
                        });
                    }
                    AssetKind::Dta => {
                        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                        let json = crate::engine::assets::dta::export_dta_to_json(&bytes, &stem)
                            .unwrap_or_else(|e| format!("Error decoding Lighting Set: {}", e));
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(14);
                                ui.set_mat_json_text(json.into());
                                ui.set_has_animations(false);
                            }
                        });
                    }
                    AssetKind::VoicePackage => {
                        let json = crate::engine::assets::vpk::export_vpk_to_json(&bytes)
                            .unwrap_or_else(|e| format!("Error decoding Voice Package: {}", e));
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(15);
                                ui.set_mat_json_text(json.into());
                                ui.set_has_animations(false);
                            }
                        });
                    }
                    _ => {
                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_selected_index(filtered_index);
                                ui.set_active_file_path(path_str.into());
                                ui.set_active_kind_id(5);
                                ui.set_has_animations(false);
                            }
                        });
                    }
                }
            }

            WorkerCommand::SelectAnimation { clip_index } => {
                let mut re_render_opt = None;
                {
                    let mut st = self.state.lock().unwrap();
                    if let Some(ref mut preview) = st.active_mesh
                        && clip_index >= 0
                        && (clip_index as usize) < preview.available_clips.len()
                    {
                        preview.current_clip_index = Some(clip_index as usize);
                        preview.current_time_seconds = 0.0;
                        let clip = &preview.available_clips[clip_index as usize];
                        let duration = clip.duration_seconds;

                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_anim_duration(duration);
                                ui.set_anim_current_time(0.0);
                            }
                        });

                        re_render_opt = Some((preview.clone(), st.camera));
                    }
                }

                if let Some((mut preview, cam)) = re_render_opt {
                    self.evaluate_and_render_animated_frame(&mut preview, &cam);
                }
            }

            WorkerCommand::SetAnimationTime { time_seconds } => {
                let mut re_render_opt = None;
                {
                    let mut st = self.state.lock().unwrap();
                    if let Some(ref mut preview) = st.active_mesh {
                        preview.current_time_seconds = time_seconds;
                        re_render_opt = Some((preview.clone(), st.camera));
                    }
                }
                if let Some((mut preview, cam)) = re_render_opt {
                    self.evaluate_and_render_animated_frame(&mut preview, &cam);
                }
            }

            WorkerCommand::TickAnimationPlayback { delta_seconds } => {
                let mut re_render_opt = None;
                let mut time_ui = 0.0f32;
                {
                    let mut st = self.state.lock().unwrap();
                    if let Some(ref mut preview) = st.active_mesh
                        && let Some(c_idx) = preview.current_clip_index
                    {
                        let duration = preview.available_clips[c_idx].duration_seconds.max(0.01);
                        preview.current_time_seconds = (preview.current_time_seconds
                            + delta_seconds * preview.playback_speed)
                            % duration;
                        time_ui = preview.current_time_seconds;
                        re_render_opt = Some((preview.clone(), st.camera));
                    }
                }

                let ui_h = self.ui_handle.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_h.upgrade() {
                        ui.set_anim_current_time(time_ui);
                    }
                });

                if let Some((mut preview, cam)) = re_render_opt {
                    self.evaluate_and_render_animated_frame(&mut preview, &cam);
                }
            }

            WorkerCommand::ToggleCompositeView => {
                let (proj_dir, active_preview_opt) = {
                    let st = self.state.lock().unwrap();
                    (st.current_proj_dir.clone(), st.active_mesh.clone())
                };

                if let Some(mut preview) = active_preview_opt {
                    let stem = preview.composite_name.clone();
                    let (composite_submeshes, has_composite) =
                        build_composite_mesh_assembly(&stem, proj_dir.as_deref());

                    if has_composite && !composite_submeshes.is_empty() {
                        preview.is_composite = !preview.is_composite;
                        if preview.is_composite {
                            preview.submeshes = composite_submeshes;
                        } else {
                            preview.submeshes.truncate(1);
                        }

                        let cam = self.center_camera_for_preview(&preview);
                        let is_comp_active = preview.is_composite;
                        let stats_lines = build_stats_lines(&preview.submeshes);

                        {
                            let mut st = self.state.lock().unwrap();
                            st.active_mesh = Some(preview.clone());
                        }

                        self.evaluate_and_render_animated_frame(&mut preview, &cam);

                        let ui_h = self.ui_handle.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_is_composite_active(is_comp_active);
                                ui.set_mesh_stats_lines(ModelRc::from(std::rc::Rc::new(
                                    VecModel::from(stats_lines),
                                )));
                            }
                        });
                    }
                }
            }

            WorkerCommand::ToggleSkinning => {
                let mut re_render_opt = None;
                let is_enabled = {
                    let mut st = self.state.lock().unwrap();
                    st.is_skinning_enabled = !st.is_skinning_enabled;
                    if let Some(ref mut preview) = st.active_mesh {
                        re_render_opt = Some((preview.clone(), st.camera));
                    }
                    st.is_skinning_enabled
                };

                let ui_h = self.ui_handle.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_h.upgrade() {
                        ui.set_is_skinning_enabled(is_enabled);
                    }
                });

                if let Some((mut preview, cam)) = re_render_opt {
                    self.evaluate_and_render_animated_frame(&mut preview, &cam);
                }
            }

            WorkerCommand::ToggleRootMotion => {
                let mut re_render_opt = None;
                let is_enabled = {
                    let mut st = self.state.lock().unwrap();
                    st.is_root_motion_enabled = !st.is_root_motion_enabled;
                    if let Some(ref mut preview) = st.active_mesh {
                        re_render_opt = Some((preview.clone(), st.camera));
                    }
                    st.is_root_motion_enabled
                };

                let ui_h = self.ui_handle.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_h.upgrade() {
                        ui.set_is_root_motion_enabled(is_enabled);
                    }
                });

                if let Some((mut preview, cam)) = re_render_opt {
                    self.evaluate_and_render_animated_frame(&mut preview, &cam);
                }
            }

            WorkerCommand::ToggleMeshVis => {
                let (preview_opt, val) = {
                    let mut st = self.state.lock().unwrap();
                    st.show_mesh = !st.show_mesh;
                    (st.active_mesh.clone(), st.show_mesh)
                };
                let ui_h = self.ui_handle.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_h.upgrade() {
                        ui.set_show_mesh(val);
                    }
                });
                if let Some(mut p) = preview_opt {
                    let cam = self.state.lock().unwrap().camera;
                    self.evaluate_and_render_animated_frame(&mut p, &cam);
                }
            }
            WorkerCommand::ToggleSkeletonVis => {
                let (preview_opt, val) = {
                    let mut st = self.state.lock().unwrap();
                    st.show_skeleton = !st.show_skeleton;
                    (st.active_mesh.clone(), st.show_skeleton)
                };
                let ui_h = self.ui_handle.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_h.upgrade() {
                        ui.set_show_skeleton(val);
                    }
                });
                if let Some(mut p) = preview_opt {
                    let cam = self.state.lock().unwrap().camera;
                    self.evaluate_and_render_animated_frame(&mut p, &cam);
                }
            }
            WorkerCommand::ToggleBoneNames => {
                let (preview_opt, val) = {
                    let mut st = self.state.lock().unwrap();
                    st.show_bone_names = !st.show_bone_names;
                    (st.active_mesh.clone(), st.show_bone_names)
                };
                let ui_h = self.ui_handle.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_h.upgrade() {
                        ui.set_show_bone_names(val);
                    }
                });
                if let Some(mut p) = preview_opt {
                    let cam = self.state.lock().unwrap().camera;
                    self.evaluate_and_render_animated_frame(&mut p, &cam);
                }
            }
            WorkerCommand::ToggleWireframe => {
                let (preview_opt, val) = {
                    let mut st = self.state.lock().unwrap();
                    st.show_wireframe = !st.show_wireframe;
                    (st.active_mesh.clone(), st.show_wireframe)
                };
                let ui_h = self.ui_handle.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_h.upgrade() {
                        ui.set_show_wireframe(val);
                    }
                });
                if let Some(mut p) = preview_opt {
                    let cam = self.state.lock().unwrap().camera;
                    self.evaluate_and_render_animated_frame(&mut p, &cam);
                }
            }
            WorkerCommand::ToggleGrid => {
                let (preview_opt, val) = {
                    let mut st = self.state.lock().unwrap();
                    st.show_grid = !st.show_grid;
                    (st.active_mesh.clone(), st.show_grid)
                };
                let ui_h = self.ui_handle.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_h.upgrade() {
                        ui.set_show_grid(val);
                    }
                });
                if let Some(mut p) = preview_opt {
                    let cam = self.state.lock().unwrap().camera;
                    self.evaluate_and_render_animated_frame(&mut p, &cam);
                }
            }

            WorkerCommand::RotateMeshViewport {
                delta_yaw,
                delta_pitch,
            } => {
                let mut re_render_opt = None;
                {
                    let mut st = self.state.lock().unwrap();
                    st.camera.yaw += delta_yaw;
                    st.camera.pitch = (st.camera.pitch + delta_pitch).clamp(-1.45, 1.45);
                    if let Some(ref mut preview) = st.active_mesh {
                        re_render_opt = Some((preview.clone(), st.camera));
                    }
                }

                if let Some((mut preview, cam)) = re_render_opt {
                    self.evaluate_and_render_animated_frame(&mut preview, &cam);
                }
            }

            WorkerCommand::ZoomMeshViewport { delta_zoom } => {
                let factor = if delta_zoom > 0.0 { 0.88 } else { 1.14 };
                let mut re_render_opt = None;
                {
                    let mut st = self.state.lock().unwrap();
                    st.camera.zoom(factor);
                    if let Some(ref mut preview) = st.active_mesh {
                        re_render_opt = Some((preview.clone(), st.camera));
                    }
                }

                if let Some((mut preview, cam)) = re_render_opt {
                    self.evaluate_and_render_animated_frame(&mut preview, &cam);
                }
            }

            WorkerCommand::SetViewportFov { fov_degrees } => {
                let mut re_render_opt = None;
                {
                    let mut st = self.state.lock().unwrap();
                    st.camera.fov_degrees = fov_degrees.clamp(20.0, 90.0);
                    if let Some(ref mut preview) = st.active_mesh {
                        re_render_opt = Some((preview.clone(), st.camera));
                    }
                }

                if let Some((mut preview, cam)) = re_render_opt {
                    self.evaluate_and_render_animated_frame(&mut preview, &cam);
                }
            }

            WorkerCommand::SetViewportLighting { mode } => {
                let mut re_render_opt = None;
                {
                    let mut st = self.state.lock().unwrap();
                    st.camera.lighting_mode = mode;
                    if let Some(ref mut preview) = st.active_mesh {
                        re_render_opt = Some((preview.clone(), st.camera));
                    }
                }

                if let Some((mut preview, cam)) = re_render_opt {
                    self.evaluate_and_render_animated_frame(&mut preview, &cam);
                }
            }

            WorkerCommand::SetViewportUpAxis { mode } => {
                let mut re_render_opt = None;
                {
                    let mut st = self.state.lock().unwrap();
                    st.camera.up_axis = mode;
                    if let Some(ref mut preview) = st.active_mesh {
                        re_render_opt = Some((preview.clone(), st.camera));
                    }
                }

                if let Some((mut preview, cam)) = re_render_opt {
                    self.evaluate_and_render_animated_frame(&mut preview, &cam);
                }
            }

            WorkerCommand::ResetViewportCamera => {
                let mut re_render_opt = None;
                {
                    let mut st = self.state.lock().unwrap();
                    st.camera.yaw = 0.785;
                    st.camera.pitch = 0.35;
                    st.camera.fov_degrees = 45.0;
                    if let Some(ref mut preview) = st.active_mesh {
                        re_render_opt = Some((preview.clone(), st.camera));
                    }
                }

                if let Some((mut preview, cam)) = re_render_opt {
                    self.evaluate_and_render_animated_frame(&mut preview, &cam);
                }
            }

            // --- STANDALONE DIRECT TOOLBOX OPERATIONS ---
            WorkerCommand::Decompile8ldDirect { src, dst } => {
                self.logger
                    .log(&format!("[*] Extracting XML from .8ld: {:?}", src));
                match service::decompile_8ld_file(&src, &dst) {
                    Ok(out_file) => {
                        self.logger.log(&format!(
                            "[+] Successfully extracted XML to: {:?}",
                            out_file
                        ));
                        self.set_ui_status("XML extracted successfully.", false);
                    }
                    Err(e) => {
                        self.logger.log(&format!("[!] 8LD Extract Error: {}", e));
                        self.set_ui_error(format!("8LD Error: {}", e));
                    }
                }
            }

            WorkerCommand::Compile8ldDirect { src, dst } => {
                self.logger
                    .log(&format!("[*] Compiling XML to .8ld: {:?}", src));
                match service::compile_8ld_file(&src, &dst) {
                    Ok(out_file) => {
                        self.logger.log(&format!(
                            "[+] Successfully compiled to .8ld: {:?}",
                            out_file
                        ));
                        self.set_ui_status("XML compiled to .8ld successfully.", false);
                    }
                    Err(e) => {
                        self.logger.log(&format!("[!] 8LD Compile Error: {}", e));
                        self.set_ui_error(format!("8LD Error: {}", e));
                    }
                }
            }

            WorkerCommand::Decompile8ldBatch { src_dir, dst_dir } => {
                self.logger.log(&format!(
                    "[*] Batch extracting .8ld files from: {:?}",
                    src_dir
                ));
                match service::batch_decompile_8ld(&src_dir, &dst_dir) {
                    Ok(count) => {
                        self.logger.log(&format!(
                            "[+] Successfully converted {} files to XML in {:?}",
                            count, dst_dir
                        ));
                        self.set_ui_status("Batch XML extraction complete.", false);
                    }
                    Err(e) => {
                        self.logger
                            .log(&format!("[!] Batch 8LD Extract Error: {}", e));
                        self.set_ui_error(format!("Batch 8LD Error: {}", e));
                    }
                }
            }

            WorkerCommand::Compile8ldBatch { src_dir, dst_dir } => {
                self.logger.log(&format!(
                    "[*] Batch compiling XML files from: {:?}",
                    src_dir
                ));
                match service::batch_compile_8ld(&src_dir, &dst_dir) {
                    Ok(count) => {
                        self.logger.log(&format!(
                            "[+] Successfully converted {} files to .8ld in {:?}",
                            count, dst_dir
                        ));
                        self.set_ui_status("Batch 8LD compilation complete.", false);
                    }
                    Err(e) => {
                        self.logger.log(&format!("[!] Batch XML->8LD Error: {}", e));
                        self.set_ui_error(format!("Batch 8LD Error: {}", e));
                    }
                }
            }

            WorkerCommand::DirectVpkToJson { src, dst } => match fs::read(&src) {
                Ok(data) => match crate::engine::assets::vpk::export_vpk_to_json(&data) {
                    Ok(json) => {
                        let _ = fs::write(&dst, json.as_bytes());
                        self.logger.log(&format!(
                            "[+] Converted Voice Package (.debug-vpk) to JSON: {:?}",
                            dst
                        ));
                        self.set_ui_status("Voice Package converted to JSON successfully.", false);
                    }
                    Err(e) => self.set_ui_error(format!("VPK Export Error: {}", e)),
                },
                Err(e) => self.set_ui_error(format!("Failed to read VPK: {}", e)),
            },

            WorkerCommand::DirectJsonToVpk {
                src_json,
                baseline_vpk,
                dst,
            } => match (fs::read_to_string(&src_json), fs::read(&baseline_vpk)) {
                (Ok(json), Ok(base)) => {
                    match crate::engine::assets::vpk::import_vpk_from_json(&json, &base) {
                        Ok(bin) => {
                            let _ = fs::write(&dst, bin);
                            self.logger.log(&format!(
                                "[+] Rebuilt Voice Package (.debug-vpk) from JSON: {:?}",
                                dst
                            ));
                            self.set_ui_status("Voice Package built successfully.", false);
                        }
                        Err(e) => self.set_ui_error(format!("VPK Compile Error: {}", e)),
                    }
                }
                (Err(e), _) => self.set_ui_error(format!("Failed to read JSON: {}", e)),
                (_, Err(e)) => self.set_ui_error(format!("Failed to read baseline VPK: {}", e)),
            },

            WorkerCommand::DirectDtaToJson { src, dst } => match fs::read(&src) {
                Ok(data) => {
                    let stem = src.file_stem().unwrap_or_default().to_string_lossy();
                    match crate::engine::assets::dta::export_dta_to_json(&data, &stem) {
                        Ok(json) => {
                            let _ = fs::write(&dst, json.as_bytes());
                            self.logger
                                .log(&format!("[+] Converted DTA to JSON: {:?}", dst));
                            self.set_ui_status("Successfully converted DTA to JSON.", false);
                        }
                        Err(e) => self.set_ui_error(format!("DTA Error: {}", e)),
                    }
                }
                Err(e) => self.set_ui_error(format!("Failed to read DTA: {}", e)),
            },

            WorkerCommand::DirectJsonToDta {
                src_json,
                baseline_dta,
                dst,
            } => match (fs::read_to_string(&src_json), fs::read(&baseline_dta)) {
                (Ok(json), Ok(base)) => {
                    match crate::engine::assets::dta::import_dta_from_json(&json, &base) {
                        Ok(bin) => {
                            let _ = fs::write(&dst, bin);
                            self.logger
                                .log(&format!("[+] Rebuilt DTA from JSON: {:?}", dst));
                            self.set_ui_status("Successfully built DTA.", false);
                        }
                        Err(e) => self.set_ui_error(format!("DTA Compile Error: {}", e)),
                    }
                }
                (Err(e), _) => self.set_ui_error(format!("Failed to read JSON: {}", e)),
                (_, Err(e)) => self.set_ui_error(format!("Failed to read baseline DTA: {}", e)),
            },

            WorkerCommand::DirectEnvToJson { src, dst } => match fs::read(&src) {
                Ok(data) => {
                    match crate::engine::assets::environment::export_environment_to_json(&data) {
                        Ok(json) => {
                            let _ = fs::write(&dst, json.as_bytes());
                            self.logger
                                .log(&format!("[+] Converted ENV to JSON: {:?}", dst));
                            self.set_ui_status("Successfully converted ENV to JSON.", false);
                        }
                        Err(e) => self.set_ui_error(format!("ENV Error: {}", e)),
                    }
                }
                Err(e) => self.set_ui_error(format!("Failed to read ENV: {}", e)),
            },

            WorkerCommand::DirectJsonToEnv {
                src_json,
                baseline_env,
                dst,
            } => match (fs::read_to_string(&src_json), fs::read(&baseline_env)) {
                (Ok(json), Ok(base)) => {
                    match crate::engine::assets::environment::import_environment_from_json(
                        &json, &base,
                    ) {
                        Ok(bin) => {
                            let _ = fs::write(&dst, bin);
                            self.logger
                                .log(&format!("[+] Rebuilt ENV from JSON: {:?}", dst));
                            self.set_ui_status("Successfully built ENV profile.", false);
                        }
                        Err(e) => self.set_ui_error(format!("ENV Compile Error: {}", e)),
                    }
                }
                (Err(e), _) => self.set_ui_error(format!("Failed to read JSON: {}", e)),
                (_, Err(e)) => self.set_ui_error(format!("Failed to read baseline ENV: {}", e)),
            },

            WorkerCommand::DirectMeshExport { src, dst, is_glb } => {
                match service::export_mesh(&src, &dst, is_glb) {
                    Ok(stats) => {
                        let kind = if is_glb { "glTF" } else { "OBJ" };
                        self.logger.log(&format!(
                            "[+] Exported {} mesh: {:?} ({} verts, {} tris)",
                            kind, dst, stats.vertex_count, stats.triangle_count
                        ));
                        self.set_ui_status("Mesh exported successfully.", false);
                    }
                    Err(e) => self.set_ui_error(format!("Mesh Export Error: {}", e)),
                }
            }

            WorkerCommand::DirectMeshImport {
                chunk_target,
                model_src,
                is_glb,
            } => match service::import_mesh(&chunk_target, &model_src, is_glb) {
                Ok(_) => {
                    self.logger.log(&format!(
                        "[+] Injected 3D model {:?} into {:?}",
                        model_src, chunk_target
                    ));
                    self.set_ui_status("Mesh chunk successfully updated.", false);
                }
                Err(e) => self.set_ui_error(format!("Mesh Import Error: {}", e)),
            },

            WorkerCommand::DirectAssembleLevel {
                omp_path,
                assets_dir,
                dst,
            } => {
                self.logger.log(&format!(
                    "[*] Assembling full 3D scene from {:?} using assets {:?}",
                    omp_path, assets_dir
                ));
                match fs::read(&omp_path) {
                    Ok(data) => {
                        match crate::engine::assets::map::assemble_level_scene_glb(
                            &data,
                            &assets_dir,
                        ) {
                            Ok(glb) => {
                                let _ = fs::write(&dst, glb);
                                self.logger
                                    .log(&format!("[+] Level scene assembled into: {:?}", dst));
                                self.set_ui_status(
                                    "Level scene successfully assembled with PBR textures.",
                                    false,
                                );
                            }
                            Err(e) => self.set_ui_error(format!("Level Assembly Error: {}", e)),
                        }
                    }
                    Err(e) => self.set_ui_error(format!("Failed to read OMP: {}", e)),
                }
            }

            WorkerCommand::DirectTerrainExport { src, dst, is_glb } => {
                match service::export_terrain(&src, &dst, is_glb) {
                    Ok((v, t)) => {
                        self.logger.log(&format!(
                            "[+] Exported terrain heightmap: {:?} ({} vertices, {} triangles)",
                            dst, v, t
                        ));
                        self.set_ui_status("Terrain exported successfully.", false);
                    }
                    Err(e) => self.set_ui_error(format!("Terrain Export Error: {}", e)),
                }
            }

            WorkerCommand::DirectCollisionExport { src, dst } => {
                match service::export_collision_glb(&src, &dst) {
                    Ok(size) => {
                        self.logger.log(&format!(
                            "[+] Exported 3D collision boxes: {:?} ({} bytes)",
                            dst, size
                        ));
                        self.set_ui_status("Collision exported to 3D GLB successfully.", false);
                    }
                    Err(e) => self.set_ui_error(format!("Collision Export Error: {}", e)),
                }
            }

            WorkerCommand::DirectCollisionImport {
                chunk_target,
                glb_src,
            } => match service::import_collision_glb(&chunk_target, &glb_src) {
                Ok(_) => {
                    self.logger.log(&format!(
                        "[+] Updated collision chunk {:?} from {:?}",
                        chunk_target, glb_src
                    ));
                    self.set_ui_status("Collision chunk updated.", false);
                }
                Err(e) => self.set_ui_error(format!("Collision Import Error: {}", e)),
            },

            WorkerCommand::DirectFontToJson { src, dst } => match fs::read(&src) {
                Ok(data) => {
                    match crate::engine::assets::ui_sprite::export_ui_sprite_collection(&data) {
                        Ok(json) => {
                            let _ = fs::write(&dst, json.as_bytes());
                            self.logger.log(&format!(
                                "[+] Exported UI Font / Sprite collection to JSON: {:?}",
                                dst
                            ));
                            self.set_ui_status("Font collection exported to JSON.", false);
                        }
                        Err(e) => self.set_ui_error(format!("Font Export Error: {}", e)),
                    }
                }
                Err(e) => self.set_ui_error(format!("Failed to read Font CLB: {}", e)),
            },

            WorkerCommand::DirectJsonToFont { src_json, dst } => {
                match fs::read_to_string(&src_json) {
                    Ok(text) => {
                        match crate::engine::assets::ui_sprite::import_ui_sprite_collection(&text) {
                            Ok(bin) => {
                                let _ = fs::write(&dst, bin);
                                self.logger
                                    .log(&format!("[+] Rebuilt Font CLB collection: {:?}", dst));
                                self.set_ui_status("Font collection built.", false);
                            }
                            Err(e) => self.set_ui_error(format!("Font Compile Error: {}", e)),
                        }
                    }
                    Err(e) => self.set_ui_error(format!("Failed to read JSON: {}", e)),
                }
            }

            WorkerCommand::DirectTextureExport { src, dst } => {
                match service::export_texture(&src, &dst) {
                    Ok(_) => {
                        self.logger
                            .log(&format!("[+] Exported texture chunk to: {:?}", dst));
                        self.set_ui_status("Texture exported successfully.", false);
                    }
                    Err(e) => self.set_ui_error(format!("Texture Export Error: {}", e)),
                }
            }

            WorkerCommand::DirectTextureImport {
                chunk_target,
                img_src,
            } => match service::import_texture(&chunk_target, &img_src) {
                Ok(_) => {
                    self.logger.log(&format!(
                        "[+] Updated texture chunk {:?} from {:?}",
                        chunk_target, img_src
                    ));
                    self.set_ui_status("Texture chunk updated.", false);
                }
                Err(e) => self.set_ui_error(format!("Texture Import Error: {}", e)),
            },

            WorkerCommand::DirectAudioExport { src, dst } => {
                match service::export_audio(&src, &dst) {
                    Ok(_) => {
                        self.logger
                            .log(&format!("[+] Exported audio chunk to WAV: {:?}", dst));
                        self.set_ui_status("Audio exported successfully.", false);
                    }
                    Err(e) => self.set_ui_error(format!("Audio Export Error: {}", e)),
                }
            }

            WorkerCommand::DirectAudioImport {
                chunk_target,
                wav_src,
            } => match service::import_audio(&chunk_target, &wav_src) {
                Ok(_) => {
                    self.logger.log(&format!(
                        "[+] Updated audio chunk {:?} from {:?}",
                        chunk_target, wav_src
                    ));
                    self.set_ui_status("Audio chunk updated.", false);
                }
                Err(e) => self.set_ui_error(format!("Audio Import Error: {}", e)),
            },

            // --- PROJECT & ARCHIVE OPERATIONS ---
            WorkerCommand::UnpackArchive { src, dst } => {
                self.logger
                    .log(&format!("[*] Unpacking archive: {:?}", src));
                match crate::engine::container::project::unpack_archive(&src, &dst) {
                    Ok((count, info)) => {
                        self.logger.log(&info);
                        self.logger
                            .log(&format!("[+] Unpack complete: {} chunks extracted.", count));
                        let (items, cached, haystacks) = scan_project_folder(&dst);
                        {
                            let mut st = self.state.lock().unwrap();
                            st.visible_indices = (0..items.len()).collect();
                            st.all_cached_assets = cached;
                            st.all_ui_items = items.clone();
                            st.all_search_haystack = haystacks;
                            st.current_proj_dir = Some(dst.clone());
                        }

                        let ui_h = self.ui_handle.clone();
                        let dst_str = dst.to_string_lossy().to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_active_project_dir(dst_str.into());
                                ui.set_asset_list(ModelRc::from(std::rc::Rc::new(VecModel::from(
                                    items,
                                ))));
                                ui.set_status_is_error(false);
                                ui.set_status_msg(
                                    format!("Extracted {} items into workspace.", count).into(),
                                );
                            }
                        });
                    }
                    Err(e) => {
                        self.logger.log(&format!("[!] Unpack error: {}", e));
                        self.set_ui_error(format!("Unpack Error: {}", e));
                    }
                }
            }

            WorkerCommand::LoadProject { proj_dir } => {
                let actual_dir = resolve_project_dir(&proj_dir);
                self.logger
                    .log(&format!("[*] Loading project from: {:?}", actual_dir));

                if !actual_dir.exists()
                    || (!actual_dir.join("chunks").exists()
                        && !actual_dir.join("chunks_vanilla").exists()
                        && !actual_dir.join("project.json").exists())
                {
                    self.logger.log(&format!(
                        "[!] Error: {:?} is not a valid project folder (missing project.json or chunks/)",
                        actual_dir
                    ));
                    self.set_ui_error("Error: Not a valid Overlord project folder!".into());
                    return;
                }

                let (items, cached, haystacks) = scan_project_folder(&actual_dir);
                let total = items.len();
                {
                    let mut st = self.state.lock().unwrap();
                    st.visible_indices = (0..items.len()).collect();
                    st.all_cached_assets = cached;
                    st.all_ui_items = items.clone();
                    st.all_search_haystack = haystacks;
                    st.current_proj_dir = Some(actual_dir.clone());
                }

                self.logger
                    .log(&format!("[+] Project loaded: {} items found.", total));
                let ui_h = self.ui_handle.clone();
                let actual_dir_str = actual_dir.to_string_lossy().to_string();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_h.upgrade() {
                        ui.set_active_project_dir(actual_dir_str.into());
                        ui.set_selected_index(-1);
                        ui.set_active_file_path("".into());
                        ui.set_asset_list(ModelRc::from(std::rc::Rc::new(VecModel::from(items))));
                        ui.set_status_is_error(false);
                        ui.set_status_msg(
                            format!("Loaded {} resources into workspace.", total).into(),
                        );
                    }
                });
            }

            WorkerCommand::CleanRebuild { proj_dir } => {
                let actual_dir = resolve_project_dir(&proj_dir);
                self.logger.log(&format!(
                    "[*] Performing clean rebuild from vanilla: {:?}",
                    actual_dir
                ));
                match crate::engine::container::sync::clean_rebuild_project(&actual_dir) {
                    Ok(synced) => {
                        self.logger.log(&format!(
                            "[+] Clean rebuild completed: {} assets re-synced.",
                            synced
                        ));
                        self.refresh_project_state(&actual_dir, "Clean rebuild successful!");
                    }
                    Err(e) => {
                        self.logger.log(&format!("[!] Clean rebuild error: {}", e));
                        self.set_ui_error(format!("Rebuild Error: {}", e));
                    }
                }
            }

            WorkerCommand::RevertAsset {
                proj_dir,
                chunk_path,
            } => {
                let actual_dir = resolve_project_dir(&proj_dir);
                self.logger
                    .log(&format!("[*] Reverting asset to vanilla: {:?}", chunk_path));
                match crate::engine::container::sync::revert_single_asset(&actual_dir, &chunk_path)
                {
                    Ok(_) => {
                        self.logger.log(&format!(
                            "[+] Reverted {:?} to pristine vanilla state.",
                            chunk_path
                        ));
                        self.refresh_project_state(
                            &actual_dir,
                            "Asset restored from vanilla baseline.",
                        );
                    }
                    Err(e) => {
                        self.logger.log(&format!("[!] Revert error: {}", e));
                        self.set_ui_error(format!("Revert Error: {}", e));
                    }
                }
            }

            WorkerCommand::PackArchive { proj_dir } => {
                let actual_dir = resolve_project_dir(&proj_dir);
                self.logger
                    .log(&format!("[*] Packing project: {:?}", actual_dir));
                let out = actual_dir.join("rebuilt.prp");
                match crate::engine::container::project::pack_archive(&actual_dir, &out, 0) {
                    Ok(size) => {
                        self.logger
                            .log(&format!("[+] Pack complete: {:?} ({} bytes)", out, size));
                        self.set_ui_status("Archive successfully packed to rebuilt.prp!", false);
                    }
                    Err(e) => {
                        self.logger.log(&format!("[!] Pack error: {}", e));
                        self.set_ui_error(format!("Pack Error: {}", e));
                    }
                }
            }

            WorkerCommand::CreatePatch {
                base_dir,
                mod_dir,
                out_file,
            } => match crate::utils::diff::create_diff(&base_dir, &mod_dir, &out_file) {
                Ok(count) => self.logger.log(&format!(
                    "[+] Mod patch created: {} modified items tracked.",
                    count
                )),
                Err(e) => self.logger.log(&format!("[!] Patch creation error: {}", e)),
            },

            WorkerCommand::ApplyPatch {
                target_dir,
                patch_file,
            } => match crate::utils::diff::apply_patch(&target_dir, &patch_file) {
                Ok(count) => self
                    .logger
                    .log(&format!("[+] Patch applied: {} files updated.", count)),
                Err(e) => self
                    .logger
                    .log(&format!("[!] Patch application error: {}", e)),
            },

            // --- CONTEXTUAL EXPORT OPERATIONS ---
            WorkerCommand::ExportDds {
                chunk_path,
                out_path,
            } => match service::export_texture(&chunk_path, &out_path) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] Texture exported to: {:?}", out_path)),
                Err(e) => self.logger.log(&format!("[!] DDS export error: {}", e)),
            },

            WorkerCommand::ImportDds {
                chunk_path,
                in_path,
            } => match service::import_texture(&chunk_path, &in_path) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] Chunk {:?} updated with new DDS.", chunk_path)),
                Err(e) => self
                    .logger
                    .log(&format!("[!] DDS replacement error: {}", e)),
            },

            WorkerCommand::ExportWav {
                chunk_path,
                out_path,
            } => match service::export_audio(&chunk_path, &out_path) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] Audio exported to: {:?}", out_path)),
                Err(e) => self.logger.log(&format!("[!] Audio export error: {}", e)),
            },

            WorkerCommand::ImportWav {
                chunk_path,
                in_path,
            } => match service::import_audio(&chunk_path, &in_path) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] Audio chunk {:?} updated.", chunk_path)),
                Err(e) => self.logger.log(&format!("[!] Audio import error: {}", e)),
            },

            WorkerCommand::ExportMesh {
                chunk_path,
                out_path,
                is_glb,
            } => match service::export_mesh(&chunk_path, &out_path, is_glb) {
                Ok(stats) => {
                    let kind = if is_glb { "glTF" } else { "OBJ" };
                    self.logger.log(&format!(
                        "[+] {} exported: {:?} ({} verts, {} tris, skinned: {})",
                        kind, out_path, stats.vertex_count, stats.triangle_count, stats.is_skinned
                    ));
                }
                Err(e) => self.logger.log(&format!("[!] Mesh export error: {}", e)),
            },

            WorkerCommand::ImportMesh {
                chunk_path,
                in_path,
                is_glb,
            } => match service::import_mesh(&chunk_path, &in_path, is_glb) {
                Ok(_) => {
                    let kind = if is_glb { ".glb" } else { "OBJ" };
                    self.logger.log(&format!(
                        "[+] Mesh chunk {:?} rebuilt from {} (Skinning preserved)",
                        chunk_path, kind
                    ));
                }
                Err(e) => self.logger.log(&format!("[!] Mesh import error: {}", e)),
            },

            WorkerCommand::ExportTerrain {
                chunk_path,
                out_path,
                is_glb,
            } => match service::export_terrain(&chunk_path, &out_path, is_glb) {
                Ok((v_count, tri_count)) => {
                    if is_glb {
                        self.logger.log(&format!(
                            "[+] Terrain glTF exported: {:?} ({} vertices, {} triangles)",
                            out_path, v_count, tri_count
                        ));
                    } else {
                        self.logger.log(&format!(
                            "[+] Terrain OBJ exported: {:?} ({} vertices, {} triangles)",
                            out_path, v_count, tri_count
                        ));
                    }
                }
                Err(e) => self.logger.log(&format!("[!] Terrain export error: {}", e)),
            },

            WorkerCommand::ExportCollisionGlb {
                chunk_path,
                out_path,
            } => match service::export_collision_glb(&chunk_path, &out_path) {
                Ok(size) => self.logger.log(&format!(
                    "[+] 3D Collision exported to glTF (.glb): {:?} ({} bytes)",
                    out_path, size
                )),
                Err(e) => self
                    .logger
                    .log(&format!("[!] 3D Collision GLB export error: {}", e)),
            },

            WorkerCommand::ImportCollisionGlb {
                chunk_path,
                in_path,
            } => match service::import_collision_glb(&chunk_path, &in_path) {
                Ok(_) => self.logger.log(&format!(
                    "[+] Collision chunk {:?} updated from 3D .glb boxes.",
                    chunk_path
                )),
                Err(e) => self
                    .logger
                    .log(&format!("[!] 3D Collision GLB import error: {}", e)),
            },

            WorkerCommand::ExportLua {
                chunk_path,
                out_path,
            } => match service::export_lua(&chunk_path, &out_path) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] Lua bytecode exported: {:?}", out_path)),
                Err(e) => self.logger.log(&format!("[!] Lua export error: {}", e)),
            },

            WorkerCommand::ImportLua {
                chunk_path,
                in_path,
            } => match service::import_lua(&chunk_path, &in_path) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] Lua chunk {:?} updated.", chunk_path)),
                Err(e) => self.logger.log(&format!("[!] Lua import error: {}", e)),
            },

            WorkerCommand::ExportAnimGlb {
                chunk_path,
                out_path,
            } => match service::export_anim_glb(&chunk_path, &out_path) {
                Ok(_) => self.logger.log(&format!(
                    "[+] Animation timeline exported to glTF: {:?}",
                    out_path
                )),
                Err(e) => self
                    .logger
                    .log(&format!("[!] Animation GLB export error: {}", e)),
            },

            WorkerCommand::ExportAnimJson {
                chunk_path,
                out_path,
            } => match service::export_anim_json(&chunk_path, &out_path) {
                Ok(_) => self.logger.log(&format!(
                    "[+] Animation keyframes exported to JSON: {:?}",
                    out_path
                )),
                Err(e) => self
                    .logger
                    .log(&format!("[!] Animation JSON export error: {}", e)),
            },

            // --- CONTEXTUAL ASSET SAVE OPERATIONS ---
            WorkerCommand::SaveMaterial {
                chunk_path,
                json_data,
            } => match service::save_material(&chunk_path, &json_data) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] Material chunk {:?} updated.", chunk_path)),
                Err(e) => self.logger.log(&format!("[!] Material save error: {}", e)),
            },

            WorkerCommand::SaveUI {
                chunk_path,
                json_data,
            } => match service::save_ui(&chunk_path, &json_data) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] UI layout chunk {:?} updated.", chunk_path)),
                Err(e) => self.logger.log(&format!("[!] UI save error: {}", e)),
            },

            WorkerCommand::SaveObject {
                chunk_path,
                json_data,
            } => match service::save_object(&chunk_path, &json_data) {
                Ok(_) => self.logger.log(&format!(
                    "[+] 3D Object entity chunk {:?} updated.",
                    chunk_path
                )),
                Err(e) => self.logger.log(&format!("[!] Object save error: {}", e)),
            },

            WorkerCommand::SaveTerrainPalette {
                chunk_path,
                json_data,
            } => match service::save_terrain_palette(&chunk_path, &json_data) {
                Ok(_) => self.logger.log(&format!(
                    "[+] Terrain Palette chunk {:?} updated.",
                    chunk_path
                )),
                Err(e) => self
                    .logger
                    .log(&format!("[!] Terrain Palette save error: {}", e)),
            },

            WorkerCommand::SaveEnvironment {
                chunk_path,
                json_data,
            } => match service::save_environment(&chunk_path, &json_data) {
                Ok(_) => self
                    .logger
                    .log(&format!("[+] Environment chunk {:?} updated.", chunk_path)),
                Err(e) => self
                    .logger
                    .log(&format!("[!] Environment save error: {}", e)),
            },

            WorkerCommand::SaveM8ld {
                chunk_path,
                json_data,
            } => match service::save_m8ld(&chunk_path, &json_data) {
                Ok(_) => self.logger.log(&format!(
                    "[+] Level Map Logic (.8ld) {:?} updated.",
                    chunk_path
                )),
                Err(e) => self
                    .logger
                    .log(&format!("[!] Level Map Logic save error: {}", e)),
            },

            WorkerCommand::SaveUiSprite {
                chunk_path,
                json_data,
            } => match service::save_ui_sprite(&chunk_path, &json_data) {
                Ok(_) => self.logger.log(&format!(
                    "[+] UI Sprite Collection {:?} updated.",
                    chunk_path
                )),
                Err(e) => self.logger.log(&format!("[!] UI Sprite save error: {}", e)),
            },

            WorkerCommand::SaveDta {
                chunk_path,
                json_data,
            } => match service::save_dta(&chunk_path, &json_data) {
                Ok(_) => self.logger.log(&format!(
                    "[+] Lighting Set (.dta) {:?} updated.",
                    chunk_path
                )),
                Err(e) => self
                    .logger
                    .log(&format!("[!] Lighting Set save error: {}", e)),
            },

            WorkerCommand::SaveVpk {
                chunk_path,
                json_data,
            } => match service::save_vpk(&chunk_path, &json_data) {
                Ok(_) => self.logger.log(&format!(
                    "[+] Voice Package (.debug-vpk) {:?} updated.",
                    chunk_path
                )),
                Err(e) => self
                    .logger
                    .log(&format!("[!] Voice Package save error: {}", e)),
            },
        }
    }

    fn evaluate_and_render_animated_frame(
        &mut self,
        preview: &mut ActiveMeshPreview,
        cam: &ViewportCamera,
    ) {
        let mut debug_lines = Vec::new();
        let mut bone_labels = Vec::new();

        let (
            is_skinning,
            _is_root_motion,
            show_mesh,
            show_skeleton,
            show_names,
            show_wire,
            show_grid,
        ) = {
            let st = self.state.lock().unwrap();
            (
                st.is_skinning_enabled,
                st.is_root_motion_enabled,
                st.show_mesh,
                st.show_skeleton,
                st.show_bone_names,
                st.show_wireframe,
                st.show_grid,
            )
        };

        if let Some(clip_idx) = preview.current_clip_index
            && let Some(clip) = preview.available_clips.get(clip_idx)
        {
            let t = preview.current_time_seconds;

            for sm in &mut preview.submeshes {
                if !sm.bones.is_empty() && !sm.weights.is_empty() {
                    let (skin_matrices, lines) = compute_skinning_matrices(&sm.bones, clip, t);

                    if show_skeleton {
                        for mut line_vert in lines {
                            let p = Vector3 {
                                x: line_vert.position[0],
                                y: line_vert.position[1],
                                z: line_vert.position[2],
                            };
                            let tp = match cam.up_axis {
                                1 => [p.x, -p.z, p.y],
                                2 => [p.x, p.z, -p.y],
                                _ => [p.x, p.y, p.z],
                            };
                            line_vert.position = tp;
                            debug_lines.push(line_vert);
                        }
                    }

                    if is_skinning {
                        apply_skeletal_skinning(
                            &sm.rest_positions,
                            &sm.rest_normals,
                            &sm.joints,
                            &sm.weights,
                            &skin_matrices,
                            &mut sm.positions,
                            &mut sm.normals,
                        );
                    } else {
                        sm.positions.copy_from_slice(&sm.rest_positions);
                        sm.normals.copy_from_slice(&sm.rest_normals);
                    }
                }
            }
        }

        if show_names && !preview.submeshes.is_empty() {
            let width = 1024.0f32;
            let height = 1024.0f32;
            let aspect = width / height;
            let fov_rad = cam.fov_degrees.to_radians();
            let proj = perspective_rh_zo(fov_rad, aspect, 0.1, 2000.0);

            let mut min = glam::Vec3::splat(f32::INFINITY);
            let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
            for sm in &preview.submeshes {
                for &p in &sm.positions {
                    let tp = match cam.up_axis {
                        1 => glam::Vec3::new(p.x, -p.z, p.y),
                        2 => glam::Vec3::new(p.x, p.z, -p.y),
                        _ => glam::Vec3::new(p.x, p.y, p.z),
                    };
                    min = min.min(tp);
                    max = max.max(tp);
                }
            }
            let center = if min.x.is_finite() {
                (min + max) * 0.5
            } else {
                glam::Vec3::ZERO
            };
            let eye = center
                + glam::Vec3::new(
                    cam.yaw.sin() * cam.pitch.cos() * cam.distance,
                    cam.pitch.sin() * cam.distance,
                    cam.yaw.cos() * cam.pitch.cos() * cam.distance,
                );
            let view = look_at_rh(eye, center, glam::Vec3::Y);
            let view_proj = proj * view;

            if let Some(clip_idx) = preview.current_clip_index
                && let Some(clip) = preview.available_clips.get(clip_idx)
                && let Some(sm) = preview.submeshes.first()
            {
                let (_, lines) =
                    compute_skinning_matrices(&sm.bones, clip, preview.current_time_seconds);
                for (b_idx, bone) in sm.bones.iter().enumerate() {
                    if b_idx * 2 < lines.len() {
                        let bp = lines[b_idx * 2].position;
                        let world_pos = glam::Vec4::new(bp[0], bp[1], bp[2], 1.0);
                        let clip_pos = view_proj * world_pos;
                        if clip_pos.w > 0.0 {
                            let ndc = clip_pos.truncate() / clip_pos.w;
                            let sx = (ndc.x * 0.5 + 0.5) * width;
                            let sy = (1.0 - (ndc.y * 0.5 + 0.5)) * height;
                            bone_labels.push(crate::BoneLabel {
                                name: bone.name.clone().into(),
                                x: sx,
                                y: sy,
                            });
                        }
                    }
                }
            }
        }

        let draw_data: Vec<SubmeshDrawData> = if show_mesh {
            preview
                .submeshes
                .iter()
                .map(|sm| SubmeshDrawData {
                    positions: &sm.positions,
                    indices: &sm.indices,
                    normals: &sm.normals,
                    uvs: &sm.uvs,
                    texture: sm.texture.as_ref().map(|t| TextureData {
                        width: t.0,
                        height: t.1,
                        rgba: &t.2,
                    }),
                })
                .collect()
        } else {
            Vec::new()
        };

        let buf = self.gpu_renderer.as_mut().unwrap().render(
            &draw_data,
            &debug_lines,
            show_grid,
            show_wire,
            (1024, 1024),
            cam,
        );

        let ui_h = self.ui_handle.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_h.upgrade() {
                ui.set_mesh_preview(Image::from_rgba8(buf));
                ui.set_bone_labels(ModelRc::from(std::rc::Rc::new(VecModel::from(bone_labels))));
            }
        });
    }

    fn center_camera_for_preview(&self, preview: &ActiveMeshPreview) -> ViewportCamera {
        let mut st = self.state.lock().unwrap();
        let up_axis = st.camera.up_axis;

        let mut min = Vector3 {
            x: f32::INFINITY,
            y: f32::INFINITY,
            z: f32::INFINITY,
        };
        let mut max = Vector3 {
            x: f32::NEG_INFINITY,
            y: f32::NEG_INFINITY,
            z: f32::NEG_INFINITY,
        };

        let transform = |p: Vector3| -> Vector3 {
            match up_axis {
                1 => Vector3 {
                    x: p.x,
                    y: -p.z,
                    z: p.y,
                },
                2 => Vector3 {
                    x: p.x,
                    y: p.z,
                    z: -p.y,
                },
                _ => p,
            }
        };

        for sm in &preview.submeshes {
            for &p in &sm.positions {
                let tp = transform(p);
                min.x = min.x.min(tp.x);
                min.y = min.y.min(tp.y);
                min.z = min.z.min(tp.z);
                max.x = max.x.max(tp.x);
                max.y = max.y.max(tp.y);
                max.z = max.z.max(tp.z);
            }
        }

        let sx = (max.x - min.x).abs();
        let sy = (max.y - min.y).abs();
        let sz = (max.z - min.z).abs();
        let max_dim = sx.max(sy).max(sz).max(1.0);
        let auto_dist = (max_dim * 1.75).clamp(1.5, 300.0);

        st.camera.distance = auto_dist;
        st.camera.target = Vector3 {
            x: (min.x + max.x) * 0.5,
            y: (min.y + max.y) * 0.5,
            z: (min.z + max.z) * 0.5,
        };
        st.camera
    }

    fn refresh_project_state(&self, project_dir: &Path, status_msg: &'static str) {
        let (items, cached, haystacks) = scan_project_folder(project_dir);
        {
            let mut st = self.state.lock().unwrap();
            st.visible_indices = (0..items.len()).collect();
            st.all_cached_assets = cached;
            st.all_ui_items = items.clone();
            st.all_search_haystack = haystacks;
        }
        let ui_h = self.ui_handle.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_h.upgrade() {
                ui.set_asset_list(ModelRc::from(std::rc::Rc::new(VecModel::from(items))));
                ui.set_status_is_error(false);
                ui.set_status_msg(status_msg.into());
            }
        });
    }

    fn set_ui_status(&self, status_msg: &'static str, is_error: bool) {
        let ui_h = self.ui_handle.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_h.upgrade() {
                ui.set_status_is_error(is_error);
                ui.set_status_msg(status_msg.into());
            }
        });
    }

    fn set_ui_error(&self, err_msg: String) {
        let ui_h = self.ui_handle.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_h.upgrade() {
                ui.set_status_is_error(true);
                ui.set_status_msg(err_msg.into());
            }
        });
    }
}

// Helper projections for bone name overlay
fn perspective_rh_zo(fov_y_radians: f32, aspect_ratio: f32, z_near: f32, z_far: f32) -> glam::Mat4 {
    let f = 1.0 / (fov_y_radians / 2.0).tan();
    glam::Mat4::from_cols_array(&[
        f / aspect_ratio,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        z_far / (z_near - z_far),
        -1.0,
        0.0,
        0.0,
        (z_far * z_near) / (z_near - z_far),
        0.0,
    ])
}

fn look_at_rh(eye: glam::Vec3, center: glam::Vec3, up: glam::Vec3) -> glam::Mat4 {
    let f = (center - eye).normalize();
    let s = f.cross(up).normalize();
    let u = s.cross(f);
    glam::Mat4::from_cols_array(&[
        s.x,
        u.x,
        -f.x,
        0.0,
        s.y,
        u.y,
        -f.y,
        0.0,
        s.z,
        u.z,
        -f.z,
        0.0,
        -eye.dot(s),
        -eye.dot(u),
        eye.dot(f),
        1.0,
    ])
}

// -----------------------------------------------------------------------------
// SMART TEXTURE & ANIMATION RESOLUTION
// -----------------------------------------------------------------------------

fn load_mesh_with_smart_texture(
    bytes: &[u8],
    mesh_stem: &str,
    project_dir: Option<&Path>,
) -> Option<RenderSubmesh> {
    let parsed = crate::engine::assets::mesh::extract_mesh_geometry(bytes).ok()?;
    let resolved_tex = resolve_smart_texture_for_mesh(project_dir, mesh_stem);
    let tex_arc = resolved_tex.map(|t| Arc::new((t.width, t.height, t.rgba)));

    Some(RenderSubmesh {
        name: mesh_stem.to_string(),
        positions: parsed.positions.clone(),
        normals: parsed.normals.clone(),
        rest_positions: parsed.positions,
        rest_normals: parsed.normals,
        joints: parsed.joints,
        weights: parsed.weights,
        bones: parsed.bones,
        indices: parsed.indices,
        uvs: parsed.uvs,
        texture: tex_arc,
    })
}

fn find_master_skeleton(project_dir: Option<&Path>) -> Option<Vec<ObjectBone>> {
    let base_dir = project_dir?;
    let chunks_dir = base_dir.join("chunks");
    if !chunks_dir.exists() {
        return None;
    }

    for entry in fs::read_dir(&chunks_dir).ok()?.flatten() {
        let p = entry.path();
        if p.is_file()
            && p.extension().is_some_and(|e| e == "bin")
            && let Ok(bytes) = fs::read(&p)
            && let Ok(bones) = parse_object_bone_container(&bytes)
            && !bones.is_empty()
        {
            return Some(bones);
        }
    }
    None
}

fn discover_companion_animations(
    display_name: &str,
    mesh_stem: &str,
    project_dir: Option<&Path>,
) -> Vec<AnimationClip> {
    let base_dir = match project_dir {
        Some(d) => d,
        None => return Vec::new(),
    };

    let mut clips = Vec::new();
    let chunks_dir = base_dir.join("chunks");

    // Clean package prefix, e.g. "[FIRE_BEETLE]MESH\2" -> "firebeetle"
    let pkg_name = display_name
        .split(']')
        .next()
        .map(|s| s.trim_start_matches('['))
        .unwrap_or(display_name);

    let norm_pkg = s_normalize(pkg_name);
    let norm_stem = s_normalize(mesh_stem);
    let norm_disp = s_normalize(display_name);

    if chunks_dir.exists()
        && let Ok(entries) = fs::read_dir(&chunks_dir)
    {
        for e in entries.flatten() {
            let path = e.path();
            if path.is_file()
                && path.extension().is_some_and(|ext| ext == "bin")
                && let Ok(bytes) = fs::read(&path)
                && let Ok(clip) = parse_animation_clip(&bytes)
            {
                let norm_rig = s_normalize(&clip.target_rig);
                let norm_clip = s_normalize(&clip.name);

                let is_match = !norm_pkg.is_empty()
                    && (norm_rig.contains(&norm_pkg) || norm_pkg.contains(&norm_rig))
                    || norm_disp.contains(&norm_rig)
                    || norm_rig.contains(&norm_stem)
                    || norm_clip.contains(&norm_stem)
                    || norm_disp.contains("beetle")
                        && (norm_rig.contains("beetle") || norm_clip.contains("beetle"))
                    || norm_disp.contains("minion")
                        && (norm_rig.contains("minion") || norm_clip.contains("minion"));

                if is_match {
                    clips.push(clip);
                }
            }
        }

        // Fallback: If no name matched, but this archive contains clips, include all archive clips
        if clips.is_empty() {
            for e in fs::read_dir(&chunks_dir)
                .ok()
                .into_iter()
                .flatten()
                .flatten()
            {
                let path = e.path();
                if path.is_file()
                    && path.extension().is_some_and(|ext| ext == "bin")
                    && let Ok(bytes) = fs::read(&path)
                    && let Ok(clip) = parse_animation_clip(&bytes)
                {
                    clips.push(clip);
                }
            }
        }
    }

    clips.sort_by(|a, b| a.name.cmp(&b.name));
    clips.dedup_by(|a, b| a.name == b.name);
    clips
}

fn resolve_smart_texture_for_mesh(
    project_dir: Option<&Path>,
    mesh_stem: &str,
) -> Option<ResolvedTexture> {
    let base_dir = project_dir?;
    let assets_dir = base_dir.join("assets");
    let textures_dir = assets_dir.join("textures");
    let objects_dir = assets_dir.join("objects");
    let materials_dir = assets_dir.join("materials");

    let norm_mesh = s_normalize(mesh_stem);

    let mut bound_material_name: Option<String> = None;
    if objects_dir.exists()
        && let Ok(entries) = fs::read_dir(&objects_dir)
    {
        for e in entries.flatten() {
            if e.path().extension().is_some_and(|ext| ext == "json")
                && let Ok(content) = fs::read_to_string(e.path())
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&content)
                && let Some(bindings) = v["mesh_bindings"].as_array()
            {
                for b in bindings {
                    let m_path = b["mesh_path"].as_str().unwrap_or_default();
                    let m_name = b["mesh_part_name"].as_str().unwrap_or_default();
                    let norm_path = s_normalize(m_path);
                    let norm_name = s_normalize(m_name);

                    if (norm_mesh.contains(&norm_path)
                        || norm_mesh.contains(&norm_name)
                        || norm_path.contains(&norm_mesh))
                        && let Some(mat_path) = b["material_path"].as_str()
                    {
                        bound_material_name = Some(mat_path.to_string());
                        break;
                    }
                }
            }
        }
    }

    let mut matched_texture_filename: Option<String> = None;
    if let Some(ref mat_target) = bound_material_name
        && materials_dir.exists()
        && let Ok(entries) = fs::read_dir(&materials_dir)
    {
        for e in entries.flatten() {
            if e.path().extension().is_some_and(|ext| ext == "json")
                && let Ok(content) = fs::read_to_string(e.path())
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&content)
            {
                let mat_val = v["blocks"]
                    .as_array()
                    .and_then(|blocks| {
                        blocks
                            .iter()
                            .find(|b| b["id"] == 20)
                            .and_then(|b| b["value"].as_str())
                    })
                    .unwrap_or_default();

                if s_normalize(mat_val) == s_normalize(mat_target)
                    && let Some(blocks) = v["blocks"].as_array()
                {
                    for b in blocks {
                        let role = b["role"].as_str().unwrap_or_default();
                        if (role.contains("Diffuse")
                            || role.contains("Base Color")
                            || b["id"] == 30)
                            && let Some(tex_name) = b["name"].as_str()
                        {
                            matched_texture_filename = Some(tex_name.to_string());
                            break;
                        }
                    }
                }
            }
        }
    }

    if textures_dir.exists()
        && let Ok(entries) = fs::read_dir(&textures_dir)
    {
        let mut fallback_dds: Option<PathBuf> = None;
        let mut diff_dds: Option<PathBuf> = None;
        let clean_target = matched_texture_filename.as_deref().map(s_normalize);

        for e in entries.flatten() {
            let p = e.path();
            if p.is_file() && p.extension().is_some_and(|ext| ext == "dds") {
                let fname = p.file_name().unwrap_or_default().to_string_lossy();
                let norm_fname = s_normalize(&fname);

                if fallback_dds.is_none() {
                    fallback_dds = Some(p.clone());
                }
                if norm_fname.contains("diff") || norm_fname.contains("base") {
                    diff_dds = Some(p.clone());
                }

                if let Some(ref target) = clean_target
                    && (norm_fname.contains(target) || target.contains(&norm_fname))
                {
                    diff_dds = Some(p);
                    break;
                }
            }
        }

        let chosen_texture = diff_dds.or(fallback_dds)?;
        if let Ok(dds_bytes) = fs::read(&chosen_texture)
            && let Ok(parsed_tex) = crate::engine::assets::texture::parse_texture_chunk(&dds_bytes)
        {
            let rgba = dds_decoder::decode_to_rgba(
                parsed_tex.width,
                parsed_tex.height,
                parsed_tex.format,
                &parsed_tex.pixel_data,
            );
            return Some(ResolvedTexture {
                filename: chosen_texture
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                width: parsed_tex.width,
                height: parsed_tex.height,
                rgba,
            });
        }
    }

    None
}

fn s_normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

fn build_composite_mesh_assembly(
    _stem: &str,
    project_dir: Option<&Path>,
) -> (Vec<RenderSubmesh>, bool) {
    let base_dir = match project_dir {
        Some(d) => d,
        None => return (Vec::new(), false),
    };

    let assets_dir = base_dir.join("assets");
    let meshes_dir = assets_dir.join("meshes");
    let chunks_dir = base_dir.join("chunks");

    let mut submeshes = Vec::new();

    if meshes_dir.exists()
        && let Ok(entries) = fs::read_dir(&meshes_dir)
    {
        let mut mesh_paths: Vec<_> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "glb"))
            .map(|e| e.path())
            .collect();
        mesh_paths.sort();

        for m_path in mesh_paths {
            let m_stem = m_path.file_stem().unwrap_or_default().to_string_lossy();
            if m_stem.ends_with("_MASTER_RIG") {
                continue;
            }

            let chunk_name = m_stem.split('_').find(|s| s.starts_with("chunk"));
            let chunk_file = chunk_name.map(|c| chunks_dir.join(format!("{}.bin", c)));

            let bytes_opt = if let Some(ref cf) = chunk_file
                && cf.exists()
            {
                fs::read(cf).ok()
            } else {
                fs::read(&m_path).ok()
            };

            if let Some(bytes) = bytes_opt
                && let Some(sm) = load_mesh_with_smart_texture(&bytes, &m_stem, project_dir)
            {
                submeshes.push(sm);
            }
        }
    }

    let has_composite = submeshes.len() > 1;
    (submeshes, has_composite)
}

fn build_stats_lines(submeshes: &[RenderSubmesh]) -> Vec<slint::SharedString> {
    let mut stats_lines: Vec<slint::SharedString> = Vec::new();
    let total_verts: usize = submeshes.iter().map(|s| s.positions.len()).sum();
    let total_tris: usize = submeshes.iter().map(|s| s.indices.len() / 3).sum();

    let mut min = Vector3 {
        x: f32::INFINITY,
        y: f32::INFINITY,
        z: f32::INFINITY,
    };
    let mut max = Vector3 {
        x: f32::NEG_INFINITY,
        y: f32::NEG_INFINITY,
        z: f32::NEG_INFINITY,
    };

    for sm in submeshes {
        for p in &sm.positions {
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            min.z = min.z.min(p.z);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
            max.z = max.z.max(p.z);
        }
    }

    let sx = (max.x - min.x).abs();
    let sy = (max.y - min.y).abs();
    let sz = (max.z - min.z).abs();

    stats_lines.push(format!("Total Vertices: {}", total_verts).into());
    stats_lines.push(format!("Total Triangles: {}", total_tris).into());
    stats_lines.push(format!("Submesh Count: {}", submeshes.len()).into());
    stats_lines.push(format!("Size: {:.2}m × {:.2}m × {:.2}m", sx, sy, sz).into());
    stats_lines.push(format!("Bounds Min: [{:.2}, {:.2}, {:.2}]", min.x, min.y, min.z).into());
    stats_lines.push(format!("Bounds Max: [{:.2}, {:.2}, {:.2}]", max.x, max.y, max.z).into());

    for (i, sm) in submeshes.iter().enumerate() {
        let tex_status = if sm.texture.is_some() {
            "Texture: Linked"
        } else {
            "Texture: Neutral Slate"
        };
        stats_lines.push(format!("Submesh [{}]: {} ({})", i, sm.name, tex_status).into());
    }

    stats_lines
}
