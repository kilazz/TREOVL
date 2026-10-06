use super::camera::ViewportCamera;
use super::gizmo::render_axis_gizmo;
use super::grid::{GridVertex, build_gpu_grid_vertices};
use super::texture::TextureData;
use crate::engine::math::{Vector2, Vector3, Vector4};
use anyhow::{Context, Result};
use glam::{Mat4, Vec3};
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::collections::HashMap;
use std::iter;
use std::time::{Duration, Instant};

const STAGING_BUFFER_COUNT: usize = 2;

/// Render settings and viewport flags passed to the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderOptions {
    pub is_skinning_enabled: bool,
    pub show_grid: bool,
    pub show_wire: bool,
    pub size: (u32, u32),
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
}

/// Internal mesh vertex format aligned for WGPU buffer layouts with GPU skinning attributes.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    tex_coords: [f32; 2],
    joints: [u32; 4],
    weights: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SceneUniform {
    view_proj: [f32; 16],
    params: [f32; 4], // x: lighting_mode, y: is_skinned, z: up_axis, w: padding
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BonesUniform {
    matrices: [[f32; 16]; 128],
}

/// Render descriptor for an individual submesh in a single or composite draw call.
pub struct SubmeshDrawData<'a> {
    pub positions: &'a [Vector3],
    pub indices: &'a [u32],
    pub normals: &'a [Vector3],
    pub uvs: &'a [Vector2],
    pub joints: &'a [[u16; 4]],
    pub weights: &'a [Vector4],
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
    staging_buffers: [wgpu::Buffer; STAGING_BUFFER_COUNT],
    current_staging_idx: usize,
    padded_bytes_per_row: u32,
    unpadded_bytes_per_row: u32,
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
    mesh_pipeline: wgpu::RenderPipeline,
    grid_pipeline: wgpu::RenderPipeline,
    scene_buffer: wgpu::Buffer,
    bones_buffer: wgpu::Buffer,
    scene_bind_group: wgpu::BindGroup,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    default_sampler: wgpu::Sampler,
    default_texture_bind_group: wgpu::BindGroup,
    target_cache: Option<PersistentRenderTarget>,
    last_frame_time: Instant,
    last_pixel_buffer: Option<SharedPixelBuffer<Rgba8Pixel>>,

    grid_buffer: Option<(wgpu::Buffer, usize)>,
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

        let shader_src = r#"
struct SceneUniform {
    view_proj: mat4x4<f32>,
    params: vec4<f32>, // x: lighting_mode, y: is_skinned, z: up_axis, w: padding
};

struct BonesUniform {
    matrices: array<mat4x4<f32>, 128>,
};

@group(0) @binding(0)
var<uniform> scene: SceneUniform;

@group(0) @binding(1)
var<uniform> bones: BonesUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tex_coords: vec2<f32>,
    @location(3) joints: vec4<u32>,
    @location(4) weights: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
};

@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    var local_pos = vec4<f32>(model.position, 1.0);
    var local_norm = model.normal;

    // 1. Apply vertex skinning in pure native model coordinates (Triumph engine space)
    let is_skinned = scene.params.y > 0.5;
    if (is_skinned && (model.weights.x + model.weights.y + model.weights.z + model.weights.w) > 0.001) {
        let bone_m = model.weights.x * bones.matrices[model.joints.x]
                   + model.weights.y * bones.matrices[model.joints.y]
                   + model.weights.z * bones.matrices[model.joints.z]
                   + model.weights.w * bones.matrices[model.joints.w];

        local_pos = bone_m * local_pos;
        local_norm = (bone_m * vec4<f32>(local_norm, 0.0)).xyz;
    }

    // 2. Map coordinates to WGPU Viewport (Y-up: X=Right, Y=-Z (Height Up), Z=Y (Forward))
    let base_world_pos = vec3<f32>(local_pos.x, -local_pos.z, local_pos.y);
    let base_world_norm = vec3<f32>(local_norm.x, -local_norm.z, local_norm.y);

    let up_axis = u32(scene.params.z);
    var world_pos = base_world_pos;
    var world_norm = base_world_norm;

    if (up_axis == 1u) {
        world_pos = vec3<f32>(base_world_pos.x, -base_world_pos.z, base_world_pos.y);
        world_norm = vec3<f32>(base_world_norm.x, -base_world_norm.z, base_world_norm.y);
    } else if (up_axis == 2u) {
        world_pos = vec3<f32>(base_world_pos.x, base_world_pos.z, -base_world_pos.y);
        world_norm = vec3<f32>(base_world_norm.x, base_world_norm.z, -base_world_norm.y);
    } else if (up_axis == 3u) {
        world_pos = vec3<f32>(base_world_pos.x, -base_world_pos.y, -base_world_pos.z);
        world_norm = vec3<f32>(base_world_norm.x, -base_world_norm.y, -base_world_norm.z);
    }

    out.clip_position = scene.view_proj * vec4<f32>(world_pos, 1.0);
    out.world_normal = world_norm;
    out.tex_coords = model.tex_coords;
    return out;
}

@group(1) @binding(0)
var t_diffuse: texture_2d<f32>;
@group(1) @binding(1)
var s_diffuse: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    let mode = u32(scene.params.x);

    // Mode 2: Unlit
    if (mode == 2u) {
        return vec4<f32>(tex_color.rgb, 1.0);
    }

    let norm = normalize(in.world_normal);

    // 3-Point Studio Lighting
    let key_dir = normalize(vec3<f32>(0.5, 0.85, 0.65));
    let fill_dir = normalize(vec3<f32>(-0.6, 0.35, -0.5));
    let back_dir = normalize(vec3<f32>(0.0, -0.8, -0.6));

    let key_diff = abs(dot(norm, key_dir)) * 0.75;
    let fill_diff = abs(dot(norm, fill_dir)) * 0.35;
    let back_diff = abs(dot(norm, back_dir)) * 0.15;

    // Mode 1: Bright Fill Boost
    var ambient = 0.35;
    if (mode == 1u) {
        ambient = 0.65;
    }

    let lighting = ambient + key_diff + fill_diff + back_diff;
    return vec4<f32>(tex_color.rgb * lighting, 1.0);
}

