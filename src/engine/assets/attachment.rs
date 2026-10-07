use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::{
    build_chunk_from_elements_with_endian, build_typed_container_with_endian, parse_chunk_elements,
    parse_typed_container,
};
use crate::engine::common::{Endian, read_length_prefixed_string};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ItemAttachmentJson {
    pub _engine_metadata: AttachmentEngineMetadataJson,
    pub item_name: String,
    pub internal_model_slot: String,
    pub mesh_package: String,
    pub submesh_name: String,
    pub sound_bank: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drop_sound: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact_sound: Option<String>,
    pub hold_offset: [f32; 3],
    pub flags: ItemFlagsJson,
    pub socket: ItemSocketConfigJson,
    pub physics: ItemPhysicsConfigJson,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AttachmentEngineMetadataJson {
    pub resource_tag: String,
    pub type_id_hex: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub unmapped_properties: Vec<RawAttachmentProp>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ItemFlagsJson {
    pub is_pickable: bool,
    pub cast_shadows: bool,
    pub drop_physics: bool,
    #[serde(default = "default_unmapped_flags")]
    pub raw_mask_hex: String,
}

fn default_unmapped_flags() -> String {
    "0x21400000".to_string()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ItemSocketConfigJson {
    pub mount_point: String,
    pub primary_slot: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_slot: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ItemPhysicsConfigJson {
    pub category_id: u32,
    pub world_collision: bool,
    pub is_buoyant: bool,
    pub damage_on_throw: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RawAttachmentProp {
    pub id: u32,
    pub hex: String,
}

pub fn export_attachment_to_json(data: &[u8]) -> Result<String> {
    if data.len() < 7 {
        bail!("Data too short for Item / Weapon Attachment");
    }

    let (_, root_elements) =
        parse_chunk_elements(data).context("Failed to parse root Attachment container")?;

    let mut resource_tag = String::new();
    let mut item_name = String::from("Item");
    let mut internal_model_slot = String::new();
    let mut mesh_package = String::new();
    let mut submesh_name = String::new();
    let mut sound_bank = String::new();
    let mut drop_sound = None;
    let mut impact_sound = None;
    let mut hold_offset = [0.0f32, 0.0f32, 0.0f32];
    let mut item_type_id = 0x0046200Du32;

    let mut flags = ItemFlagsJson {
        is_pickable: true,
        cast_shadows: true,
        drop_physics: true,
        raw_mask_hex: "0x21400000".into(),
    };

    let mut socket = ItemSocketConfigJson {
        mount_point: "Right_Hand_Carry".into(),
        primary_slot: 40,
        secondary_slot: Some(43),
    };

    let mut physics = ItemPhysicsConfigJson {
        category_id: 5633,
        world_collision: true,
        is_buoyant: false,
        damage_on_throw: true,
    };

    let mut unmapped_properties = Vec::new();

    if let Some((_, tag_bytes)) = root_elements.iter().find(|(id, _)| *id == 20)
        && let Some(s) = read_length_prefixed_string(tag_bytes)
    {
        resource_tag = s;
    }

    for (_, chunk_bytes) in &root_elements {
        let payload_opt = if let Some(pos) = chunk_bytes
            .windows(4)
            .position(|w| w == b"\x0D\x20\x46\x00")
        {
            item_type_id = 0x0046200D;
            Some(&chunk_bytes[pos..])
        } else if let Some(pos) = chunk_bytes
            .windows(4)
            .position(|w| w == b"\x15\x20\x46\x00")
        {
            item_type_id = 0x00462015;
            Some(&chunk_bytes[pos..])
        } else if chunk_bytes.starts_with(b"\x01\x01\x00") && chunk_bytes.len() > 6 {
            Some(&chunk_bytes[6..])
        } else {
            None
        };

        if let Some(payload) = payload_opt
            && let Ok((type_id, elements)) = parse_typed_container(payload)
        {
            item_type_id = type_id;
            for (id, chunk) in elements {
                match id {
                    20 => {
                        if let Some(s) = read_length_prefixed_string(&chunk) {
                            internal_model_slot = s;
                        }
                    }
                    21 => {
                        if let Some(s) = read_length_prefixed_string(&chunk) {
                            item_name = s;
                        }
                    }
                    22 if chunk.len() >= 4 => {
                        let mask = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
                        flags = ItemFlagsJson {
                            is_pickable: (mask & 0x01) != 0,
                            cast_shadows: (mask & 0x08) != 0,
                            drop_physics: (mask & 0x0040_0000) != 0,
                            raw_mask_hex: format!("0x{:08X}", mask & !(0x01 | 0x08 | 0x0040_0000)),
                        };
                    }
                    29 => {
                        socket = parse_socket_data(&chunk);
                    }
                    30 => {
                        if let Ok((_, m_elems)) = parse_chunk_elements(&chunk) {
                            for (mid, mdata) in m_elems {
                                if mid == 20
                                    && let Some(s) = read_length_prefixed_string(&mdata)
                                {
                                    mesh_package = s;
                                } else if mid == 21
                                    && let Some(s) = read_length_prefixed_string(&mdata)
                                {
                                    submesh_name = s;
                                }
                            }
                        }
                    }
                    35 if chunk.len() >= 4 => {
                        physics.category_id =
                            u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
                    }
                    137 if !chunk.is_empty() => {
                        physics.world_collision = chunk[0] != 0;
                    }
                    138 if !chunk.is_empty() => {
                        physics.is_buoyant = chunk[0] != 0;
                    }
                    139 if !chunk.is_empty() => {
                        physics.damage_on_throw = chunk[0] != 0;
                    }
                    143 => {
                        if let Some(pos) = parse_transform_offset(&chunk) {
                            hold_offset = pos;
                        }
                    }
                    _ => {
                        if let Some(s) = read_length_prefixed_string(&chunk) {
                            if s.starts_with("Drop ") {
                                drop_sound = Some(s);
                                continue;
                            } else if s.contains("Impact") || s.contains("Hit") {
                                impact_sound = Some(s);
                                continue;
                            }
                        }
                        if ![23, 28, 36, 140, 141, 19, 1].contains(&id) {
                            unmapped_properties.push(RawAttachmentProp {
                                id,
                                hex: hex::encode_upper(&chunk),
                            });
                        }
                    }
                }
            }
        }
    }

    if let Some((_, snd_bytes)) = root_elements.iter().find(|(id, _)| *id == 30)
        && let Some(pos) = snd_bytes.windows(4).position(|w| w == b"\x57\x00\x00\x04")
        && let Ok((_, s_elements)) = parse_typed_container(&snd_bytes[pos..])
    {
        for (sid, sdata) in s_elements {
            if sid == 10
                && let Some(s) = read_length_prefixed_string(&sdata)
            {
                sound_bank = s;
            }
        }
    }

    let metadata = AttachmentEngineMetadataJson {
        resource_tag,
        type_id_hex: format!("{:08X}", item_type_id),
        unmapped_properties,
    };

    let item_json = ItemAttachmentJson {
        _engine_metadata: metadata,
        item_name,
        internal_model_slot,
        mesh_package,
        submesh_name,
        sound_bank,
        drop_sound,
        impact_sound,
        hold_offset,
        flags,
        socket,
        physics,
    };

    serde_json::to_string_pretty(&item_json).map_err(|e| anyhow::anyhow!(e))
}

pub fn import_attachment_from_json(json_str: &str) -> Result<Vec<u8>> {
    import_attachment_from_json_with_endian(json_str, Endian::Little)
}

pub fn import_attachment_from_json_with_endian(json_str: &str, endian: Endian) -> Result<Vec<u8>> {
    let parsed: ItemAttachmentJson =
        serde_json::from_str(json_str).context("Syntax error in Item Attachment JSON format")?;

    let type_id = u32::from_str_radix(
        parsed
            ._engine_metadata
            .type_id_hex
            .trim()
            .trim_start_matches("0x"),
        16,
    )
    .unwrap_or(0x0046200D);

    let mut item_sub = Vec::new();
    item_sub.push((
        20,
        endian.write_length_prefixed_string(&parsed.internal_model_slot),
    ));
    item_sub.push((21, endian.write_length_prefixed_string(&parsed.item_name)));

    let raw_hex = parsed
        .flags
        .raw_mask_hex
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X");
    let mut flag_bits = u32::from_str_radix(raw_hex, 16).unwrap_or(0x21400000);

    if parsed.flags.is_pickable {
        flag_bits |= 0x01;
    }
    if parsed.flags.cast_shadows {
        flag_bits |= 0x08;
    }
    if parsed.flags.drop_physics {
        flag_bits |= 0x0040_0000;
    }
    item_sub.push((22, endian.u32_to_bytes(flag_bits).to_vec()));

    item_sub.push((23, vec![1u8]));
    item_sub.push((28, vec![0u8]));
    item_sub.push((29, build_socket_data(&parsed.socket)));

    let mesh_elems = vec![
        (
            20,
            endian.write_length_prefixed_string(&parsed.mesh_package),
        ),
        (
            21,
            endian.write_length_prefixed_string(&parsed.submesh_name),
        ),
    ];
    item_sub.push((
        30,
        build_chunk_from_elements_with_endian(false, &mesh_elems, endian),
    ));

    item_sub.push((35, endian.u32_to_bytes(parsed.physics.category_id).to_vec()));
    item_sub.push((36, vec![1u8, 1, 0, 0]));
    item_sub.push((
        137,
        vec![
            if parsed.physics.world_collision {
                1u8
            } else {
                0u8
            },
            1,
            0,
            0,
        ],
    ));
    item_sub.push((138, vec![if parsed.physics.is_buoyant { 1u8 } else { 0u8 }]));
    item_sub.push((
        139,
        vec![
            if parsed.physics.damage_on_throw {
                1u8
            } else {
                0u8
            },
            1,
            0,
            0,
        ],
    ));
    item_sub.push((140, vec![1u8, 1, 0, 0]));
    item_sub.push((141, vec![1u8, 1, 0, 0]));

    item_sub.push((143, build_transform_offset(parsed.hold_offset, endian)));

    for prop in parsed._engine_metadata.unmapped_properties {
        if let Ok(b) = hex::decode(&prop.hex) {
            item_sub.push((prop.id, b));
        }
    }

    item_sub.push((19, vec![0xFF, 0xFF, 0xFF, 0xFF]));
    item_sub.push((1, vec![0u8]));
    item_sub.sort_by_key(|&(id, _)| id);

    let item_resource_blob = build_typed_container_with_endian(type_id, &item_sub, endian);

    let sound_elems = vec![
        (10, endian.write_length_prefixed_string(&parsed.sound_bank)),
        (11, vec![0, 0, 0, 0]),
    ];
    let sound_blob = build_typed_container_with_endian(0x0400_0057, &sound_elems, endian);

    let mut prefix_21 = Vec::new();
    let slot_tag = parsed
        .internal_model_slot
        .split('\\')
        .next()
        .unwrap_or("9872")
        .trim_start_matches('[')
        .trim_end_matches(']');
    prefix_21.extend_from_slice(&endian.write_length_prefixed_string(slot_tag));
    prefix_21.extend_from_slice(&item_resource_blob);

    let root_elements = vec![
        (
            20,
            endian.write_length_prefixed_string(&parsed._engine_metadata.resource_tag),
        ),
        (21, prefix_21),
        (30, sound_blob),
    ];

    Ok(build_chunk_from_elements_with_endian(
        false,
        &root_elements,
        endian,
    ))
}

fn parse_transform_offset(chunk: &[u8]) -> Option<[f32; 3]> {
    if chunk.len() >= 15 && chunk[0] == 1 && chunk[1] == 20 {
        let mut cur = Cursor::new(&chunk[3..15]);
        let x = cur.read_f32::<LittleEndian>().ok()?;
        let y = cur.read_f32::<LittleEndian>().ok()?;
        let z = cur.read_f32::<LittleEndian>().ok()?;
        if x.is_finite() && y.is_finite() && z.is_finite() {
            return Some([x, y, z]);
        }
    }
    None
}

fn build_transform_offset(offset: [f32; 3], endian: Endian) -> Vec<u8> {
    let mut out = Vec::with_capacity(15);
    out.push(1);
    out.push(20);
    out.push(0);
    let _ = endian.write_f32(&mut out, offset[0]);
    let _ = endian.write_f32(&mut out, offset[1]);
    let _ = endian.write_f32(&mut out, offset[2]);
    out
}

fn parse_socket_data(chunk: &[u8]) -> ItemSocketConfigJson {
    if chunk.len() >= 13 && chunk.starts_with(&[1, 40, 0, 2, 40, 0, 43, 4]) {
        ItemSocketConfigJson {
            mount_point: "Right_Hand_Carry".into(),
            primary_slot: 40,
            secondary_slot: Some(43),
        }
    } else {
        ItemSocketConfigJson {
            mount_point: "Standard_Grip".into(),
            primary_slot: 40,
            secondary_slot: None,
        }
    }
}

fn build_socket_data(socket: &ItemSocketConfigJson) -> Vec<u8> {
    if socket.secondary_slot.is_some() {
        vec![1, 40, 0, 2, 40, 0, 43, 4, 1, 1, 0, 0, 0]
    } else {
        vec![1, 40, 0, 1, 40, 0, 1, 1, 0, 0]
    }
}
