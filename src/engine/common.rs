use anyhow::{Result, bail};
use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use crc32fast::Hasher;
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Endian {
    #[default]
    Little, // PC (Intel/AMD)
    Big, // Xbox 360 (PowerPC) / PlayStation 3 (Cell)
}

impl Endian {
    pub fn read_u16(self, cur: &mut Cursor<&[u8]>) -> Result<u16, std::io::Error> {
        match self {
            Endian::Little => cur.read_u16::<LittleEndian>(),
            Endian::Big => cur.read_u16::<BigEndian>(),
        }
    }

    pub fn read_u32(self, cur: &mut Cursor<&[u8]>) -> Result<u32, std::io::Error> {
        match self {
            Endian::Little => cur.read_u32::<LittleEndian>(),
            Endian::Big => cur.read_u32::<BigEndian>(),
        }
    }

    pub fn read_f32(self, cur: &mut Cursor<&[u8]>) -> Result<f32, std::io::Error> {
        match self {
            Endian::Little => cur.read_f32::<LittleEndian>(),
            Endian::Big => cur.read_f32::<BigEndian>(),
        }
    }

    pub fn write_u16(self, cur: &mut impl std::io::Write, val: u16) -> Result<(), std::io::Error> {
        match self {
            Endian::Little => byteorder::WriteBytesExt::write_u16::<LittleEndian>(cur, val),
            Endian::Big => byteorder::WriteBytesExt::write_u16::<BigEndian>(cur, val),
        }
    }

    pub fn write_u32(self, cur: &mut impl std::io::Write, val: u32) -> Result<(), std::io::Error> {
        match self {
            Endian::Little => byteorder::WriteBytesExt::write_u32::<LittleEndian>(cur, val),
            Endian::Big => byteorder::WriteBytesExt::write_u32::<BigEndian>(cur, val),
        }
    }

    pub fn u32_to_bytes(self, val: u32) -> [u8; 4] {
        match self {
            Endian::Little => val.to_le_bytes(),
            Endian::Big => val.to_be_bytes(),
        }
    }
}

/// Detects archive endianness by reading the file size field at offset 12 in the PRP header.
pub fn detect_endianness(header_bytes: &[u8]) -> Endian {
    if header_bytes.len() < 16 {
        return Endian::Little;
    }

    let size_le = u32::from_le_bytes(header_bytes[12..16].try_into().unwrap_or_default());
    if size_le > 0x7FFF_FFFF {
        Endian::Big
    } else {
        Endian::Little
    }
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
    pub endian: Endian,
}

pub fn parse_raw_container_table(
    data: &[u8],
    pos: usize,
    allow_magic: bool,
) -> Result<ParsedContainerTable> {
    // We default to Little Endian as 99% of Overlord mods are for PC.
    let mut endian = Endian::Little;

    let mut temp_pos = pos;
    if allow_magic
        && data.len() >= temp_pos + 3
        && &data[temp_pos..temp_pos + 3] == magic::CONTAINER_MAGIC
    {
        temp_pos += 3;
    }

    // Safely deduce chunk endianness if there is a 'large_count' component.
    // This prevents interpreting unrelated memory addresses/offsets as large Endian flags.
    if data.len() > temp_pos + 4 {
        let control_byte = data[temp_pos];
        if (control_byte & 0x80) != 0 {
            // Has large entries
            let l_le = u32::from_le_bytes(
                data[temp_pos + 1..temp_pos + 5]
                    .try_into()
                    .unwrap_or_default(),
            );
            let l_be = u32::from_be_bytes(
                data[temp_pos + 1..temp_pos + 5]
                    .try_into()
                    .unwrap_or_default(),
            );
            // If the element count is completely absurd in Little-Endian but sane (<10,000)
            // in Big-Endian, we confidently mark this chunk as Xbox/PS3 Big-Endian format.
            if l_le > 10_000 && l_be < 10_000 {
                endian = Endian::Big;
            }
        }
    }

    parse_raw_container_table_with_endian(data, pos, allow_magic, endian)
}

