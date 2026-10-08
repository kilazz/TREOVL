use anyhow::{Result, bail};

use crate::engine::common::{
    Endian, extract_elements_from_table, magic, parse_raw_container_table,
    serialize_container_payload_with_endian,
};

// 1. Core / Foundation
pub mod animation;
pub mod attachment;
pub mod character;
pub mod collision;
pub mod mesh;
pub mod object;
pub mod projectile;
pub mod shader;

// 2. Logic & Scripting
#[path = "logic/event.rs"]
pub mod event;
#[path = "logic/lua.rs"]
pub mod lua;
#[path = "logic/m8ld.rs"]
pub mod m8ld;
#[path = "logic/parameter.rs"]
pub mod parameter;
#[path = "logic/vfx.rs"]
pub mod vfx;

// 3. Media & Resources
#[path = "media/audio.rs"]
pub mod audio;
#[path = "media/facefx.rs"]
pub mod facefx;
#[path = "media/material.rs"]
pub mod material;
#[path = "media/texture.rs"]
pub mod texture;
#[path = "media/vpk.rs"]
pub mod vpk;

// 4. UI Layouts & Graphics
#[path = "ui/cptx.rs"]
pub mod cptx;
#[path = "ui/font.rs"]
pub mod font;
#[path = "ui/ui.rs"]
pub mod ui;
#[path = "ui/ui_sprite.rs"]
pub mod ui_sprite;

// 5. World & Environment
#[path = "world/dta.rs"]
pub mod dta;
#[path = "world/environment.rs"]
pub mod environment;
#[path = "world/map.rs"]
pub mod map;
#[path = "world/terrain.rs"]
pub mod terrain;
#[path = "world/terrain_palette.rs"]
pub mod terrain_palette;

// 6. System & Handlers
pub mod codec;
pub mod handler;
pub mod sniffer;
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
