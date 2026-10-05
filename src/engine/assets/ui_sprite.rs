use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::{
    build_chunk_from_elements, build_typed_container, parse_chunk_elements, parse_typed_container,
};
use crate::engine::common::{read_length_prefixed_string, write_length_prefixed_string};

pub const CRL_MAGIC: &[u8; 4] = b"CRL\0";
pub const CRL_HEADER_SIZE: usize = 24;
pub const SPRITE_COLLECTION_TYPE_ID: u32 = 0x00410060;
pub const SPRITE_SLICE_TYPE_ID: u32 = 0x00410078;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CrlHeaderJson {
    pub magic: String,
    pub type_id_hex: String,
    pub version: u32,
    pub payload_size: u32,
    pub table_offset: u32,
    pub reserved: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpriteSliceJson {
    pub slice_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset_x: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset_y: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advance_x: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atlas_offset: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub unmapped_data: Vec<(u32, String)>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpriteCollectionJson {
    pub collection_name: String,
    pub header: CrlHeaderJson,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_header_hex: Option<String>,
    pub sprites: Vec<SpriteSliceJson>,
}

pub fn export_ui_sprite_collection(data: &[u8]) -> Result<String> {
    if data.len() < CRL_HEADER_SIZE {
        bail!("Data too short for CRL Sprite Collection (minimum 24 bytes required)");
    }

    let (header, root_container_data, raw_hdr_hex) = if data.starts_with(CRL_MAGIC) {
        let mut cur = Cursor::new(&data[..CRL_HEADER_SIZE]);
        let mut magic_bytes = [0u8; 4];
        std::io::Read::read_exact(&mut cur, &mut magic_bytes)?;
        let type_id = cur.read_u32::<LittleEndian>()?;
        let version = cur.read_u32::<LittleEndian>()?;
        let payload_size = cur.read_u32::<LittleEndian>()?;
        let table_offset = cur.read_u32::<LittleEndian>()?;
        let reserved = cur.read_u32::<LittleEndian>()?;

        let parsed_hdr = CrlHeaderJson {
            magic: String::from_utf8_lossy(&magic_bytes)
                .trim_matches(char::from(0))
                .to_string(),
            type_id_hex: format!("{:08X}", type_id),
            version,
            payload_size,
            table_offset,
            reserved,
        };

        (
            parsed_hdr,
            &data[CRL_HEADER_SIZE..],
            hex::encode_upper(&data[..CRL_HEADER_SIZE]),
        )
    } else {
        let def_hdr = CrlHeaderJson {
            magic: "CRL".to_string(),
            type_id_hex: format!("{:08X}", SPRITE_COLLECTION_TYPE_ID),
            version: 0,
            payload_size: data.len() as u32,
            table_offset: 0,
            reserved: 0,
        };
        (def_hdr, data, String::new())
    };

    let (_, root_elements) = parse_chunk_elements(root_container_data)
        .context("Failed to parse root CRL container table")?;

    let mut collection_name = String::from("Unnamed_Collection");
    let mut sprites_container_data = &[][..];

    for (id, chunk) in &root_elements {
        match *id {
            20 => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    collection_name = s;
                }
            }
            21 => {
                sprites_container_data = chunk.as_slice();
            }
            _ => {}
        }
    }

    if sprites_container_data.is_empty() {
        bail!("Missing sprite sub-container (Element ID 21) in Sprite Collection");
    }

    let (_, slice_elements) = parse_chunk_elements(sprites_container_data)
        .context("Failed to parse sprites sub-container (Element ID 21)")?;

    let mut sprites = Vec::with_capacity(slice_elements.len());

    for (slice_id, slice_data) in slice_elements {
        if let Ok((slice_type, props)) = parse_typed_container(&slice_data)
            && slice_type == SPRITE_SLICE_TYPE_ID
        {
            let mut slice = SpriteSliceJson {
                slice_id,
                slot_tag: None,
                texture_file: None,
                width: None,
                height: None,
                offset_x: None,
                offset_y: None,
                advance_x: None,
                cell_width: None,
                atlas_offset: None,
                unmapped_data: Vec::new(),
            };

            for (pid, pdata) in props {
                match pid {
                    20 => slice.slot_tag = read_length_prefixed_string(&pdata),
                    21 => slice.texture_file = read_length_prefixed_string(&pdata),
                    32 if pdata.len() >= 4 => {
                        slice.width = Cursor::new(&pdata).read_u32::<LittleEndian>().ok()
                    }
                    33 if pdata.len() >= 4 => {
                        slice.height = Cursor::new(&pdata).read_u32::<LittleEndian>().ok()
                    }
                    40 if pdata.len() >= 4 => {
                        slice.offset_x = Cursor::new(&pdata).read_i32::<LittleEndian>().ok()
                    }
                    41 if pdata.len() >= 4 => {
                        slice.offset_y = Cursor::new(&pdata).read_i32::<LittleEndian>().ok()
                    }
                    42 if pdata.len() >= 4 => {
                        slice.advance_x = Cursor::new(&pdata).read_i32::<LittleEndian>().ok()
                    }
                    43 if pdata.len() >= 4 => {
                        slice.cell_width = Cursor::new(&pdata).read_u32::<LittleEndian>().ok()
                    }
                    19 if pdata.len() >= 4 => {
                        slice.atlas_offset = Cursor::new(&pdata).read_u32::<LittleEndian>().ok()
                    }
                    _ => slice.unmapped_data.push((pid, hex::encode_upper(&pdata))),
                }
            }
            sprites.push(slice);
        }
    }

    let collection_json = SpriteCollectionJson {
        collection_name,
        header,
        raw_header_hex: if raw_hdr_hex.is_empty() {
            None
        } else {
            Some(raw_hdr_hex)
        },
        sprites,
    };

    serde_json::to_string_pretty(&collection_json)
        .context("Failed to serialize Sprite Collection to JSON")
}

