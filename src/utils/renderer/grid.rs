use super::camera::ViewportCamera;

pub fn render_ground_grid(
    pixels: &mut [u8],
    z_buffer: &mut [f32],
    width: u32,
    height: u32,
    floor_y: f32,
    cam: &ViewportCamera,
) {
    let (sin_y, cos_y) = (cam.yaw.sin(), cam.yaw.cos());
    let (sin_p, cos_p) = (cam.pitch.sin(), cam.pitch.cos());
    let fov = 1.8f32;
    let aspect = width as f32 / height as f32;
    let distance = cam.distance;

    let grid_size = 1.6f32;
    let step = 0.2f32;
    let grid_lines = ((grid_size * 2.0) / step) as i32;

    let project_point = |x: f32, y: f32, z: f32| -> Option<(f32, f32, f32)> {
        let x1 = x * cos_y + z * sin_y;
        let z1 = -x * sin_y + z * cos_y;
        let y2 = y * cos_p - z1 * sin_p;
        let z2 = y * sin_p + z1 * cos_p + distance;

        if z2 <= 0.15 {
            return None;
        }

        let px = (x1 * fov / (z2 * aspect) + 1.0) * 0.5 * width as f32;
        let py = (-y2 * fov / z2 + 1.0) * 0.5 * height as f32;
        Some((px, py, z2))
    };

    for i in 0..=grid_lines {
        let coord = -grid_size + (i as f32 * step);
        let is_center = coord.abs() < 1e-4;
        let color = if is_center {
            [55, 65, 85, 255]
        } else {
            [36, 40, 48, 255]
        };

        // Grid lines parallel to X axis
        if let (Some(p0), Some(p1)) = (
            project_point(-grid_size, floor_y, coord),
            project_point(grid_size, floor_y, coord),
        ) {
            draw_line_depth(pixels, z_buffer, width, height, p0, p1, color);
        }

        // Grid lines parallel to Z axis
        if let (Some(p0), Some(p1)) = (
            project_point(coord, floor_y, -grid_size),
            project_point(coord, floor_y, grid_size),
        ) {
            draw_line_depth(pixels, z_buffer, width, height, p0, p1, color);
        }
    }
}

fn draw_line_depth(
    pixels: &mut [u8],
    z_buffer: &mut [f32],
    width: u32,
    height: u32,
    p0: (f32, f32, f32),
    p1: (f32, f32, f32),
    color: [u8; 4],
) {
    let dx = p1.0 - p0.0;
    let dy = p1.1 - p0.1;
    let steps = (dx.abs().max(dy.abs()) as usize).clamp(1, 1200);

    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let x = (p0.0 + dx * t).round() as i32;
        let y = (p0.1 + dy * t).round() as i32;
        let z = p0.2 + (p1.2 - p0.2) * t;

        if x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
            let idx = (y as usize) * (width as usize) + (x as usize);
            if z <= z_buffer[idx] {
                z_buffer[idx] = z;
                let p_idx = idx * 4;
                pixels[p_idx] = color[0];
                pixels[p_idx + 1] = color[1];
                pixels[p_idx + 2] = color[2];
                pixels[p_idx + 3] = color[3];
            }
        }
    }
}
