pub mod camera;
pub mod gizmo;
pub mod grid;
pub mod pipelines;
pub mod targets;
pub mod texture;

pub use camera::{ViewportCamera, look_at_rh, perspective_rh_zo};
pub use gizmo::build_gpu_gizmo_vertices;
pub use grid::build_gpu_grid_vertices;
pub use texture::TextureData;

use anyhow::{Context, Result};
use glam::{Mat4, Vec3};
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::collections::HashMap;
use std::iter;
use std::time::{Duration, Instant};

use crate::engine::math::{GridVertex, Vector2, Vector3, Vector4};
use pipelines::{BonesUniform, Pipelines, SceneUniform, Vertex, create_pipelines};
use targets::{RenderTargetManager, STAGING_BUFFER_COUNT};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderOptions {
    pub is_skinning_enabled: bool,
    pub show_grid: bool,
    pub show_wire: bool,
    pub show_xray: bool,
    pub size: (u32, u32),
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
}

pub struct SubmeshDrawData<'a> {
    pub positions: &'a [Vector3],
    pub indices: &'a [u32],
    pub normals: &'a [Vector3],
    pub uvs: &'a [Vector2],
    pub joints: &'a [[u16; 4]],
    pub weights: &'a [Vector4],
    pub texture: Option<TextureData<'a>>,
}

struct CachedSubmeshBuffers {
    vertex_buffer: wgpu::Buffer,
    vertex_capacity: usize,
    index_buffer: wgpu::Buffer,
    index_capacity: usize,
    wire_buffer: Option<wgpu::Buffer>,
    wire_capacity: usize,
}

struct CachedTextureResource {
    _texture: wgpu::Texture,
    _view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

#[derive(Clone, PartialEq, Eq)]
struct RenderStateKey {
    width: u32,
    height: u32,
    yaw_bits: u32,
    pitch_bits: u32,
    dist_bits: u32,
    target_bits: [u32; 3],
    fov_bits: u32,
    lighting_mode: u32,
    up_axis: u32,
    show_grid: bool,
    show_wire: bool,
    show_xray: bool,
    is_skinning_enabled: bool,
    debug_lines_len: usize,
    submesh_count: usize,
    matrix_hash: u64,
    bounds_min_bits: [u32; 3],
    bounds_max_bits: [u32; 3],
}

struct PreparedDrawCall {
    submesh_idx: usize,
    index_count: u32,
    wire_count: u32,
    bind_group: wgpu::BindGroup,
}

pub struct WgpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: Pipelines,
    targets_manager: RenderTargetManager,
    last_frame_time: Instant,
    last_pixel_buffer: Option<SharedPixelBuffer<Rgba8Pixel>>,

    grid_buffer: Option<(wgpu::Buffer, usize)>,
    gizmo_buffer: Option<(wgpu::Buffer, usize)>,
    submesh_buffers: Vec<CachedSubmeshBuffers>,
    texture_cache: HashMap<(usize, u32, u32), CachedTextureResource>,
    last_render_key: Option<RenderStateKey>,
}

impl WgpuRenderer {
    pub fn new() -> Result<Self> {
        pollster::block_on(Self::init_async())
    }

