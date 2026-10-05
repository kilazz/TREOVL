use glam::Vec3;

/// 3D Ground Grid vertex format aligned for WGPU buffer layouts
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GridVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

/// Generates 3D ground grid line vertices positioned at the base of the model (`floor_y`),
/// dynamically scaled to the bounding dimensions of the loaded geometry.
pub fn build_gpu_grid_vertices(center: Vec3, floor_y: f32, model_radius: f32) -> Vec<GridVertex> {
    let grid_radius = (model_radius * 1.6).clamp(2.0, 50.0);
    let step = (grid_radius / 10.0).clamp(0.2, 5.0);
    let count = (grid_radius / step).ceil() as i32;

    let mut lines = Vec::with_capacity(((count * 2 + 1) * 4) as usize);
    let main_color = [0.28, 0.35, 0.48, 1.0];
    let sub_color = [0.18, 0.20, 0.25, 0.75];

    let min_x = center.x - grid_radius;
    let max_x = center.x + grid_radius;
    let min_z = center.z - grid_radius;
    let max_z = center.z + grid_radius;

    for i in -count..=count {
        let offset = i as f32 * step;
        let is_center = i == 0;
        let color = if is_center { main_color } else { sub_color };

        // Lines parallel to X-axis
        let z = center.z + offset;
        lines.push(GridVertex {
            position: [min_x, floor_y, z],
            color,
        });
        lines.push(GridVertex {
            position: [max_x, floor_y, z],
            color,
        });

        // Lines parallel to Z-axis
        let x = center.x + offset;
        lines.push(GridVertex {
            position: [x, floor_y, min_z],
            color,
        });
        lines.push(GridVertex {
            position: [x, floor_y, max_z],
            color,
        });
    }

    lines
}
