pub mod direct_tools;
pub mod inspectors;
pub mod preview;
pub mod project_ops;
pub mod viewport;

pub use preview::ResolvedTexture;

use parking_lot::Mutex;
use std::sync::{Arc, mpsc::Receiver};

use slint::{ModelRc, VecModel};

use crate::AppWindow;
use crate::engine::service;
use crate::gui::AppState;
use crate::gui::commands::WorkerCommand;
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
                    let mut st = self.state.lock();
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
                inspectors::handle_select_asset(
                    &self.ui_handle,
                    &self.state,
                    &self.logger,
                    &mut self.gpu_renderer,
                    filtered_index,
                    &path,
                    kind,
                );
            }

            WorkerCommand::SelectAnimation { clip_index } => {
                let mut re_render_opt = None;
                {
                    let mut st = self.state.lock();
                    if let Some(ref mut preview) = st.active_mesh {
                        if clip_index == 0 {
                            preview.current_clip_index = None;
                            preview.current_time_seconds = 0.0;

                            let ui_h = self.ui_handle.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_h.upgrade() {
                                    ui.set_anim_duration(0.0);
                                    ui.set_anim_current_time(0.0);
                                    ui.set_is_anim_playing(false);
                                }
                            });

                            re_render_opt = Some((preview.clone(), st.camera));
                        } else if clip_index > 0
                            && ((clip_index - 1) as usize) < preview.available_clips.len()
                        {
                            let actual_clip_idx = (clip_index - 1) as usize;
                            preview.current_clip_index = Some(actual_clip_idx);
                            preview.current_time_seconds = 0.0;
                            let clip = &preview.available_clips[actual_clip_idx];
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
                    let mut st = self.state.lock();
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
                    let mut st = self.state.lock();
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

            WorkerCommand::SetViewportInteracting { is_active } => {
                let mut re_render_opt = None;
                {
                    let mut st = self.state.lock();
                    if st.is_interacting != is_active {
                        st.is_interacting = is_active;
                        if let Some(ref mut preview) = st.active_mesh {
                            re_render_opt = Some((preview.clone(), st.camera));
                        }
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

            WorkerCommand::ToggleCompositeView => {
                let (proj_dir, active_preview_opt) = {
                    let st = self.state.lock();
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
                            let mut st = self.state.lock();
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
                    let mut st = self.state.lock();
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
                    let mut st = self.state.lock();
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
                    let mut st = self.state.lock();
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
                    let cam = self.state.lock().camera;
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
                    let mut st = self.state.lock();
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
                    let cam = self.state.lock().camera;
                    viewport::evaluate_and_render_animated_frame(
                        &self.ui_handle,
                        &self.state,
                        &mut self.gpu_renderer,
                        &mut p,
                        &cam,
                    );
                }
            }

            WorkerCommand::ToggleXRay => {
                let (preview_opt, val) = {
                    let mut st = self.state.lock();
                    st.show_xray = !st.show_xray;
                    (st.active_mesh.clone(), st.show_xray)
                };
                let ui_h = self.ui_handle.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_h.upgrade() {
                        ui.set_show_xray(val);
                    }
                });
                if let Some(mut p) = preview_opt {
                    let cam = self.state.lock().camera;
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
                    let mut st = self.state.lock();
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
                    let cam = self.state.lock().camera;
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
                    let mut st = self.state.lock();
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
                    let cam = self.state.lock().camera;
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
                    let mut st = self.state.lock();
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
                    let cam = self.state.lock().camera;
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
                    let mut st = self.state.lock();
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
                    let mut st = self.state.lock();
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
                    let mut st = self.state.lock();
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
                    let mut st = self.state.lock();
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
                    let mut st = self.state.lock();
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
                    let mut st = self.state.lock();
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
}