    async fn init_async() -> Result<Self> {
        let instance = wgpu::Instance::default();

        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
        {
            Ok(a) => a,
            Err(_) => instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    compatible_surface: None,
                    force_fallback_adapter: true,
                    apply_limit_buckets: false,
                })
                .await
                .context("TREOVL: No hardware or software GPU adapter found on this system.")?,
        };

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .context("TREOVL: Failed to create WGPU logical device and queue.")?;

        let pipelines = create_pipelines(&device, &queue)?;

        Ok(Self {
            device,
            queue,
            pipelines,
            targets_manager: RenderTargetManager::new(),
            last_frame_time: Instant::now(),
            last_pixel_buffer: None,
            grid_buffer: None,
            gizmo_buffer: None,
            submesh_buffers: Vec::new(),
            texture_cache: HashMap::new(),
            last_render_key: None,
        })
    }

    pub fn render(
        &mut self,
        submeshes: &[SubmeshDrawData],
        debug_lines: &[GridVertex],
        skin_matrices: &[Mat4],
        options: RenderOptions,
        cam: &ViewportCamera,
    ) -> Result<SharedPixelBuffer<Rgba8Pixel>> {
        let (width, height) = options.size;
        let is_skinning_enabled = options.is_skinning_enabled;
        let show_grid = options.show_grid;
        let show_wire = options.show_wire;

        let mut matrix_hash: u64 = 0xcbf29ce484222325;
        for m in skin_matrices {
            for val in m.to_cols_array() {
                matrix_hash = matrix_hash.wrapping_mul(0x100000001b3) ^ (val.to_bits() as u64);
            }
        }

        let current_key = RenderStateKey {
            width,
            height,
            yaw_bits: cam.yaw.to_bits(),
            pitch_bits: cam.pitch.to_bits(),
            dist_bits: cam.distance.to_bits(),
            target_bits: [
                cam.target.x.to_bits(),
                cam.target.y.to_bits(),
                cam.target.z.to_bits(),
            ],
            fov_bits: cam.fov_degrees.to_bits(),
            lighting_mode: cam.lighting_mode,
            up_axis: cam.up_axis,
            show_grid,
            show_wire,
            show_xray: options.show_xray,
            is_skinning_enabled,
            debug_lines_len: debug_lines.len(),
            submesh_count: submeshes.len(),
            matrix_hash,
            bounds_min_bits: [
                options.bounds_min[0].to_bits(),
                options.bounds_min[1].to_bits(),
                options.bounds_min[2].to_bits(),
            ],
            bounds_max_bits: [
                options.bounds_max[0].to_bits(),
                options.bounds_max[1].to_bits(),
                options.bounds_max[2].to_bits(),
            ],
        };

        if self.last_render_key.as_ref() == Some(&current_key)
            && let Some(ref prev) = self.last_pixel_buffer
        {
            return Ok(prev.clone());
        }

        let now = Instant::now();
        if now.duration_since(self.last_frame_time) < Duration::from_millis(15)
            && let Some(ref prev) = self.last_pixel_buffer
        {
            return Ok(prev.clone());
        }
        self.last_frame_time = now;

        let min = Vec3::from_array(options.bounds_min);
        let max = Vec3::from_array(options.bounds_max);
        let center = if min.x.is_finite() {
            (min + max) * 0.5
        } else {
            Vec3::ZERO
        };

        let aspect = width as f32 / height as f32;
        let fov_rad = cam.fov_degrees.to_radians().clamp(0.4, 2.0);
        let proj = perspective_rh_zo(fov_rad, aspect, 0.1, 2000.0);

        let eye = center
            + Vec3::new(
                cam.yaw.sin() * cam.pitch.cos() * cam.distance,
                cam.pitch.sin() * cam.distance,
                cam.yaw.cos() * cam.pitch.cos() * cam.distance,
            );
        let view = look_at_rh(eye, center, Vec3::Y);
        let view_proj = proj * view;

        let scene_uniform = SceneUniform {
            view_proj: view_proj.to_cols_array(),
            params: [
                cam.lighting_mode as f32,
                if is_skinning_enabled && !skin_matrices.is_empty() {
                    1.0
                } else {
                    0.0
                },
                cam.up_axis as f32,
                0.0,
            ],
        };

        self.queue.write_buffer(
            &self.pipelines.scene_buffer,
            0,
            bytemuck::cast_slice(&[scene_uniform]),
        );

        if is_skinning_enabled && !skin_matrices.is_empty() {
            let mut bones_data = BonesUniform {
                matrices: [Mat4::IDENTITY.to_cols_array(); 128],
            };
            for (idx, mat) in skin_matrices.iter().take(128).enumerate() {
                bones_data.matrices[idx] = mat.to_cols_array();
            }
            self.queue.write_buffer(
                &self.pipelines.bones_buffer,
                0,
                bytemuck::cast_slice(&[bones_data]),
            );
        }

        let floor_y = if min.y.is_finite() { min.y } else { 0.0 };
        let model_radius = if min.x.is_finite() {
            (max - min).length() * 0.5
        } else {
            2.0
        };

        let mut grid_vertices = Vec::new();
        if show_grid {
            grid_vertices = build_gpu_grid_vertices(center, floor_y, model_radius);
        }

        let grid_count = grid_vertices.len() as u32;
        let skeleton_count = debug_lines.len() as u32;
        let total_line_count = grid_count + skeleton_count;

        let mut all_lines = Vec::with_capacity(total_line_count as usize);
        all_lines.extend_from_slice(&grid_vertices);
        all_lines.extend_from_slice(debug_lines);

        if total_line_count > 0 {
            let need_new = match &self.grid_buffer {
                Some((_, cap)) => *cap < all_lines.len(),
                None => true,
            };
            if need_new {
                let new_cap = all_lines.len().next_power_of_two().max(512);
                let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Persistent Grid & Lines Buffer"),
                    size: (new_cap * std::mem::size_of::<GridVertex>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.grid_buffer = Some((buf, new_cap));
            }
            if let Some((ref buf, _)) = self.grid_buffer {
                self.queue
                    .write_buffer(buf, 0, bytemuck::cast_slice(&all_lines));
            }
        }

        let gizmo_vertices = build_gpu_gizmo_vertices(cam);
        if !gizmo_vertices.is_empty() {
            let need_new = match &self.gizmo_buffer {
                Some((_, cap)) => *cap < gizmo_vertices.len(),
                None => true,
            };
            if need_new {
                let new_cap = gizmo_vertices.len().next_power_of_two().max(160);
                let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Persistent Gizmo Vertex Buffer"),
                    size: (new_cap * std::mem::size_of::<GridVertex>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.gizmo_buffer = Some((buf, new_cap));
            }
            if let Some((ref buf, _)) = self.gizmo_buffer {
                self.queue
                    .write_buffer(buf, 0, bytemuck::cast_slice(&gizmo_vertices));
            }
        }

        while self.submesh_buffers.len() < submeshes.len() {
            self.submesh_buffers.push(CachedSubmeshBuffers {
                vertex_buffer: self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Persistent Vertex Buffer"),
                    size: (1024 * std::mem::size_of::<Vertex>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                vertex_capacity: 1024,
                index_buffer: self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Persistent Index Buffer"),
                    size: (1024 * std::mem::size_of::<u32>()) as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                index_capacity: 1024,
                wire_buffer: None,
                wire_capacity: 0,
            });
        }

        let mut draw_calls = Vec::with_capacity(submeshes.len());

        let transform_pos_bounds = |p: Vector3| -> [f32; 3] {
            // Unify with WGSL mapping (X, -Z, Y)
            let aligned = [p.x, -p.z, p.y];
            match cam.up_axis {
                1 => [aligned[0], -aligned[2], aligned[1]],
                2 => [aligned[0], aligned[2], -aligned[1]],
                3 => [aligned[0], -aligned[1], -aligned[2]],
                _ => aligned,
            }
        };

        for (sm_idx, sm) in submeshes.iter().enumerate() {
            if sm.indices.is_empty() {
                continue;
            }

            let vertices: Vec<Vertex> = sm
                .positions
                .iter()
                .enumerate()
                .map(|(i, &p)| {
                    let n = sm.normals.get(i).copied().unwrap_or(Vector3 {
                        x: 0.0,
                        y: 1.0,
                        z: 0.0,
                    });
                    let uv = sm.uvs.get(i).copied().unwrap_or(Vector2 { x: 0.0, y: 0.0 });
                    let j_raw = sm.joints.get(i).copied().unwrap_or([0, 0, 0, 0]);
                    let w_raw = sm.weights.get(i).copied().unwrap_or(Vector4 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                        w: 0.0,
                    });

                    Vertex {
                        position: [p.x, p.y, p.z],
                        normal: [n.x, n.y, n.z],
                        tex_coords: [uv.x, uv.y],
                        joints: [
                            (j_raw[0] as usize).min(127) as u32,
                            (j_raw[1] as usize).min(127) as u32,
                            (j_raw[2] as usize).min(127) as u32,
                            (j_raw[3] as usize).min(127) as u32,
                        ],
                        weights: [w_raw.x, w_raw.y, w_raw.z, w_raw.w],
                    }
                })
                .collect();

            let wire_count = {
                let cached = &mut self.submesh_buffers[sm_idx];

                if vertices.len() > cached.vertex_capacity {
                    let new_cap = vertices.len().next_power_of_two();
                    cached.vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Resized Persistent Vertex Buffer"),
                        size: (new_cap * std::mem::size_of::<Vertex>()) as u64,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    cached.vertex_capacity = new_cap;
                }
                self.queue
                    .write_buffer(&cached.vertex_buffer, 0, bytemuck::cast_slice(&vertices));

                if sm.indices.len() > cached.index_capacity {
                    let new_cap = sm.indices.len().next_power_of_two();
                    cached.index_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Resized Persistent Index Buffer"),
                        size: (new_cap * std::mem::size_of::<u32>()) as u64,
                        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    cached.index_capacity = new_cap;
                }
                self.queue
                    .write_buffer(&cached.index_buffer, 0, bytemuck::cast_slice(sm.indices));

                if show_wire {
                    let mut wire_vertices = Vec::with_capacity(sm.indices.len() * 2);

                    let get_skinned_pos = |idx: usize| -> Vector3 {
                        let raw_p = sm.positions.get(idx).copied().unwrap_or_default();

                        if is_skinning_enabled && !skin_matrices.is_empty() {
                            let j = sm.joints.get(idx).copied().unwrap_or([0, 0, 0, 0]);
                            let w = sm.weights.get(idx).copied().unwrap_or(Vector4 {
                                x: 1.0,
                                y: 0.0,
                                z: 0.0,
                                w: 0.0,
                            });
                            let w_sum = w.x + w.y + w.z + w.w;
                            if w_sum > 0.001 {
                                let get_m = |joint_id: u16| -> Mat4 {
                                    let j_idx = (joint_id as usize).min(127);
                                    skin_matrices.get(j_idx).copied().unwrap_or(Mat4::IDENTITY)
                                };
                                let m0 = get_m(j[0]);
                                let m1 = get_m(j[1]);
                                let m2 = get_m(j[2]);
                                let m3 = get_m(j[3]);

                                let blended_m = m0 * w.x + m1 * w.y + m2 * w.z + m3 * w.w;
                                let p4 = blended_m
                                    .transform_point3(Vec3::new(raw_p.x, raw_p.y, raw_p.z));
                                return Vector3 {
                                    x: p4.x,
                                    y: p4.y,
                                    z: p4.z,
                                };
                            }
                        }
                        raw_p
                    };

                    for tri in sm.indices.as_chunks::<3>().0 {
                        let p0 = transform_pos_bounds(get_skinned_pos(tri[0] as usize));
                        let p1 = transform_pos_bounds(get_skinned_pos(tri[1] as usize));
                        let p2 = transform_pos_bounds(get_skinned_pos(tri[2] as usize));
                        let color = [0.85, 0.9, 1.0, 0.85];
                        wire_vertices.push(GridVertex {
                            position: p0,
                            color,
                        });
                        wire_vertices.push(GridVertex {
                            position: p1,
                            color,
                        });
                        wire_vertices.push(GridVertex {
                            position: p1,
                            color,
                        });
                        wire_vertices.push(GridVertex {
                            position: p2,
                            color,
                        });
                        wire_vertices.push(GridVertex {
                            position: p2,
                            color,
                        });
                        wire_vertices.push(GridVertex {
                            position: p0,
                            color,
                        });
                    }

                    let req = wire_vertices.len();
                    if cached.wire_capacity < req {
                        let new_cap = req.next_power_of_two();
                        cached.wire_buffer =
                            Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                                label: Some("Resized Persistent Wire Buffer"),
                                size: (new_cap * std::mem::size_of::<GridVertex>()) as u64,
                                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                                mapped_at_creation: false,
                            }));
                        cached.wire_capacity = new_cap;
                    }
                    if let Some(ref w_buf) = cached.wire_buffer {
                        self.queue
                            .write_buffer(w_buf, 0, bytemuck::cast_slice(&wire_vertices));
                    }
                    req as u32
                } else {
                    0
                }
            };

            let bind_group = if let Some(ref t) = sm.texture {
                let tex_key = (t.rgba.as_ptr() as usize, t.width, t.height);

                if !self.texture_cache.contains_key(&tex_key) {
                    let extent = wgpu::Extent3d {
                        width: t.width,
                        height: t.height,
                        depth_or_array_layers: 1,
                    };
                    let wgpu_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("Cached Texture"),
                        size: extent,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    });

                    self.queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &wgpu_texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        t.rgba,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(t.width * 4),
                            rows_per_image: Some(t.height),
                        },
                        extent,
                    );

                    let view = wgpu_texture.create_view(&wgpu::TextureViewDescriptor::default());
                    let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("Cached Texture Bind Group"),
                        layout: &self.pipelines.texture_bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(
                                    &self.pipelines.default_sampler,
                                ),
                            },
                        ],
                    });

                    self.texture_cache.insert(
                        tex_key,
                        CachedTextureResource {
                            _texture: wgpu_texture,
                            _view: view,
                            bind_group: bg,
                        },
                    );
                }

                self.texture_cache.get(&tex_key).unwrap().bind_group.clone()
            } else {
                self.pipelines.default_texture_bind_group.clone()
            };

            draw_calls.push(PreparedDrawCall {
                submesh_idx: sm_idx,
                index_count: sm.indices.len() as u32,
                wire_count,
                bind_group,
            });
        }

        let targets = self
            .targets_manager
            .get_or_create(&self.device, width, height);
        let staging_idx = targets.current_staging_idx;
        targets.current_staging_idx = (staging_idx + 1) % STAGING_BUFFER_COUNT;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Main Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Main Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.render_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.11,
                            g: 0.11,
                            b: 0.13,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            // 1. Draw Mesh (if visible)
            if !show_wire {
                render_pass.set_pipeline(&self.pipelines.mesh_pipeline);
                render_pass.set_bind_group(0, &self.pipelines.scene_bind_group, &[]);

                for dc in &draw_calls {
                    let buf = &self.submesh_buffers[dc.submesh_idx];
                    render_pass.set_bind_group(1, &dc.bind_group, &[]);
                    render_pass.set_vertex_buffer(0, buf.vertex_buffer.slice(..));
                    render_pass
                        .set_index_buffer(buf.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.draw_indexed(0..dc.index_count, 0, 0..1);
                }
            } else {
                // If showing wireframe, render the mesh wireframe using grid pipeline
                render_pass.set_pipeline(&self.pipelines.grid_pipeline);
                render_pass.set_bind_group(0, &self.pipelines.scene_bind_group, &[]);

                for dc in &draw_calls {
                    let buf = &self.submesh_buffers[dc.submesh_idx];
                    if let Some(ref w_buf) = buf.wire_buffer {
                        render_pass.set_vertex_buffer(0, w_buf.slice(..));
                        render_pass.draw(0..dc.wire_count, 0..1);
                    }
                }
            }

            // 2. Draw Floor Grid & Skeleton Lines
            if total_line_count > 0
                && let Some((ref g_buf, _)) = self.grid_buffer
            {
                render_pass.set_bind_group(0, &self.pipelines.scene_bind_group, &[]);
                render_pass.set_vertex_buffer(0, g_buf.slice(..));

                // Draw Grid (Depth Tested)
                if grid_count > 0 {
                    render_pass.set_pipeline(&self.pipelines.grid_pipeline);
                    render_pass.draw(0..grid_count, 0..1);
                }

                // Draw Skeleton
                if skeleton_count > 0 {
                    if options.show_xray {
                        render_pass.set_pipeline(&self.pipelines.skeleton_xray_pipeline);
                    } else {
                        render_pass.set_pipeline(&self.pipelines.skeleton_depth_pipeline);
                    }
                    render_pass.draw(grid_count..(grid_count + skeleton_count), 0..1);
                }
            }

            // 3. Draw UI Axis Gizmo on top
            if !gizmo_vertices.is_empty()
                && let Some((ref gz_buf, _)) = self.gizmo_buffer
            {
                render_pass.set_pipeline(&self.pipelines.gizmo_pipeline);
                render_pass.set_vertex_buffer(0, gz_buf.slice(..));
                render_pass.draw(0..gizmo_vertices.len() as u32, 0..1);
            }
        }

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &targets.render_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &targets.staging_buffers[staging_idx],
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(targets.padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let submission_index = self.queue.submit(iter::once(encoder.finish()));

        let active_buffer = &targets.staging_buffers[staging_idx];
        let buffer_slice = active_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });

        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission_index),
                timeout: None,
            })
            .context("WGPU device poll failed during frame rendering")?;

        rx.recv()
            .context("WGPU map channel closed unexpectedly")?
            .context("WGPU staging buffer mapping failed")?;

        let data = buffer_slice
            .get_mapped_range()
            .context("TREOVL: Failed to get mapped memory range from WGPU staging buffer")?;

        let mut pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
        let dst = pixel_buffer.make_mut_bytes();

        let padded_stride = targets.padded_bytes_per_row as usize;
        let unpadded_stride = targets.unpadded_bytes_per_row as usize;

        for y in 0..height as usize {
            let src_start = y * padded_stride;
            let dst_start = y * unpadded_stride;
            dst[dst_start..dst_start + unpadded_stride]
                .copy_from_slice(&data[src_start..src_start + unpadded_stride]);
        }

        drop(data);
        active_buffer.unmap();

        self.last_render_key = Some(current_key);
        self.last_pixel_buffer = Some(pixel_buffer.clone());
        Ok(pixel_buffer)
    }
}
