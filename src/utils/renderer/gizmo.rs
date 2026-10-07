use super::camera::ViewportCamera;
use super::grid::GridVertex;
use glam::Vec3;

/// Builds GPU line and triangle vertices for the 3D coordinate axis gizmo (X=Red, Y=Green, Z=Blue)
/// and its circular background disc directly in NDC screen space for WGPU hardware rendering.
pub fn build_gpu_gizmo_vertices(cam: &ViewportCamera) -> Vec<GridVertex> {
    let mut vertices = Vec::with_capacity(160);

    // Center anchor in NDC (top-right corner of the viewport)
    let cx = 0.82f32;
    let cy = 0.82f32;
    let radius = 0.12f32;

    // 1. Translucent backing circular disc (32 triangles)
    let disc_color = [0.07f32, 0.08, 0.10, 0.86];
    let segments = 32;
    for i in 0..segments {
        let a0 = std::f32::consts::TAU * (i as f32) / (segments as f32);
        let a1 = std::f32::consts::TAU * ((i + 1) as f32) / (segments as f32);

        vertices.push(GridVertex {
            position: [cx, cy, 0.0],
            color: disc_color,
        });
        vertices.push(GridVertex {
            position: [cx + radius * a0.cos(), cy + radius * a0.sin(), 0.0],
            color: disc_color,
        });
        vertices.push(GridVertex {
            position: [cx + radius * a1.cos(), cy + radius * a1.sin(), 0.0],
            color: disc_color,
        });
    }

    // Camera view basis calculation
    let eye = Vec3::new(
        cam.yaw.sin() * cam.pitch.cos() * cam.distance,
        cam.pitch.sin() * cam.distance,
        cam.yaw.cos() * cam.pitch.cos() * cam.distance,
    );
    let center = Vec3::ZERO;
    let f = (center - eye).normalize();

    let up_ref = if f.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let s = f.cross(up_ref).normalize_or(Vec3::X);
    let u = s.cross(f).normalize();

    struct AxisDesc {
        world_dir: Vec3,
        color: [f32; 4],
    }

    // Standard 3D Viewport World Axes:
    // Red (X) = Right [1, 0, 0]
    // Green (Y) = Up [0, 1, 0] (Points UP towards sky)
    // Blue (Z) = Forward/Depth [0, 0, 1] (Lies on grid floor)
    let mut axes = [
        AxisDesc {
            world_dir: Vec3::X,
            color: [0.92, 0.28, 0.28, 1.0], // Red: X (Right)
        },
        AxisDesc {
            world_dir: Vec3::Y,
            color: [0.28, 0.85, 0.35, 1.0], // Green: Y (Up)
        },
        AxisDesc {
            world_dir: Vec3::Z,
            color: [0.32, 0.55, 0.95, 1.0], // Blue: Z (Depth/Grid)
        },
    ];

    // Sort axes by depth so the ones closer to the camera are drawn on top
    axes.sort_by(|a, b| {
        let da = a.world_dir.dot(-f);
        let db = b.world_dir.dot(-f);
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    });

    let shaft_len = 0.075f32;
    let half_width = 0.0035f32;
    let arrow_len = 0.020f32;
    let arrow_hw = 0.009f32;

    for axis in axes {
        let depth = axis.world_dir.dot(-f);
        let mut color = axis.color;
        // Dim arrow if pointing away from camera
        if depth < 0.0 {
            color[3] = 0.45;
        }

        let sx = axis.world_dir.dot(s);
        let sy = axis.world_dir.dot(u);
        let dir = glam::Vec2::new(sx, sy);
        let len = dir.length();

        if len > 0.005 {
            let nd = dir / len;
            let p = glam::Vec2::new(-nd.y, nd.x) * half_width;
            let ap = glam::Vec2::new(-nd.y, nd.x) * arrow_hw;

            let c = glam::Vec2::new(cx, cy);
            let s_end = c + nd * (shaft_len * len.clamp(0.2, 1.0));
            let tip = s_end + nd * arrow_len;

            // Shaft rectangle (2 triangles)
            let v0 = c - p;
            let v1 = c + p;
            let v2 = s_end + p;
            let v3 = s_end - p;

            vertices.push(GridVertex {
                position: [v0.x, v0.y, 0.0],
                color,
            });
            vertices.push(GridVertex {
                position: [v1.x, v1.y, 0.0],
                color,
            });
            vertices.push(GridVertex {
                position: [v2.x, v2.y, 0.0],
                color,
            });

            vertices.push(GridVertex {
                position: [v0.x, v0.y, 0.0],
                color,
            });
            vertices.push(GridVertex {
                position: [v2.x, v2.y, 0.0],
                color,
            });
            vertices.push(GridVertex {
                position: [v3.x, v3.y, 0.0],
                color,
            });

            // Arrowhead tip (1 triangle)
            let a0 = s_end - ap;
            let a1 = s_end + ap;
            let a2 = tip;

            vertices.push(GridVertex {
                position: [a0.x, a0.y, 0.0],
                color,
            });
            vertices.push(GridVertex {
                position: [a1.x, a1.y, 0.0],
                color,
            });
            vertices.push(GridVertex {
                position: [a2.x, a2.y, 0.0],
                color,
            });
        }
    }

    vertices
}
