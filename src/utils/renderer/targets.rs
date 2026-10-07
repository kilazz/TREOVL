use std::collections::HashMap;

pub const STAGING_BUFFER_COUNT: usize = 2;

pub struct PersistentRenderTarget {
    pub _width: u32,
    pub _height: u32,
    pub render_texture: wgpu::Texture,
    pub render_view: wgpu::TextureView,
    pub _depth_texture: wgpu::Texture,
    pub depth_view: wgpu::TextureView,
    pub staging_buffers: [wgpu::Buffer; STAGING_BUFFER_COUNT],
    pub current_staging_idx: usize,
    pub padded_bytes_per_row: u32,
    pub unpadded_bytes_per_row: u32,
}

impl PersistentRenderTarget {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let render_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("Persistent Render Target {}x{}", width, height)),
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

        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("Persistent Depth Target {}x{}", width, height)),
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
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!(
                    "Persistent Staging Buffer {}x{} [{}]",
                    width, height, idx
                )),
                size: buffer_size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };

        let staging_buffers = [create_staging(0), create_staging(1)];

        Self {
            _width: width,
            _height: height,
            render_texture,
            render_view,
            _depth_texture: depth_texture,
            depth_view,
            staging_buffers,
            current_staging_idx: 0,
            padded_bytes_per_row,
            unpadded_bytes_per_row,
        }
    }
}

#[derive(Default)]
pub struct RenderTargetManager {
    targets: HashMap<(u32, u32), PersistentRenderTarget>,
}

impl RenderTargetManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_create(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> &mut PersistentRenderTarget {
        self.targets
            .entry((width, height))
            .or_insert_with(|| PersistentRenderTarget::new(device, width, height))
    }
}
