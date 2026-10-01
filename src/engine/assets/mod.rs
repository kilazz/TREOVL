use anyhow::{Result, bail};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

use crate::engine::common::magic;

pub mod animation;
pub mod audio;
pub mod lua;
pub mod map;
pub mod material;
pub mod mesh;
pub mod shader;
pub mod sniffer;
pub mod terrain;
pub mod texture;

pub type ChunkElement = (u32, Vec<u8>);

pub fn parse_chunk_elements(data: &[u8]) -> Result<(bool, Vec<ChunkElement>)> {
    let mut pos = 0;
    let mut has_magic = false;

    // ACTIVELY USES magic::CONTAINER_MAGIC
    if data.len() > 3 && &data[0..3] == magic::CONTAINER_MAGIC {
        has_magic = true;
        pos = 3;
    }

    if pos >= data.len() {
        bail!("Not a valid Asset Container block.");
    }

    let control_byte = data[pos];
    let has_large = (control_byte & 0x80) != 0;
    let small_count = (control_byte & 0x7F) as usize;
    pos += 1;

    let mut large_count = 0;
    if has_large {
        if pos + 4 > data.len() {
            bail!("Corrupted container header.");
        }
        let mut cur = Cursor::new(&data[pos..pos + 4]);
        large_count = cur.read_u32::<LittleEndian>()? as usize;
        pos += 4;
    }

    let total_entries = small_count + large_count;
    if total_entries == 0 || total_entries > 4096 {
        bail!("Invalid entry count in container.");
    }

    let table_size = (small_count * 2) + (large_count * 8);
    let data_start = pos + table_size;
    if data_start > data.len() {
        bail!("Table offsets exceed chunk boundaries.");
    }

    let mut cur = Cursor::new(&data[pos..pos + table_size]);
    let mut offsets = Vec::new();
    for _ in 0..small_count {
        offsets.push((cur.read_u8()? as u32, cur.read_u8()? as usize));
    }
    for _ in 0..large_count {
        offsets.push((
            cur.read_u32::<LittleEndian>()?,
            cur.read_u32::<LittleEndian>()? as usize,
        ));
    }
    offsets.sort_by_key(|&(_, off)| off);

    if offsets.is_empty() || offsets[0].1 != 0 {
        bail!("First table offset is not zero.");
    }

    let mut elements = Vec::new();
    for i in 0..offsets.len() {
        let (id, offset) = offsets[i];
        let start = data_start + offset;
        let end = if i + 1 < offsets.len() {
            data_start + offsets[i + 1].1
        } else {
            data.len()
        };

        if start <= data.len() && end <= data.len() && start <= end {
            elements.push((id, data[start..end].to_vec()));
        }
    }

    Ok((has_magic, elements))
}

pub fn build_chunk_from_elements(has_magic: bool, elements: &[ChunkElement]) -> Vec<u8> {
    let mut table = Vec::new();
    if has_magic {
        table.extend_from_slice(magic::CONTAINER_MAGIC);
    }

    let mut small_entries = Vec::new();
    let mut large_entries = Vec::new();
    let mut current_offset = 0;
    let mut data_segment: Vec<u8> = Vec::new();

    for (id, chunk) in elements {
        if *id <= 255 && current_offset <= 255 {
            small_entries.push((*id as u8, current_offset as u8));
        } else {
            large_entries.push((*id, current_offset as u32));
        }
        data_segment.extend(chunk);
        current_offset += chunk.len();
    }

    let has_large = !large_entries.is_empty();
    let mut control_byte = (small_entries.len() & 0x7F) as u8;
    if has_large {
        control_byte |= 0x80;
    }
    table.push(control_byte);

    if has_large {
        table
            .write_u32::<LittleEndian>(large_entries.len() as u32)
            .unwrap();
    }
    for (id, offset) in small_entries {
        table.write_u8(id).unwrap();
        table.write_u8(offset).unwrap();
    }
    for (id, offset) in large_entries {
        table.write_u32::<LittleEndian>(id).unwrap();
        table.write_u32::<LittleEndian>(offset).unwrap();
    }

    table.extend(data_segment);
    table
}

pub fn parse_typed_container(data: &[u8]) -> Result<(u32, Vec<ChunkElement>)> {
    if data.len() < 5 {
        bail!("Data is too short to be a typed container.");
    }

    let mut cur = Cursor::new(&data[0..4]);
    let type_id = cur.read_u32::<LittleEndian>()?;

    let payload = &data[4..];
    let control_byte = payload[0];
    let has_large = (control_byte & 0x80) != 0;
    let small_count = (control_byte & 0x7F) as usize;
    let mut pos = 1;

    let mut large_count = 0;
    if has_large {
        if pos + 4 > payload.len() {
            bail!("Corrupted container table.");
        }
        let mut c = Cursor::new(&payload[pos..pos + 4]);
        large_count = c.read_u32::<LittleEndian>()? as usize;
        pos += 4;
    }

    let total_entries = small_count + large_count;
    if total_entries == 0 || total_entries > 4096 {
        bail!("Invalid entry count in typed container.");
    }

    let table_size = (small_count * 2) + (large_count * 8);
    if pos + table_size > payload.len() {
        bail!("Table offsets exceed chunk boundaries.");
    }

    let data_start = pos + table_size;
    let mut c = Cursor::new(&payload[pos..pos + table_size]);

    let mut offsets = Vec::new();
    for _ in 0..small_count {
        offsets.push((c.read_u8()? as u32, c.read_u8()? as usize));
    }
    for _ in 0..large_count {
        offsets.push((
            c.read_u32::<LittleEndian>()?,
            c.read_u32::<LittleEndian>()? as usize,
        ));
    }
    offsets.sort_by_key(|&(_, off)| off);

    if offsets.is_empty() || offsets[0].1 != 0 {
        bail!("First table offset is not zero.");
    }

    let mut elements = Vec::new();
    for i in 0..offsets.len() {
        let (id, offset) = offsets[i];
        let start = data_start + offset;
        let end = if i + 1 < offsets.len() {
            data_start + offsets[i + 1].1
        } else {
            payload.len()
        };

        if start <= payload.len() && end <= payload.len() && start <= end {
            elements.push((id, payload[start..end].to_vec()));
        }
    }

    Ok((type_id, elements))
}

pub fn build_typed_container(type_id: u32, elements: &[ChunkElement]) -> Vec<u8> {
    let mut out = Vec::new();
    out.write_u32::<LittleEndian>(type_id).unwrap();

    let mut small_entries = Vec::new();
    let mut large_entries = Vec::new();
    let mut current_offset = 0;
    let mut data_segment: Vec<u8> = Vec::new();

    for (id, chunk) in elements {
        if *id <= 255 && current_offset <= 255 {
            small_entries.push((*id as u8, current_offset as u8));
        } else {
            large_entries.push((*id, current_offset as u32));
        }
        data_segment.extend(chunk);
        current_offset += chunk.len();
    }

    let has_large = !large_entries.is_empty();
    let mut control_byte = (small_entries.len() & 0x7F) as u8;
    if has_large {
        control_byte |= 0x80;
    }
    out.push(control_byte);

    if has_large {
        out.write_u32::<LittleEndian>(large_entries.len() as u32)
            .unwrap();
    }
    for (id, offset) in small_entries {
        out.write_u8(id).unwrap();
        out.write_u8(offset).unwrap();
    }
    for (id, offset) in large_entries {
        out.write_u32::<LittleEndian>(id).unwrap();
        out.write_u32::<LittleEndian>(offset).unwrap();
    }

    out.extend(data_segment);
    out
}
