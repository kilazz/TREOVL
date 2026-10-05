use super::camera::ViewportCamera;

/// Renders an interactive 3D coordinate axis gizmo (X=Red, Y=Green, Z=Blue)
/// on a translucent circular backdrop in the top-right corner of the viewport.
pub fn render_axis_gizmo(pixels: &mut [u8], width: u32, height: u32, cam: &ViewportCamera) {
    let (sin_y, cos_y) = (cam.yaw.sin(), cam.yaw.cos());
    let (sin_p, cos_p) = (cam.pitch.sin(), cam.pitch.cos());

    // Top-right corner anchor coordinates
    let gizmo_x = width as i32 - 46;
    let gizmo_y = 46i32;
    let radius = 24.0f32;

    // Translucent circular backing disc
    for dy in -32..=32 {
        for dx in -32..=32 {
            if dx * dx + dy * dy <= 28 * 28 {
                let px = gizmo_x + dx;
                let py = gizmo_y + dy;
                if px >= 0 && px < width as i32 && py >= 0 && py < height as i32 {
                    let idx = ((py as usize) * (width as usize) + (px as usize)) * 4;
                    if idx + 3 < pixels.len() {
                        pixels[idx] = 18;
                        pixels[idx + 1] = 20;
                        pixels[idx + 2] = 24;
                        pixels[idx + 3] = 220;
                    }
                }
            }
        }
    }

    let project_axis = |vx: f32, vy: f32, vz: f32| -> (f32, f32) {
        let x1 = vx * cos_y + vz * sin_y;
        let z1 = -vx * sin_y + vz * cos_y;
        let y2 = vy * cos_p - z1 * sin_p;
        (x1 * radius, -y2 * radius)
    };

    let axes = [
        (project_axis(1.0, 0.0, 0.0), [235, 75, 75, 255]), // X-axis: Red
        (project_axis(0.0, 1.0, 0.0), [75, 215, 85, 255]), // Y-axis: Green
        (project_axis(0.0, 0.0, 1.0), [75, 140, 245, 255]), // Z-axis: Blue
    ];

    for ((dx, dy), color) in axes {
        draw_line_2d(
            pixels,
            width,
            height,
            [gizmo_x as f32, gizmo_y as f32],
            [gizmo_x as f32 + dx, gizmo_y as f32 + dy],
            color,
        );
    }
}

fn draw_line_2d(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    p0: [f32; 2],
    p1: [f32; 2],
    color: [u8; 4],
) {
    let dx = p1[0] - p0[0];
    let dy = p1[1] - p0[1];
    let steps = (dx.abs().max(dy.abs()) as usize).clamp(1, 100);

    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let x = (p0[0] + dx * t).round() as i32;
        let y = (p0[1] + dy * t).round() as i32;

        if x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
            let idx = ((y as usize) * (width as usize) + (x as usize)) * 4;
            if idx + 3 < pixels.len() {
                pixels[idx] = color[0];
                pixels[idx + 1] = color[1];
                pixels[idx + 2] = color[2];
                pixels[idx + 3] = color[3];
            }
        }
    }
}
