pub mod direct_tools;
pub mod preview;
pub mod project_ops;
pub mod viewport;

pub use preview::ResolvedTexture;

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc::Receiver};

use slint::{Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

use crate::AppWindow;
use crate::engine::assets::sniffer::AssetKind;
use crate::engine::service;
use crate::gui::commands::WorkerCommand;
use crate::gui::{ActiveMeshPreview, AppState};
use crate::utils::dds_decoder;
use crate::utils::logger::UiLogger;
use crate::utils::renderer::WgpuRenderer;

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
        let gpu_renderer = match WgpuRenderer::new() {
            Ok(renderer) => {
                logger.log("[+] 3D Graphics: WGPU hardware renderer initialized successfully.");
                Some(renderer)
            }
            Err(e) => {
                logger.log(&format!(
                    "[!] Warning: 3D hardware graphics initialization failed: {}. 3D Viewport is disabled.",
                    e
                ));
                None
            }
        };

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
                // Coalesce high-frequency viewport rotation events
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

                // Coalesce high-frequency zoom events
                WorkerCommand::ZoomMeshViewport { mut delta_zoom } => {
                    let mut extra_commands = Vec::new();
                    while let Ok(next_cmd) = rx.try_recv() {
                        match next_cmd {
                            WorkerCommand::ZoomMeshViewport { delta_zoom: dz } => {
                                delta_zoom += dz;
                            }
                            other => {
                                extra_commands.push(other);
                                break;
                            }
                        }
                    }

                    self.handle_command(WorkerCommand::ZoomMeshViewport { delta_zoom });

                    for other_cmd in extra_commands {
                        self.handle_command(other_cmd);
                    }
                }

                // Coalesce animation timeline scrubbing
                WorkerCommand::SetAnimationTime { mut time_seconds } => {
                    let mut extra_commands = Vec::new();
                    while let Ok(next_cmd) = rx.try_recv() {
                        match next_cmd {
                            WorkerCommand::SetAnimationTime { time_seconds: t } => {
                                time_seconds = t;
                            }
                            other => {
                                extra_commands.push(other);
                                break;
                            }
                        }
                    }

                    self.handle_command(WorkerCommand::SetAnimationTime { time_seconds });

                    for other_cmd in extra_commands {
                        self.handle_command(other_cmd);
                    }
                }

                // Coalesce 30 FPS animation timer ticks
                WorkerCommand::TickAnimationPlayback { mut delta_seconds } => {
                    let mut extra_commands = Vec::new();
                    while let Ok(next_cmd) = rx.try_recv() {
                        match next_cmd {
                            WorkerCommand::TickAnimationPlayback { delta_seconds: dt } => {
                                delta_seconds += dt;
                            }
                            other => {
                                extra_commands.push(other);
                                break;
                            }
                        }
                    }

                    self.handle_command(WorkerCommand::TickAnimationPlayback { delta_seconds });

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
                self.handle_select_asset(filtered_index, &path, kind);
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
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut preview,
                        &cam,
                    );
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
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut preview,
                        &cam,
                    );
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
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut preview,
                        &cam,
                    );
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
                        preview::build_composite_mesh_assembly(&stem, proj_dir.as_deref());

                    if has_composite && !composite_submeshes.is_empty() {
                        preview.is_composite = !preview.is_composite;
                        if preview.is_composite {
                            preview.submeshes = composite_submeshes;
                        } else {
                            preview.submeshes.truncate(1);
                        }

                        let cam = viewport::center_camera_for_preview(&self.state, &preview);
                        let is_comp_active = preview.is_composite;
                        let stats_lines = preview::build_stats_lines(&preview.submeshes);

                        {
                            let mut st = self.state.lock().unwrap();
                            st.active_mesh = Some(preview.clone());
                        }

                        viewport::evaluate_and_render_animated_frame(
                            &self.ui_handle,
                            &self.state,
                            &mut self.gpu_renderer,
                            &mut preview,
                            &cam,
                        );

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
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut preview,
                        &cam,
                    );
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
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut preview,
                        &cam,
                    );
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
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut p,
                        &cam,
                    );
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
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut p,
                        &cam,
                    );
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
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut p,
                        &cam,
                    );
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
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut p,
                        &cam,
                    );
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
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut p,
                        &cam,
                    );
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
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut preview,
                        &cam,
                    );
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
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut preview,
                        &cam,
                    );
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
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut preview,
                        &cam,
                    );
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
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut preview,
                        &cam,
                    );
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
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut preview,
                        &cam,
                    );
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
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut preview,
                        &cam,
                    );
                }
            }

            // Standalone direct operations
            WorkerCommand::Decompile8ldDirect { src, dst } => {
                direct_tools::handle_decompile_8ld_direct(&self.ui_handle, &self.logger, src, dst);
            }
            WorkerCommand::Compile8ldDirect { src, dst } => {
                direct_tools::handle_compile_8ld_direct(&self.ui_handle, &self.logger, src, dst);
            }
            WorkerCommand::Decompile8ldBatch { src_dir, dst_dir } => {
                direct_tools::handle_decompile_8ld_batch(
                    &self.ui_handle,
                    &self.logger,
                    src_dir,
                    dst_dir,
                );
            }
            WorkerCommand::Compile8ldBatch { src_dir, dst_dir } => {
                direct_tools::handle_compile_8ld_batch(
                    &self.ui_handle,
                    &self.logger,
                    src_dir,
                    dst_dir,
                );
            }
            WorkerCommand::DirectVpkToJson { src, dst } => {
                direct_tools::handle_direct_vpk_to_json(&self.ui_handle, &self.logger, src, dst);
            }
            WorkerCommand::DirectJsonToVpk {
                src_json,
                baseline_vpk,
                dst,
            } => {
                direct_tools::handle_direct_json_to_vpk(
                    &self.ui_handle,
                    &self.logger,
                    src_json,
                    baseline_vpk,
                    dst,
                );
            }
            WorkerCommand::DirectDtaToJson { src, dst } => {
                direct_tools::handle_direct_dta_to_json(&self.ui_handle, &self.logger, src, dst);
            }
            WorkerCommand::DirectJsonToDta {
                src_json,
                baseline_dta,
                dst,
            } => {
                direct_tools::handle_direct_json_to_dta(
                    &self.ui_handle,
                    &self.logger,
                    src_json,
                    baseline_dta,
                    dst,
                );
            }
            WorkerCommand::DirectEnvToJson { src, dst } => {
                direct_tools::handle_direct_env_to_json(&self.ui_handle, &self.logger, src, dst);
            }
            WorkerCommand::DirectJsonToEnv {
                src_json,
                baseline_env,
                dst,
            } => {
                direct_tools::handle_direct_json_to_env(
                    &self.ui_handle,
                    &self.logger,
                    src_json,
                    baseline_env,
                    dst,
                );
            }
            WorkerCommand::DirectMeshExport { src, dst, is_glb } => {
                direct_tools::handle_direct_mesh_export(
                    &self.ui_handle,
                    &self.logger,
                    src,
                    dst,
                    is_glb,
                );
            }
            WorkerCommand::DirectMeshImport {
                chunk_target,
                model_src,
                is_glb,
            } => {
                direct_tools::handle_direct_mesh_import(
                    &self.ui_handle,
                    &self.logger,
                    chunk_target,
                    model_src,
                    is_glb,
                );
            }
            WorkerCommand::DirectAssembleLevel {
                omp_path,
                assets_dir,
                dst,
            } => {
                direct_tools::handle_direct_assemble_level(
                    &self.ui_handle,
                    &self.logger,
                    omp_path,
                    assets_dir,
                    dst,
                );
            }
            WorkerCommand::DirectTerrainExport { src, dst, is_glb } => {
                direct_tools::handle_direct_terrain_export(
                    &self.ui_handle,
                    &self.logger,
                    src,
                    dst,
                    is_glb,
                );
            }
            WorkerCommand::DirectCollisionExport { src, dst } => {
                direct_tools::handle_direct_collision_export(
                    &self.ui_handle,
                    &self.logger,
                    src,
                    dst,
                );
            }
            WorkerCommand::DirectCollisionImport {
                chunk_target,
                glb_src,
            } => {
                direct_tools::handle_direct_collision_import(
                    &self.ui_handle,
                    &self.logger,
                    chunk_target,
                    glb_src,
                );
            }
            WorkerCommand::DirectFontToJson { src, dst } => {
                direct_tools::handle_direct_font_to_json(&self.ui_handle, &self.logger, src, dst);
            }
            WorkerCommand::DirectJsonToFont { src_json, dst } => {
                direct_tools::handle_direct_json_to_font(
                    &self.ui_handle,
                    &self.logger,
                    src_json,
                    dst,
                );
            }
            WorkerCommand::DirectTextureExport { src, dst } => {
                direct_tools::handle_direct_texture_export(&self.ui_handle, &self.logger, src, dst);
            }
            WorkerCommand::DirectTextureImport {
                chunk_target,
                img_src,
            } => {
                direct_tools::handle_direct_texture_import(
                    &self.ui_handle,
                    &self.logger,
                    chunk_target,
                    img_src,
                );
            }
            WorkerCommand::DirectAudioExport { src, dst } => {
                direct_tools::handle_direct_audio_export(&self.ui_handle, &self.logger, src, dst);
            }
            WorkerCommand::DirectAudioImport {
                chunk_target,
                wav_src,
            } => {
                direct_tools::handle_direct_audio_import(
                    &self.ui_handle,
                    &self.logger,
                    chunk_target,
                    wav_src,
                );
            }

            // Project operations
            WorkerCommand::UnpackArchive { src, dst } => {
                project_ops::handle_unpack_archive(
                    &self.ui_handle,
                    &self.logger,
                    &self.state,
                    src,
                    dst,
                );
            }
            WorkerCommand::LoadProject { proj_dir } => {
                project_ops::handle_load_project(
                    &self.ui_handle,
                    &self.logger,
                    &self.state,
                    proj_dir,
                );
            }
            WorkerCommand::CleanRebuild { proj_dir } => {
                project_ops::handle_clean_rebuild(
                    &self.ui_handle,
                    &self.logger,
                    &self.state,
                    proj_dir,
                );
            }
            WorkerCommand::RevertAsset {
                proj_dir,
                chunk_path,
            } => {
                project_ops::handle_revert_asset(
                    &self.ui_handle,
                    &self.logger,
                    &self.state,
                    proj_dir,
                    chunk_path,
                );
            }
            WorkerCommand::PackArchive { proj_dir } => {
                project_ops::handle_pack_archive(&self.ui_handle, &self.logger, proj_dir);
            }
            WorkerCommand::CreatePatch {
                base_dir,
                mod_dir,
                out_file,
            } => {
                project_ops::handle_create_patch(&self.logger, base_dir, mod_dir, out_file);
            }
            WorkerCommand::ApplyPatch {
                target_dir,
                patch_file,
            } => {
                project_ops::handle_apply_patch(&self.logger, target_dir, patch_file);
            }

            // Contextual asset operations
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
                    let ext = if is_glb { "glTF" } else { "OBJ" };
                    self.logger.log(&format!(
                        "[+] Terrain {} exported: {:?} ({} vertices, {} triangles)",
                        ext, out_path, v_count, tri_count
                    ));
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

    fn handle_select_asset(&mut self, filtered_index: i32, path: &Path, kind: AssetKind) {
        let bytes = fs::read(path).unwrap_or_default();
        let path_str = path.to_string_lossy().to_string();

        if kind != AssetKind::Mesh && kind != AssetKind::Object && kind != AssetKind::Character {
            let mut st = self.state.lock().unwrap();
            st.active_mesh = None;
        }

        match kind {
            AssetKind::Texture => {
                let mut tex_buf = None;
                let mut tex_desc = String::from("Invalid or corrupted texture");
                let mut ok = false;

                if let Ok(tex) = crate::engine::assets::texture::parse_texture_chunk(&bytes) {
                    let rgba = dds_decoder::decode_to_rgba(
                        tex.width,
                        tex.height,
                        tex.format,
                        &tex.pixel_data,
                    );
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
                    preview::load_mesh_with_smart_texture(&bytes, &stem, proj_dir.as_deref())
                {
                    let (_, has_composite) =
                        preview::build_composite_mesh_assembly(&stem, proj_dir.as_deref());

                    if submesh.bones.is_empty()
                        && let Some(master_bones) =
                            preview::find_master_skeleton(proj_dir.as_deref())
                    {
                        submesh.bones = master_bones;
                    }

                    let stats_lines = preview::build_stats_lines(std::slice::from_ref(&submesh));
                    let mesh_info = format!(
                        "Verts: {} | Tris: {}",
                        submesh.positions.len(),
                        submesh.indices.len() / 3
                    );

                    let clips = preview::discover_companion_animations(
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

                    let cam = viewport::center_camera_for_preview(&self.state, &preview);
                    let initial_duration = preview
                        .available_clips
                        .first()
                        .map(|c| c.duration_seconds)
                        .unwrap_or(1.0);

                    {
                        let mut st = self.state.lock().unwrap();
                        st.active_mesh = Some(preview.clone());
                    }

                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut preview,
                        &cam,
                    );

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
                    preview::build_composite_mesh_assembly(&stem, proj_dir.as_deref());

                let json = if kind == AssetKind::Character {
                    crate::engine::assets::character::export_character_to_json(&bytes, None, &stem)
                        .unwrap_or_default()
                } else {
                    crate::engine::assets::object::export_object_to_json(&bytes, None)
                        .unwrap_or_default()
                };

                let clips = preview::discover_companion_animations(
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
                    let cam = viewport::center_camera_for_preview(&self.state, &preview);
                    let initial_duration = preview
                        .available_clips
                        .first()
                        .map(|c| c.duration_seconds)
                        .unwrap_or(1.0);

                    {
                        let mut st = self.state.lock().unwrap();
                        st.active_mesh = Some(preview.clone());
                    }

                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut preview,
                        &cam,
                    );

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
                let json = crate::engine::assets::ui::export_ui_to_json(&bytes).unwrap_or_default();
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
                let json = crate::engine::assets::attachment::export_attachment_to_json(&bytes)
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
                if let Ok(clip) = crate::engine::assets::animation::parse_animation_clip(&bytes) {
                    info = format!(
                        "Clip: {} | Rig: {} | {:.1} FPS | Duration: {:.3}s | Tracks: {}",
                        clip.name,
                        clip.target_rig,
                        clip.frame_rate,
                        clip.duration_seconds,
                        clip.bone_tracks.len()
                    );
                }
                let json = crate::engine::assets::animation::export_animation_to_json(&bytes)
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
                    crate::engine::assets::terrain_palette::export_terrain_palette_to_json(&bytes)
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
                let json = crate::engine::assets::collision::export_collision_to_json(&bytes, "")
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
                let json = crate::engine::assets::environment::export_environment_to_json(&bytes)
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
                let json = crate::engine::assets::ui_sprite::export_ui_sprite_collection(&bytes)
                    .unwrap_or_else(|e| format!("Error parsing Sprite Collection: {}", e));
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
}
