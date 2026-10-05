use crate::engine::math::Vector3;

#[derive(Debug, Clone, Copy)]
pub struct ViewportCamera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vector3,
    pub fov_degrees: f32,
    pub lighting_mode: u32, // 0 = Studio Lit, 1 = Bright Fill, 2 = Unlit
    pub up_axis: u32,       // 0 = Y-Up (Raw), 1 = Z-Up to Y-Up (Triumph Engine / 3ds Max)
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
            up_axis: 1, // Default to 1 (Z-Up to Y-Up conversion)
        }
    }
}

impl ViewportCamera {
    pub fn zoom(&mut self, factor: f32) {
        // Multiplicative smooth zooming clamped to safe boundaries
        self.distance = (self.distance * factor).clamp(0.1, 500.0);
    }
}
