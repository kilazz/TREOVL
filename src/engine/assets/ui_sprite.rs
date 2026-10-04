use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::{build_typed_container, parse_typed_container};
use crate::engine::common::{read_length_prefixed_string, write_length_prefixed_string};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpriteSliceJson {
    pub slice_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture_link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset_x: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset_y: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advance_x: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub unmapped_data: Vec<(u32, String)>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpriteCollectionJson {
    pub prefix_hex: String,
    pub type_id_hex: String,
    pub sprites: Vec<SpriteSliceJson>,
}

pub fn export_ui_sprite_collection(data: &[u8]) -> Result<String> {
    // In files like Font14.clb and Ingame10.clb, the real container starts a bit further in.
    let magic_pos = data
        .windows(3)
        .position(|w| w == b"\x01\x01\x00")
        .context("Could not find CONTAINER_MAGIC (01 01 00) in UI Sprite file")?;

    if magic_pos < 4 {
        bail!("Invalid container structure: magic bytes located too early.");
    }

    let container_start = magic_pos - 4;
    let prefix = &data[..container_start];

    let (type_id, elements) = parse_typed_container(&data[container_start..])
        .context("Failed to parse Sprite Collection container")?;

    let mut sprites = Vec::new();

    for (id, chunk) in elements {
        if let Ok((slice_type_id, props)) = parse_typed_container(&chunk) {
            // TypeID 0x00410078 = TREUISpriteSlice
            if slice_type_id == 0x00410078 {
                let mut slice = SpriteSliceJson {
                    slice_id: id,
                    collection_name: None,
                    texture_link: None,
                    width: None,
                    height: None,
                    offset_x: None,
                    offset_y: None,
                    advance_x: None,
                    unmapped_data: Vec::new(),
                };

                for (pid, pdata) in props {
                    match pid {
                        20 => slice.collection_name = read_length_prefixed_string(&pdata),
                        21 => slice.texture_link = read_length_prefixed_string(&pdata),
                        32 if pdata.len() >= 4 => {
                            slice.width = Cursor::new(&pdata).read_u32::<LittleEndian>().ok()
                        }
                        33 if pdata.len() >= 4 => {
                            slice.height = Cursor::new(&pdata).read_u32::<LittleEndian>().ok()
                        }
                        40 if pdata.len() >= 4 => {
                            slice.offset_x = Cursor::new(&pdata).read_u32::<LittleEndian>().ok()
                        }
                        41 if pdata.len() >= 4 => {
                            slice.offset_y = Cursor::new(&pdata).read_u32::<LittleEndian>().ok()
                        }
                        19 if pdata.len() >= 4 => {
                            slice.advance_x = Cursor::new(&pdata).read_u32::<LittleEndian>().ok()
                        }
                        _ => slice.unmapped_data.push((pid, hex::encode_upper(&pdata))),
                    }
                }
                sprites.push(slice);
            }
        }
    }

    let json_data = SpriteCollectionJson {
        prefix_hex: hex::encode_upper(prefix),
        type_id_hex: format!("{:08X}", type_id),
        sprites,
    };

    serde_json::to_string_pretty(&json_data)
        .context("Failed to serialize Sprite Collection to JSON")
}

pub fn import_ui_sprite_collection(json_str: &str) -> Result<Vec<u8>> {
    let parsed: SpriteCollectionJson = serde_json::from_str(json_str)?;
    let type_id = u32::from_str_radix(&parsed.type_id_hex, 16).unwrap_or(0x00410060);

    let mut elements = Vec::new();

    for slice in parsed.sprites {
        let mut props = Vec::new();

        if let Some(ref s) = slice.collection_name {
            props.push((20, write_length_prefixed_string(s)));
        }
        if let Some(ref s) = slice.texture_link {
            props.push((21, write_length_prefixed_string(s)));
        }
        if let Some(v) = slice.width {
            props.push((32, v.to_le_bytes().to_vec()));
        }
        if let Some(v) = slice.height {
            props.push((33, v.to_le_bytes().to_vec()));
        }
        if let Some(v) = slice.offset_x {
            props.push((40, v.to_le_bytes().to_vec()));
        }
        if let Some(v) = slice.offset_y {
            props.push((41, v.to_le_bytes().to_vec()));
        }
        if let Some(v) = slice.advance_x {
            props.push((19, v.to_le_bytes().to_vec()));
        }

        for (pid, hex_str) in slice.unmapped_data {
            if let Ok(b) = hex::decode(&hex_str) {
                props.push((pid, b));
            }
        }

        props.sort_by_key(|&(id, _)| id);

        // 0x00410078 is the hardcoded TypeID for TREUISpriteSlice
        let slice_bin = build_typed_container(0x00410078, &props);
        elements.push((slice.slice_id, slice_bin));
    }

    elements.sort_by_key(|&(id, _)| id);
    let container_bin = build_typed_container(type_id, &elements);

    let mut final_bin = Vec::new();
    if let Ok(prefix) = hex::decode(&parsed.prefix_hex) {
        final_bin.extend_from_slice(&prefix);
    }
    final_bin.extend_from_slice(&container_bin);

    Ok(final_bin)
}