pub fn parse_raw_container_table_with_endian(
    data: &[u8],
    mut pos: usize,
    allow_magic: bool,
    endian: Endian,
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
        large_count = endian.read_u32(&mut cur)? as usize;
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
        let id = endian.read_u32(&mut cur)?;
        let offset = endian.read_u32(&mut cur)? as usize;
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
        endian,
    })
}

pub fn extract_slices_from_table<'a>(
    data: &'a [u8],
    table: &ParsedContainerTable,
) -> Vec<(u32, bool, &'a [u8])> {
    let mut elements = Vec::with_capacity(table.entries.len());
    for i in 0..table.entries.len() {
        let entry = &table.entries[i];
        let start = table.data_start + entry.offset;
        let end = if i + 1 < table.entries.len() {
            table.data_start + table.entries[i + 1].offset
        } else {
            data.len()
        };

        if start <= data.len() && end <= data.len() && start <= end {
            elements.push((entry.id, entry.is_large, &data[start..end]));
        }
    }
    elements
}

pub fn extract_elements_from_table(
    data: &[u8],
    table: &ParsedContainerTable,
) -> Vec<(u32, Vec<u8>)> {
    extract_slices_from_table(data, table)
        .into_iter()
        .map(|(id, _, slice)| (id, slice.to_vec()))
        .collect()
}

pub fn serialize_container_payload<'a, I>(elements: I) -> Vec<u8>
where
    I: IntoIterator<Item = (u32, bool, &'a [u8])>,
{
    serialize_container_payload_with_endian(elements, Endian::Little)
}

pub fn serialize_container_payload_with_endian<'a, I>(elements: I, endian: Endian) -> Vec<u8>
where
    I: IntoIterator<Item = (u32, bool, &'a [u8])>,
{
    let mut small_entries = Vec::new();
    let mut large_entries = Vec::new();
    let mut current_offset = 0usize;
    let mut data_segment = Vec::new();

    for (id, force_large, data) in elements {
        if !force_large && id <= 255 && current_offset <= 255 {
            small_entries.push((id as u8, current_offset as u8));
        } else {
            large_entries.push((id, current_offset as u32));
        }
        data_segment.extend_from_slice(data);
        current_offset += data.len();
    }

    let has_large = !large_entries.is_empty();
    let mut control_byte = (small_entries.len() & 0x7F) as u8;
    if has_large {
        control_byte |= 0x80;
    }

    let mut out = Vec::with_capacity(
        1 + 4 + small_entries.len() * 2 + large_entries.len() * 8 + data_segment.len(),
    );
    out.push(control_byte);

    if has_large {
        let _ = endian.write_u32(&mut out, large_entries.len() as u32);
    }
    for (id, offset) in small_entries {
        out.push(id);
        out.push(offset);
    }
    for (id, offset) in large_entries {
        let _ = endian.write_u32(&mut out, id);
        let _ = endian.write_u32(&mut out, offset);
    }
    out.extend_from_slice(&data_segment);
    out
}

pub fn read_length_prefixed_string(data: &[u8]) -> Option<String> {
    if data.len() < 5 {
        return None;
    }
    let len = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
    if len == 0 || len > 2048 || len > data.len() - 4 {
        return None;
    }
    let slice = &data[4..4 + len];
    let clean = slice.strip_suffix(&[0]).unwrap_or(slice);

    if !clean
        .iter()
        .all(|&b| (0x20..=0x7E).contains(&b) || b == b'\t' || b == b'\r' || b == b'\n')
    {
        return None;
    }

    std::str::from_utf8(clean)
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn write_length_prefixed_string(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(4 + bytes.len());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
    out
}

pub fn calculate_crc32(data: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

pub fn calculate_triumph_crc32(data: &[u8]) -> u32 {
    !calculate_crc32(data)
}
