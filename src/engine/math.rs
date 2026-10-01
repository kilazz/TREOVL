use binrw::{BinRead, BinWrite};
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, BinRead, BinWrite)]
#[brw(little)]
pub struct Vector2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, BinRead, BinWrite)]
#[brw(little)]
pub struct Vector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, BinRead, BinWrite)]
#[brw(little)]
pub struct Vector4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, BinRead, BinWrite)]
#[brw(little)]
pub struct BoneRotation {
    #[br(map = |raw: i16| raw as f32 * (std::f32::consts::PI / 32768.0))]
    #[bw(map = |val: &f32| (*val / (std::f32::consts::PI / 32768.0)) as i16)]
    pub pitch: f32,

    #[br(map = |raw: i16| raw as f32 * (std::f32::consts::PI / 32768.0))]
    #[bw(map = |val: &f32| (*val / (std::f32::consts::PI / 32768.0)) as i16)]
    pub yaw: f32,

    #[br(map = |raw: i16| raw as f32 * (std::f32::consts::PI / 32768.0))]
    #[bw(map = |val: &f32| (*val / (std::f32::consts::PI / 32768.0)) as i16)]
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

    pub fn read(cur: &mut Cursor<&[u8]>) -> Result<Self, std::io::Error> {
        let raw_x = cur.read_i16::<LittleEndian>()?;
        let raw_y = cur.read_i16::<LittleEndian>()?;
        let raw_z = cur.read_i16::<LittleEndian>()?;
        Ok(Self::from_raw_i16(raw_x, raw_y, raw_z))
    }

    pub fn to_quaternion(self) -> Vector4 {
        let (cp, sp) = ((self.pitch * 0.5).cos(), (self.pitch * 0.5).sin());
        let (cy, sy) = ((self.yaw * 0.5).cos(), (self.yaw * 0.5).sin());
        let (cr, sr) = ((self.roll * 0.5).cos(), (self.roll * 0.5).sin());

        Vector4 {
            x: sp * cy * cr - cp * sy * sr,
            y: cp * sy * cr + sp * cy * sr,
            z: cp * cy * sr - sp * sy * cr,
            w: cp * cy * cr + sp * sy * sr,
        }
    }
}