pub fn import_ui_sprite_collection(json_str: &str) -> Result<Vec<u8>> {
    let parsed: SpriteCollectionJson = serde_json::from_str(json_str)?;

    let mut slice_entries = Vec::with_capacity(parsed.sprites.len());

    for s in parsed.sprites {
        let mut props = Vec::new();

        if let Some(ref tag) = s.slot_tag {
            props.push((20, write_length_prefixed_string(tag)));
        }
        if let Some(ref file) = s.texture_file {
            props.push((21, write_length_prefixed_string(file)));
        }
        if let Some(w) = s.width {
            props.push((32, w.to_le_bytes().to_vec()));
        }
        if let Some(h) = s.height {
            props.push((33, h.to_le_bytes().to_vec()));
        }
        if let Some(ox) = s.offset_x {
            props.push((40, ox.to_le_bytes().to_vec()));
        }
        if let Some(oy) = s.offset_y {
            props.push((41, oy.to_le_bytes().to_vec()));
        }
        if let Some(adv) = s.advance_x {
            props.push((42, adv.to_le_bytes().to_vec()));
        }
        if let Some(cw) = s.cell_width {
            props.push((43, cw.to_le_bytes().to_vec()));
        }
        if let Some(ao) = s.atlas_offset {
            props.push((19, ao.to_le_bytes().to_vec()));
        }

        for (pid, hex_str) in s.unmapped_data {
            if let Ok(b) = hex::decode(&hex_str) {
                props.push((pid, b));
            }
        }

        props.sort_by_key(|&(id, _)| id);
        let slice_bin = build_typed_container(SPRITE_SLICE_TYPE_ID, &props);
        slice_entries.push((s.slice_id, slice_bin));
    }

    slice_entries.sort_by_key(|&(id, _)| id);

    let sprites_subcontainer = build_chunk_from_elements(true, &slice_entries);
    let root_elements = vec![
        (20, write_length_prefixed_string(&parsed.collection_name)),
        (21, sprites_subcontainer),
    ];
    let root_container_bytes = build_chunk_from_elements(false, &root_elements);

    let mut final_file = if let Some(ref hex_hdr) = parsed.raw_header_hex
        && let Ok(hdr) = hex::decode(hex_hdr)
        && hdr.len() == CRL_HEADER_SIZE
    {
        hdr
    } else {
        let type_id = u32::from_str_radix(parsed.header.type_id_hex.trim_start_matches("0x"), 16)
            .unwrap_or(SPRITE_COLLECTION_TYPE_ID);

        let mut def_hdr = vec![0u8; CRL_HEADER_SIZE];
        def_hdr[..4].copy_from_slice(CRL_MAGIC);
        def_hdr[4..8].copy_from_slice(&type_id.to_le_bytes());
        def_hdr[8..12].copy_from_slice(&parsed.header.version.to_le_bytes());
        def_hdr[12..16].copy_from_slice(&(root_container_bytes.len() as u32).to_le_bytes());
        def_hdr[16..20].copy_from_slice(&parsed.header.table_offset.to_le_bytes());
        def_hdr[20..24].copy_from_slice(&parsed.header.reserved.to_le_bytes());
        def_hdr
    };

    let total_payload_len = root_container_bytes.len() as u32;
    final_file[12..16].copy_from_slice(&total_payload_len.to_le_bytes());
    final_file.extend_from_slice(&root_container_bytes);

    Ok(final_file)
}
