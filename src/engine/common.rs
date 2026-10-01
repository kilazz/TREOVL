use anyhow::{Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;

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

#[derive(Debug, Clone, Copy)]
pub struct ContainerTableEntry {
    pub id: u32,
    pub offset: usize,
    pub is_large: bool,
}

#[derive(Debug)]
pub struct ParsedContainerTable {
    pub has_magic: bool,
    pub data_start: usize,
    pub entries: Vec<ContainerTableEntry>,
}

/// Unified parser for Triumph Studios container offset tables (DRY implementation).
pub fn parse_raw_container_table(
    data: &[u8],
    mut pos: usize,
    allow_magic: bool,
) -> Result<ParsedContainerTable> {
    let mut has_magic = false;
    if allow_magic && data.len() >= pos + 3 && &data[pos..pos + 3] == magic::CONTAINER_MAGIC {
        has_magic = true;
        pos += 3;
    }

    if pos >= data.len() {
        bail!("Container header truncated");
    }

    let control_byte = data[pos];
    let has_large = (control_byte & 0x80) != 0;
    let small_count = (control_byte & 0x7F) as usize;
    pos += 1;

    let mut large_count = 0;
    if has_large {
        if pos + 4 > data.len() {
            bail!("Corrupted container header: truncated large count");
        }
        let mut cur = Cursor::new(&data[pos..pos + 4]);
        large_count = cur.read_u32::<LittleEndian>()? as usize;
        pos += 4;
    }

    let total_entries = small_count + large_count;
    if total_entries == 0 || total_entries > 4096 {
        bail!("Invalid entry count in container table: {}", total_entries);
    }

    let table_size = (small_count * 2) + (large_count * 8);
    let data_start = pos + table_size;
    if data_start > data.len() {
        bail!("Table offsets exceed data bounds");
    }

    let mut cur = Cursor::new(&data[pos..data_start]);
    let mut entries = Vec::with_capacity(total_entries);

    for _ in 0..small_count {
        let id = cur.read_u8()? as u32;
        let offset = cur.read_u8()? as usize;
        entries.push(ContainerTableEntry {
            id,
            offset,
            is_large: false,
        });
    }

    for _ in 0..large_count {
        let id = cur.read_u32::<LittleEndian>()?;
        let offset = cur.read_u32::<LittleEndian>()? as usize;
        entries.push(ContainerTableEntry {
            id,
            offset,
            is_large: true,
        });
    }

    entries.sort_by_key(|e| e.offset);

    if entries.is_empty() || entries[0].offset != 0 {
        bail!("First table offset must be 0");
    }

    Ok(ParsedContainerTable {
        has_magic,
        data_start,
        entries,
    })
}
