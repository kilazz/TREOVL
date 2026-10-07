use anyhow::{Result, bail};

use crate::engine::common::{
    Endian, extract_elements_from_table, magic, parse_raw_container_table,
    serialize_container_payload_with_endian,
};

pub mod animation;
pub mod attachment;
pub mod audio;
pub mod character;
pub mod codec;
pub mod collision;
pub mod cptx;
pub mod dta;
pub mod environment;
pub mod event;
pub mod facefx;
pub mod font;
pub mod lua;
pub mod m8ld;
pub mod map;
pub mod material;
pub mod mesh;
pub mod object;
pub mod parameter;
pub mod projectile;
pub mod shader;
pub mod sniffer;
pub mod terrain;
pub mod terrain_palette;
pub mod texture;
pub mod ui;
pub mod ui_sprite;
pub mod vfx;
pub mod vpk;
pub mod xml;

pub type ChunkElement = (u32, Vec<u8>);

pub fn parse_chunk_elements(data: &[u8]) -> Result<(bool, Vec<ChunkElement>)> {
    let table = parse_raw_container_table(data, 0, true)?;
    let elements = extract_elements_from_table(data, &table);
    Ok((table.has_magic, elements))
}

pub fn build_chunk_from_elements(has_magic: bool, elements: &[ChunkElement]) -> Vec<u8> {
    build_chunk_from_elements_with_endian(has_magic, elements, Endian::Little)
}

pub fn build_chunk_from_elements_with_endian(
    has_magic: bool,
    elements: &[ChunkElement],
    endian: Endian,
) -> Vec<u8> {
    let mut table = Vec::new();
    if has_magic {
        table.extend_from_slice(magic::CONTAINER_MAGIC);
    }
    let entries = elements
        .iter()
        .map(|(id, data)| (*id, false, data.as_slice()));
    table.extend(serialize_container_payload_with_endian(entries, endian));
    table
}

pub fn parse_typed_container(data: &[u8]) -> Result<(u32, Vec<ChunkElement>)> {
    if data.len() < 5 {
        bail!("Data is too short to be a typed container.");
    }
    let type_id = u32::from_le_bytes(data[0..4].try_into()?);
    let table = parse_raw_container_table(data, 4, false)?;
    let elements = extract_elements_from_table(data, &table);
    Ok((type_id, elements))
}

pub fn build_typed_container(type_id: u32, elements: &[ChunkElement]) -> Vec<u8> {
    build_typed_container_with_endian(type_id, elements, Endian::Little)
}

pub fn build_typed_container_with_endian(
    type_id: u32,
    elements: &[ChunkElement],
    endian: Endian,
) -> Vec<u8> {
    let mut out = Vec::new();
    let _ = endian.write_u32(&mut out, type_id);
    let entries = elements
        .iter()
        .map(|(id, data)| (*id, false, data.as_slice()));
    out.extend(serialize_container_payload_with_endian(entries, endian));
    out
}
