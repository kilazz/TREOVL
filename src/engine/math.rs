use glam::{EulerRot, Quat};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Vector2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Vector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Vector4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct BoneRotation {
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
}

impl BoneRotation {
    pub fn from_raw_i16(pitch_raw: i16, yaw_raw: i16, roll_raw: i16) -> Self {
        const SCALE: f32 = std::f32::consts::PI / 32768.0;
        Self {
            pitch: pitch_raw as f32 * SCALE,
            yaw: yaw_raw as f32 * SCALE,
            roll: roll_raw as f32 * SCALE,
        }
    }

    /// Converts Triumph Engine Euler angles into a mathematically normalized Quaternion.
    /// Safely utilizes the ZYX intrinsic rotation order (Z=Roll, Y=Yaw, X=Pitch) matching the engine.
    pub fn to_quaternion(self) -> Vector4 {
        let q = Quat::from_euler(EulerRot::ZYX, self.roll, self.yaw, self.pitch).normalize();
        Vector4 {
            x: q.x,
            y: q.y,
            z: q.z,
            w: q.w,
        }
    }
}
