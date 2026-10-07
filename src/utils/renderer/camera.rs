use crate::engine::math::Vector3;
use glam::{Mat4, Vec3};

#[derive(Debug, Clone, Copy)]
pub struct ViewportCamera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vector3,
    pub fov_degrees: f32,
    pub lighting_mode: u32, // 0 = Studio Lit, 1 = Bright Fill, 2 = Unlit
    pub up_axis: u32, // 0 = Ground / Aligned, 1 = Pitch Up (+90°), 2 = Pitch Down (-90°), 3 = Invert (180°)
}

impl Default for ViewportCamera {
    fn default() -> Self {
        Self {
            yaw: 0.785,
            pitch: 0.35,
            distance: 3.5,
            target: Vector3::default(),
            fov_degrees: 45.0,
            lighting_mode: 0,
            up_axis: 0,
        }
    }
}

impl ViewportCamera {
    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(0.1, 500.0);
    }
}

pub fn perspective_rh_zo(fov_y_radians: f32, aspect_ratio: f32, z_near: f32, z_far: f32) -> Mat4 {
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

pub fn look_at_rh(eye: Vec3, center: Vec3, up: Vec3) -> Mat4 {
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
