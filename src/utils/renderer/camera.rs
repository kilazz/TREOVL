use crate::engine::math::Vector3;

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
