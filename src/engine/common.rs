#[allow(dead_code)]
pub mod magic {
    pub const TEX_3D: &[u8; 4] = b"\x3D\x00\x41\x00";
    pub const TEX_CUBEMAP: &[u8; 4] = b"\x99\x00\x41\x00";
    pub const TEX_INTERFACE: &[u8; 4] = b"\x98\x00\x41\x00";
    pub const TEX_MIPMAP: &[u8; 4] = b"\x24\x00\x41\x00";
    pub const AUDIO_WAV: &[u8; 4] = b"\x00\x00\xA1\x00";
    pub const MESH: &[u8; 4] = b"\x35\x00\x41\x00";
    pub const ANIM_CLIP: &[u8; 4] = b"\x05\x00\x41\x00";
    pub const ANIM_TRACK: &[u8; 4] = b"\x07\x00\x41\x00";
    pub const OBJECT: &[u8; 4] = b"\x4B\x00\x41\x00";
    pub const LUA: &[u8; 4] = b"\x1bLua";
    pub const EVENT: &[u8; 4] = b"\xB0\x00\x00\x04";
    pub const CONTAINER_MAGIC: &[u8; 3] = b"\x01\x01\x00";
}

#[allow(dead_code)]
pub mod chunk_id {
    pub const SUB_CONTAINER: u32 = 1;
    pub const INDEX_DATA: u32 = 10;
    pub const VERTEX_DATA: u32 = 11;
    pub const TAG_STRING: u32 = 20;
    pub const NAME_STRING: u32 = 21;
    pub const DATA_BLOB: u32 = 22;
    pub const FORMAT: u32 = 23;
    pub const WIDTH: u32 = 30;
    pub const HEIGHT: u32 = 31;
    pub const OBJECT_BONES: u32 = 33;
}
