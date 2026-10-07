#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VertexSemantic {
    Position,
    Normal,
    TexCoord,
    Color,
    TangentQuat,
    BlendWeights,
    BlendIndices,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub struct VertexAttribute {
    pub semantic: VertexSemantic,
    pub byte_size: usize,
    pub raw_descriptor: u32,
}

impl VertexAttribute {
    pub fn from_descriptor(desc: u32) -> Self {
        let semantic_byte = ((desc >> 16) & 0xFF) as u8;
        let flags = ((desc >> 24) & 0xFF) as u8;

        let semantic = match semantic_byte {
            0x01 => VertexSemantic::Position,
            0x04 => VertexSemantic::Normal,
            0x05 => VertexSemantic::TexCoord,
            0x06 => VertexSemantic::Color,
            0x09 => VertexSemantic::TangentQuat,
            0x0A => VertexSemantic::BlendWeights,
            0x0B => VertexSemantic::BlendIndices,
            _ => VertexSemantic::Unknown,
        };

        let byte_size = match flags {
            1 => 8,     // 2x f32 (UVs)
            2 => 12,    // 3x f32 (Vec3 Position / Normal)
            3 => 16,    // 4x f32 (Vec4 TangentQuat)
            4 | 7 => 1, // 1x u8 (Packed bone index or normalized weight)
            15 => 4,    // 4x u8 packed
            _ => 12,
        };

        Self {
            semantic,
            byte_size,
            raw_descriptor: desc,
        }
    }
}