struct GridVertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct GridVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_grid(model: GridVertexInput) -> GridVertexOutput {
    var out: GridVertexOutput;
    out.clip_position = scene.view_proj * vec4<f32>(model.position, 1.0);
    out.color = model.color;
    return out;
}

@fragment
fn fs_grid(in: GridVertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
        "#;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("TREOVL Main Shader"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });

        let scene_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Scene & Bones Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let scene_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Persistent Scene Uniform Buffer"),
            size: std::mem::size_of::<SceneUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bones_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Persistent Bones Palette Buffer"),
            size: std::mem::size_of::<BonesUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let scene_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Persistent Scene & Bones Bind Group"),
            layout: &scene_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: scene_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: bones_buffer.as_entire_binding(),
                },
            ],
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
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x3,
                        1 => Float32x3,
                        2 => Float32x2,
                        3 => Uint32x4,
                        4 => Float32x4,
                    ],
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

        Ok(Self {
            device,
            queue,
            mesh_pipeline,
            grid_pipeline,
            scene_buffer,
            bones_buffer,
            scene_bind_group,
            texture_bind_group_layout,
            default_sampler,
            default_texture_bind_group,
            target_cache: None,
            last_frame_time: Instant::now(),
            last_pixel_buffer: None,
            grid_buffer: None,
            submesh_buffers: Vec::new(),
            texture_cache: HashMap::new(),
            last_render_key: None,
        })
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
        let buffer_size = (padded_bytes_per_row * height) as u64;

        let create_staging = |idx: usize| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("Persistent Staging Buffer [{}]", idx)),
                size: buffer_size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };

        let staging_buffers = [create_staging(0), create_staging(1)];

        self.target_cache = Some(PersistentRenderTarget {
            width,
            height,
            render_texture,
            render_view,
            _depth_texture: depth_texture,
            depth_view,
            staging_buffers,
            current_staging_idx: 0,
            padded_bytes_per_row,
            unpadded_bytes_per_row,
        });
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

        self.ensure_render_targets(width, height);

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
            &self.scene_buffer,
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
            self.queue
                .write_buffer(&self.bones_buffer, 0, bytemuck::cast_slice(&[bones_data]));
        }

        let floor_y = if min.y.is_finite() { min.y } else { 0.0 };
        let model_radius = if min.x.is_finite() {
            (max - min).length() * 0.5
        } else {
            2.0
        };

        let mut all_lines = Vec::new();
        if show_grid {
            all_lines = build_gpu_grid_vertices(center, floor_y, model_radius);
        }
        all_lines.extend_from_slice(debug_lines);

        let line_count = all_lines.len();
        if line_count > 0 {
            let need_new = match &self.grid_buffer {
                Some((_, cap)) => *cap < line_count,
                None => true,
            };
            if need_new {
                let new_cap = line_count.next_power_of_two().max(512);
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

        // Initialize draw_calls vector
        let mut draw_calls = Vec::with_capacity(submeshes.len());

        let transform_pos_bounds = |p: Vector3| -> [f32; 3] {
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
                        layout: &self.texture_bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&self.default_sampler),
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
                self.default_texture_bind_group.clone()
            };

            draw_calls.push(PreparedDrawCall {
                submesh_idx: sm_idx,
                index_count: sm.indices.len() as u32,
                wire_count,
                bind_group,
            });
        }

        let targets = self
            .target_cache
            .as_mut()
            .context("Render targets missing")?;
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

            if line_count > 0
                && let Some((ref g_buf, _)) = self.grid_buffer
            {
                render_pass.set_pipeline(&self.grid_pipeline);
                render_pass.set_bind_group(0, &self.scene_bind_group, &[]);
                render_pass.set_vertex_buffer(0, g_buf.slice(..));
                render_pass.draw(0..line_count as u32, 0..1);
            }

            if !show_wire {
                render_pass.set_pipeline(&self.mesh_pipeline);
                render_pass.set_bind_group(0, &self.scene_bind_group, &[]);

                for dc in &draw_calls {
                    let buf = &self.submesh_buffers[dc.submesh_idx];
                    render_pass.set_bind_group(1, &dc.bind_group, &[]);
                    render_pass.set_vertex_buffer(0, buf.vertex_buffer.slice(..));
                    render_pass
                        .set_index_buffer(buf.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.draw_indexed(0..dc.index_count, 0, 0..1);
                }
            } else {
                render_pass.set_pipeline(&self.grid_pipeline);
                render_pass.set_bind_group(0, &self.scene_bind_group, &[]);

                for dc in &draw_calls {
                    let buf = &self.submesh_buffers[dc.submesh_idx];
                    if let Some(ref w_buf) = buf.wire_buffer {
                        render_pass.set_vertex_buffer(0, w_buf.slice(..));
                        render_pass.draw(0..dc.wire_count, 0..1);
                    }
                }
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

        render_axis_gizmo(dst, width, height, cam);

        self.last_render_key = Some(current_key);
        self.last_pixel_buffer = Some(pixel_buffer.clone());
        Ok(pixel_buffer)
    }
}
