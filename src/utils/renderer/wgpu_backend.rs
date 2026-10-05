use super::camera::ViewportCamera;
use super::gizmo::render_axis_gizmo;
use super::grid::{GridVertex, build_gpu_grid_vertices};
use super::texture::TextureData;
use crate::engine::math::{Vector2, Vector3};
use glam::{Mat4, Vec3};
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::iter;
use std::time::{Duration, Instant};
use wgpu::util::DeviceExt;

/// Internal Mesh vertex format aligned for WGPU buffer layouts
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    tex_coords: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SceneUniform {
    view_proj: [f32; 16],
    params: [f32; 4], // x: lighting_mode, y: ambient_boost, z, w: padding
}

/// Render descriptor for an individual submesh in a single or composite draw call
pub struct SubmeshDrawData<'a> {
    pub positions: &'a [Vector3],
    pub indices: &'a [u32],
    pub normals: &'a [Vector3],
    pub uvs: &'a [Vector2],
    pub texture: Option<TextureData<'a>>,
}

fn perspective_rh_zo(fov_y_radians: f32, aspect_ratio: f32, z_near: f32, z_far: f32) -> Mat4 {
    let f = 1.0 / (fov_y_radians / 2.0).tan();
    Mat4::from_cols_array(&[
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

fn look_at_rh(eye: Vec3, center: Vec3, up: Vec3) -> Mat4 {
    let f = (center - eye).normalize();
    let s = f.cross(up).normalize();
    let u = s.cross(f);
    Mat4::from_cols_array(&[
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

struct PersistentRenderTarget {
    width: u32,
    height: u32,
    render_texture: wgpu::Texture,
    render_view: wgpu::TextureView,
    _depth_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,
    output_buffer: wgpu::Buffer,
    padded_bytes_per_row: u32,
    unpadded_bytes_per_row: u32,
}

pub struct WgpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    mesh_pipeline: wgpu::RenderPipeline,
    grid_pipeline: wgpu::RenderPipeline,
    scene_buffer: wgpu::Buffer,
    scene_bind_group: wgpu::BindGroup,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    default_sampler: wgpu::Sampler,
    default_texture_bind_group: wgpu::BindGroup,
    target_cache: Option<PersistentRenderTarget>,
    last_frame_time: Instant,
    last_pixel_buffer: Option<SharedPixelBuffer<Rgba8Pixel>>,
}

impl Default for WgpuRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl WgpuRenderer {
    pub fn new() -> Self {
        pollster::block_on(Self::init_async())
    }

    async fn init_async() -> Self {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("TREOVL: Failed to find an appropriate GPU adapter");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .expect("TREOVL: Failed to create WGPU device");

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("TREOVL Main Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let scene_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Scene Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let scene_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Persistent Scene Uniform Buffer"),
            size: std::mem::size_of::<SceneUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let scene_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Persistent Scene Bind Group"),
            layout: &scene_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: scene_buffer.as_entire_binding(),
            }],
        });

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Texture Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let default_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Default Texture Sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // 1x1 neutral gray fallback texture
        let fallback_extent = wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        };
        let fallback_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("1x1 Neutral Gray Texture"),
            size: fallback_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &fallback_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[185u8, 190, 200, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            fallback_extent,
        );

        let fallback_view = fallback_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let default_texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Fallback Texture Bind Group"),
            layout: &texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&fallback_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&default_sampler),
                },
            ],
        });

        let mesh_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Mesh Pipeline Layout"),
            bind_group_layouts: &[
                Some(&scene_bind_group_layout),
                Some(&texture_bind_group_layout),
            ],
            immediate_size: 0,
        });

        let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("TREOVL Mesh Render Pipeline"),
            layout: Some(&mesh_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let grid_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Grid Pipeline Layout"),
            bind_group_layouts: &[Some(&scene_bind_group_layout)],
            immediate_size: 0,
        });

        let grid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("TREOVL 3D Ground Grid Pipeline"),
            layout: Some(&grid_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_grid"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GridVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_grid"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            device,
            queue,
            mesh_pipeline,
            grid_pipeline,
            scene_buffer,
            scene_bind_group,
            texture_bind_group_layout,
            default_sampler,
            default_texture_bind_group,
            target_cache: None,
            last_frame_time: Instant::now(),
            last_pixel_buffer: None,
        }
    }

    fn ensure_render_targets(&mut self, width: u32, height: u32) {
        if let Some(ref cache) = self.target_cache
            && cache.width == width
            && cache.height == height
        {
            return;
        }

        let render_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Persistent Render Target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let render_view = render_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let depth_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Persistent Depth Target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let unpadded_bytes_per_row = width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padding = (align - unpadded_bytes_per_row % align) % align;
        let padded_bytes_per_row = unpadded_bytes_per_row + padding;

        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Persistent VRAM to RAM Staging Buffer"),
            size: (padded_bytes_per_row * height) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.target_cache = Some(PersistentRenderTarget {
            width,
            height,
            render_texture,
            render_view,
            _depth_texture: depth_texture,
            depth_view,
            output_buffer,
            padded_bytes_per_row,
            unpadded_bytes_per_row,
        });
    }

    /// Renders single or multiple submeshes (composite character/object) in a single pass.
    pub fn render(
        &mut self,
        submeshes: &[SubmeshDrawData],
        debug_lines: &[GridVertex],
        size: (u32, u32),
        cam: &ViewportCamera,
    ) -> SharedPixelBuffer<Rgba8Pixel> {
        let (width, height) = size;

        let now = Instant::now();
        if now.duration_since(self.last_frame_time) < Duration::from_millis(15)
            && let Some(ref prev) = self.last_pixel_buffer
        {
            return prev.clone();
        }
        self.last_frame_time = now;

        self.ensure_render_targets(width, height);

        let transform_pos = |p: Vector3| -> [f32; 3] {
            match cam.up_axis {
                1 => [p.x, -p.z, p.y],
                2 => [p.x, p.z, -p.y],
                _ => [p.x, p.y, p.z],
            }
        };

        let transform_norm = |n: Vector3| -> [f32; 3] {
            match cam.up_axis {
                1 => [n.x, -n.z, n.y],
                2 => [n.x, n.z, -n.y],
                _ => [n.x, n.y, n.z],
            }
        };

        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);

        for sm in submeshes {
            for &p in sm.positions {
                let tp = transform_pos(p);
                min.x = min.x.min(tp[0]);
                min.y = min.y.min(tp[1]);
                min.z = min.z.min(tp[2]);
                max.x = max.x.max(tp[0]);
                max.y = max.y.max(tp[1]);
                max.z = max.z.max(tp[2]);
            }
        }

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
            params: [cam.lighting_mode as f32, 0.0, 0.0, 0.0],
        };

        self.queue.write_buffer(
            &self.scene_buffer,
            0,
            bytemuck::cast_slice(&[scene_uniform]),
        );

        let floor_y = if min.y.is_finite() { min.y } else { 0.0 };
        let model_radius = if min.x.is_finite() {
            (max - min).length() * 0.5
        } else {
            2.0
        };

        let mut all_lines = build_gpu_grid_vertices(center, floor_y, model_radius);
        all_lines.extend_from_slice(debug_lines);

        let grid_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Grid & Skeleton Line Vertex Buffer"),
                contents: bytemuck::cast_slice(&all_lines),
                usage: wgpu::BufferUsages::VERTEX,
            });

        struct PreparedSubmesh {
            vertex_buffer: wgpu::Buffer,
            index_buffer: wgpu::Buffer,
            index_count: u32,
            bind_group: wgpu::BindGroup,
        }

        let mut prepared_submeshes = Vec::with_capacity(submeshes.len());

        for sm in submeshes {
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
                    Vertex {
                        position: transform_pos(p),
                        normal: transform_norm(n),
                        tex_coords: [uv.x, uv.y],
                    }
                })
                .collect();

            let vertex_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Submesh Vertex Buffer"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });

            let index_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Submesh Index Buffer"),
                    contents: bytemuck::cast_slice(sm.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

            let bind_group = if let Some(ref t) = sm.texture {
                let texture_extent = wgpu::Extent3d {
                    width: t.width,
                    height: t.height,
                    depth_or_array_layers: 1,
                };
                let wgpu_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Submesh Texture"),
                    size: texture_extent,
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
                    texture_extent,
                );

                let texture_view =
                    wgpu_texture.create_view(&wgpu::TextureViewDescriptor::default());
                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Submesh Texture Bind Group"),
                    layout: &self.texture_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&texture_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.default_sampler),
                        },
                    ],
                })
            } else {
                self.default_texture_bind_group.clone()
            };

            prepared_submeshes.push(PreparedSubmesh {
                vertex_buffer,
                index_buffer,
                index_count: sm.indices.len() as u32,
                bind_group,
            });
        }

        let targets = self.target_cache.as_ref().unwrap();

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

            if !all_lines.is_empty() {
                render_pass.set_pipeline(&self.grid_pipeline);
                render_pass.set_bind_group(0, &self.scene_bind_group, &[]);
                render_pass.set_vertex_buffer(0, grid_buffer.slice(..));
                render_pass.draw(0..all_lines.len() as u32, 0..1);
            }

            render_pass.set_pipeline(&self.mesh_pipeline);
            render_pass.set_bind_group(0, &self.scene_bind_group, &[]);

            for psm in &prepared_submeshes {
                render_pass.set_bind_group(1, &psm.bind_group, &[]);
                render_pass.set_vertex_buffer(0, psm.vertex_buffer.slice(..));
                render_pass.set_index_buffer(psm.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..psm.index_count, 0, 0..1);
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
                buffer: &targets.output_buffer,
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

        let buffer_slice = targets.output_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |v| tx.send(v).unwrap());

        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission_index),
                timeout: None,
            })
            .unwrap();

        rx.recv().unwrap().unwrap();

        let data = buffer_slice
            .get_mapped_range()
            .expect("Failed to get mapped memory range from WGPU");

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
        targets.output_buffer.unmap();

        render_axis_gizmo(dst, width, height, cam);

        self.last_pixel_buffer = Some(pixel_buffer.clone());
        pixel_buffer
    }
}
