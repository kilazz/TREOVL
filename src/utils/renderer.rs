use crate::engine::math::Vector3;
use slint::{Rgba8Pixel, SharedPixelBuffer};

#[derive(Debug, Clone, Copy)]
pub struct ViewportCamera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vector3,
}

impl Default for ViewportCamera {
    fn default() -> Self {
        Self {
            yaw: 0.785,
            pitch: 0.45,
            distance: 3.5,
            target: Vector3::default(),
        }
    }
}

pub fn render_mesh_preview(
    positions: &[Vector3],
    indices: &[u32],
    normals: &[Vector3],
    width: u32,
    height: u32,
    cam: &ViewportCamera,
) -> SharedPixelBuffer<Rgba8Pixel> {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
    if positions.is_empty() || indices.is_empty() {
        return buf;
    }

    // Compute bounding box to normalize model scale and center camera
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

    for p in positions {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        min.z = min.z.min(p.z);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
        max.z = max.z.max(p.z);
    }

    let center = Vector3 {
        x: (min.x + max.x) * 0.5,
        y: (min.y + max.y) * 0.5,
        z: (min.z + max.z) * 0.5,
    };

    let extent = (max.x - min.x)
        .max(max.y - min.y)
        .max(max.z - min.z)
        .max(0.01);
    let scale = 2.0 / extent;

    // View matrix
    let (sin_y, cos_y) = (cam.yaw.sin(), cam.yaw.cos());
    let (sin_p, cos_p) = (cam.pitch.sin(), cam.pitch.cos());

    let fov = 1.8f32;
    let aspect = width as f32 / height as f32;

    let mut z_buffer = vec![f32::INFINITY; (width * height) as usize];
    let pixels = buf.make_mut_bytes();

    // Fill dark studio backdrop
    for y in 0..height {
        let t = y as f32 / height as f32;
        let c = (24.0 + t * 16.0) as u8;
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            pixels[idx] = c;
            pixels[idx + 1] = (c as f32 * 1.05) as u8;
            pixels[idx + 2] = (c as f32 * 1.15) as u8;
            pixels[idx + 3] = 255;
        }
    }

    // Directional light direction
    let light = Vector3 {
        x: 0.577,
        y: 0.577,
        z: 0.577,
    };

    // Transform and rasterize triangles
    for tri in indices.as_chunks::<3>().0 {
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;

        if i0 >= positions.len() || i1 >= positions.len() || i2 >= positions.len() {
            continue;
        }

        let mut pts = [positions[i0], positions[i1], positions[i2]];
        let norms = [
            normals.get(i0).copied().unwrap_or(Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            }),
            normals.get(i1).copied().unwrap_or(Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            }),
            normals.get(i2).copied().unwrap_or(Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            }),
        ];

        let mut screen_pts = [[0.0f32; 3]; 3];
        let mut culled = false;

        for (k, pt) in pts.iter_mut().enumerate() {
            // Translate to center & scale
            let x = (pt.x - center.x) * scale;
            let y = (pt.y - center.y) * scale;
            let z = (pt.z - center.z) * scale;

            // Rotate Yaw (around Y)
            let x1 = x * cos_y + z * sin_y;
            let z1 = -x * sin_y + z * cos_y;

            // Rotate Pitch (around X)
            let y2 = y * cos_p - z1 * sin_p;
            let z2 = y * sin_p + z1 * cos_p + cam.distance;

            if z2 <= 0.1 {
                culled = true;
                break;
            }

            // Project
            let px = (x1 * fov / (z2 * aspect) + 1.0) * 0.5 * width as f32;
            let py = (-y2 * fov / z2 + 1.0) * 0.5 * height as f32;

            screen_pts[k] = [px, py, z2];
        }

        if culled {
            continue;
        }

        // Back-face culling
        let e1x = screen_pts[1][0] - screen_pts[0][0];
        let e1y = screen_pts[1][1] - screen_pts[0][1];
        let e2x = screen_pts[2][0] - screen_pts[0][0];
        let e2y = screen_pts[2][1] - screen_pts[0][1];

        let cross = e1x * e2y - e1y * e2x;
        if cross <= 0.0 {
            continue;
        }

        // Diffuse shading
        let avg_norm = Vector3 {
            x: (norms[0].x + norms[1].x + norms[2].x) / 3.0,
            y: (norms[0].y + norms[1].y + norms[2].y) / 3.0,
            z: (norms[0].z + norms[1].z + norms[2].z) / 3.0,
        };

        // Rotate normal with camera
        let nx1 = avg_norm.x * cos_y + avg_norm.z * sin_y;
        let nz1 = -avg_norm.x * sin_y + avg_norm.z * cos_y;
        let ny2 = avg_norm.y * cos_p - nz1 * sin_p;
        let nz2 = avg_norm.y * sin_p + nz1 * cos_p;

        let dot = (nx1 * light.x + ny2 * light.y + nz2 * light.z).max(0.0);
        let shade = (0.2 + 0.8 * dot).min(1.0);

        let r = (160.0 * shade) as u8;
        let g = (175.0 * shade) as u8;
        let b = (195.0 * shade) as u8;

        // Bounding box of triangle
        let min_x = (screen_pts[0][0]
            .min(screen_pts[1][0])
            .min(screen_pts[2][0])
            .floor() as i32)
            .max(0);
        let max_x = (screen_pts[0][0]
            .max(screen_pts[1][0])
            .max(screen_pts[2][0])
            .ceil() as i32)
            .min(width as i32 - 1);
        let min_y = (screen_pts[0][1]
            .min(screen_pts[1][1])
            .min(screen_pts[2][1])
            .floor() as i32)
            .max(0);
        let max_y = (screen_pts[0][1]
            .max(screen_pts[1][1])
            .max(screen_pts[2][1])
            .ceil() as i32)
            .min(height as i32 - 1);

        for py in min_y..=max_y {
            for px in min_x..=max_x {
                let fx = px as f32 + 0.5;
                let fy = py as f32 + 0.5;

                // Barycentric coordinates
                let w0 = ((screen_pts[1][1] - screen_pts[2][1]) * (fx - screen_pts[2][0])
                    + (screen_pts[2][0] - screen_pts[1][0]) * (fy - screen_pts[2][1]))
                    / cross;
                let w1 = ((screen_pts[2][1] - screen_pts[0][1]) * (fx - screen_pts[2][0])
                    + (screen_pts[0][0] - screen_pts[2][0]) * (fy - screen_pts[2][1]))
                    / cross;
                let w2 = 1.0 - w0 - w1;

                if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                    let depth =
                        w0 * screen_pts[0][2] + w1 * screen_pts[1][2] + w2 * screen_pts[2][2];
                    let idx = (py as usize) * (width as usize) + (px as usize);

                    if depth < z_buffer[idx] {
                        z_buffer[idx] = depth;
                        let p_idx = idx * 4;
                        pixels[p_idx] = r;
                        pixels[p_idx + 1] = g;
                        pixels[p_idx + 2] = b;
                        pixels[p_idx + 3] = 255;
                    }
                }
            }
        }
    }

    buf
}
