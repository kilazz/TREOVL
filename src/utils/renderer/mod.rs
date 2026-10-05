pub mod camera;
pub mod gizmo;
pub mod grid;
pub mod texture;

pub use camera::ViewportCamera;
pub use texture::TextureData;

use crate::engine::math::{Vector2, Vector3};
use slint::{Rgba8Pixel, SharedPixelBuffer};

pub fn render_mesh_preview(
    positions: &[Vector3],
    indices: &[u32],
    normals: &[Vector3],
    uvs: &[Vector2],
    texture: Option<&TextureData>,
    size: (u32, u32),
    cam: &ViewportCamera,
) -> SharedPixelBuffer<Rgba8Pixel> {
    let (width, height) = size;
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
    if positions.is_empty() || indices.is_empty() {
        return buf;
    }

    // 1. Calculate AABB bounding box
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
    let floor_y = (min.y - center.y) * scale;

    let (sin_y, cos_y) = (cam.yaw.sin(), cam.yaw.cos());
    let (sin_p, cos_p) = (cam.pitch.sin(), cam.pitch.cos());

    let fov = 1.8f32;
    let aspect = width as f32 / height as f32;

    let mut z_buffer = vec![f32::INFINITY; (width * height) as usize];
    let pixels = buf.make_mut_bytes();

    // Background gradient fill
    for y in 0..height {
        let t = y as f32 / height as f32;
        let c = (20.0 + t * 14.0) as u8;
        let row_start = (y * width * 4) as usize;
        for x in 0..width {
            let idx = row_start + (x * 4) as usize;
            pixels[idx] = c;
            pixels[idx + 1] = (c as f32 * 1.05) as u8;
            pixels[idx + 2] = (c as f32 * 1.15) as u8;
            pixels[idx + 3] = 255;
        }
    }

    // 2. Render 3D ground plane grid beneath model base
    grid::render_ground_grid(pixels, &mut z_buffer, width, height, floor_y, cam);

    let light = Vector3 {
        x: 0.577,
        y: 0.577,
        z: 0.577,
    };

    let tri_chunks = indices.as_chunks::<3>().0;
    let total_tris = tri_chunks.len();

    let lod_step = if total_tris > 35_000 {
        3
    } else if total_tris > 18_000 {
        2
    } else {
        1
    };

    // 3. Rasterize model triangles with perspective-correct 1/Z depth & UV mapping
    for tri_idx in (0..total_tris).step_by(lod_step) {
        let tri = tri_chunks[tri_idx];
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

        let uv0 = uvs.get(i0).copied().unwrap_or_default();
        let uv1 = uvs.get(i1).copied().unwrap_or_default();
        let uv2 = uvs.get(i2).copied().unwrap_or_default();

        let mut screen_pts = [[0.0f32; 3]; 3];
        let mut culled = false;

        for (k, pt) in pts.iter_mut().enumerate() {
            let x = (pt.x - center.x) * scale;
            let y = (pt.y - center.y) * scale;
            let z = (pt.z - center.z) * scale;

            let x1 = x * cos_y + z * sin_y;
            let z1 = -x * sin_y + z * cos_y;

            let y2 = y * cos_p - z1 * sin_p;
            let z2 = y * sin_p + z1 * cos_p + cam.distance;

            if z2 <= 0.1 {
                culled = true;
                break;
            }

            let px = (x1 * fov / (z2 * aspect) + 1.0) * 0.5 * width as f32;
            let py = (-y2 * fov / z2 + 1.0) * 0.5 * height as f32;

            screen_pts[k] = [px, py, z2];
        }

        if culled {
            continue;
        }

        let e1x = screen_pts[1][0] - screen_pts[0][0];
        let e1y = screen_pts[1][1] - screen_pts[0][1];
        let e2x = screen_pts[2][0] - screen_pts[0][0];
        let e2y = screen_pts[2][1] - screen_pts[0][1];

        let cross = e1x * e2y - e1y * e2x;
        if cross.abs() < 1e-5 {
            continue;
        }

        let is_backface = cross < 0.0;
        let inv_cross = 1.0 / cross.abs();
        let normal_sign = if is_backface { -1.0 } else { 1.0 };

        let avg_norm = Vector3 {
            x: (norms[0].x + norms[1].x + norms[2].x) * 0.333333 * normal_sign,
            y: (norms[0].y + norms[1].y + norms[2].y) * 0.333333 * normal_sign,
            z: (norms[0].z + norms[1].z + norms[2].z) * 0.333333 * normal_sign,
        };

        let nx1 = avg_norm.x * cos_y + avg_norm.z * sin_y;
        let nz1 = -avg_norm.x * sin_y + avg_norm.z * cos_y;
        let ny2 = avg_norm.y * cos_p - nz1 * sin_p;
        let nz2 = avg_norm.y * sin_p + nz1 * cos_p;

        let dot = (nx1 * light.x + ny2 * light.y + nz2 * light.z).max(0.0);
        let backface_tint = if is_backface { 0.75 } else { 1.0 };
        let shade = (0.30 + 0.70 * dot) * backface_tint;

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

        let y1_sub_y2 = (screen_pts[1][1] - screen_pts[2][1]) * inv_cross;
        let x2_sub_x1 = (screen_pts[2][0] - screen_pts[1][0]) * inv_cross;
        let y2_sub_y0 = (screen_pts[2][1] - screen_pts[0][1]) * inv_cross;
        let x0_sub_x2 = (screen_pts[0][0] - screen_pts[2][0]) * inv_cross;

        let p2_x = screen_pts[2][0];
        let p2_y = screen_pts[2][1];

        let inv_z0 = 1.0 / screen_pts[0][2];
        let inv_z1 = 1.0 / screen_pts[1][2];
        let inv_z2 = 1.0 / screen_pts[2][2];

        let u0_z = uv0.x * inv_z0;
        let v0_z = uv0.y * inv_z0;
        let u1_z = uv1.x * inv_z1;
        let v1_z = uv1.y * inv_z1;
        let u2_z = uv2.x * inv_z2;
        let v2_z = uv2.y * inv_z2;

        for py in min_y..=max_y {
            let fy = py as f32 + 0.5;
            let dy = fy - p2_y;
            let row_idx = (py as usize) * (width as usize);

            for px in min_x..=max_x {
                let fx = px as f32 + 0.5;
                let dx = fx - p2_x;

                let mut w0 = y1_sub_y2 * dx + x2_sub_x1 * dy;
                let mut w1 = y2_sub_y0 * dx + x0_sub_x2 * dy;
                if is_backface {
                    w0 = -w0;
                    w1 = -w1;
                }
                let w2 = 1.0 - w0 - w1;

                if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                    let interp_inv_z = w0 * inv_z0 + w1 * inv_z1 + w2 * inv_z2;
                    if interp_inv_z <= 0.0 {
                        continue;
                    }
                    let pixel_depth = 1.0 / interp_inv_z;

                    let idx = row_idx + (px as usize);
                    if pixel_depth < z_buffer[idx] {
                        z_buffer[idx] = pixel_depth;

                        let base_color = if let Some(tex) = texture {
                            let u = (w0 * u0_z + w1 * u1_z + w2 * u2_z) * pixel_depth;
                            let v = (w0 * v0_z + w1 * v1_z + w2 * v2_z) * pixel_depth;
                            tex.sample_bilinear(u, v)
                        } else {
                            [170, 180, 200]
                        };

                        let p_idx = idx * 4;
                        pixels[p_idx] = (base_color[0] as f32 * shade).min(255.0) as u8;
                        pixels[p_idx + 1] = (base_color[1] as f32 * shade).min(255.0) as u8;
                        pixels[p_idx + 2] = (base_color[2] as f32 * shade).min(255.0) as u8;
                        pixels[p_idx + 3] = 255;
                    }
                }
            }
        }
    }

    // 4. Render interactive 3D XYZ orientation gizmo in TOP-RIGHT corner
    gizmo::render_axis_gizmo(pixels, width, height, cam);

    buf
}
