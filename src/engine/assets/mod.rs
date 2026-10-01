use anyhow::{Result, bail};
use byteorder::{LittleEndian, WriteBytesExt};

use crate::engine::common::{magic, parse_raw_container_table};

pub mod animation;
pub mod audio;
pub mod lua;
pub mod map;
pub mod material;
pub mod mesh;
pub mod object;
pub mod shader;
pub mod sniffer;
pub mod terrain;
pub mod texture;
pub mod ui;
pub mod vfx;

pub type ChunkElement = (u32, Vec<u8>);

pub fn parse_chunk_elements(data: &[u8]) -> Result<(bool, Vec<ChunkElement>)> {
    let table = parse_raw_container_table(data, 0, true)?;
    let mut elements = Vec::with_capacity(table.entries.len());

    for i in 0..table.entries.len() {
        let start = table.data_start + table.entries[i].offset;
        let end = if i + 1 < table.entries.len() {
            table.data_start + table.entries[i + 1].offset
        } else {
            data.len()
        };

        if start <= data.len() && end <= data.len() && start <= end {
            elements.push((table.entries[i].id, data[start..end].to_vec()));
        }
    }

    Ok((table.has_magic, elements))
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

    let type_id = u32::from_le_bytes(data[0..4].try_into()?);
    let table = parse_raw_container_table(data, 4, false)?;
    let mut elements = Vec::with_capacity(table.entries.len());

    for i in 0..table.entries.len() {
        let start = table.data_start + table.entries[i].offset;
        let end = if i + 1 < table.entries.len() {
            table.data_start + table.entries[i + 1].offset
        } else {
            data.len()
        };

        if start <= data.len() && end <= data.len() && start <= end {
            elements.push((table.entries[i].id, data[start..end].to_vec()));
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
