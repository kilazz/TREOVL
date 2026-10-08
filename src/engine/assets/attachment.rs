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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub equipment_config: Option<ItemEquipmentConfigJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weapon_config: Option<ItemWeaponConfigJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breakable_config: Option<ItemBreakableConfigJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AttachmentEngineMetadataJson {
    pub resource_tag: String,
    pub type_id_hex: String,
    #[serde(default)]
    pub is_direct_container: bool,
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

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ItemEquipmentConfigJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_rating: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub equipment_category: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armor_value: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ItemWeaponConfigJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_two_handed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_multiplier: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weapon_flags: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_modifier: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack_range: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub impact_actions: Vec<ItemTriggerActionJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ItemBreakableConfigJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_health: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physics_material_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawn_particles: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_fuse: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub break_sound: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuse_sound: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact_sound_alt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debris_config_hex: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub fuse_actions: Vec<ItemTriggerActionJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub break_actions: Vec<ItemTriggerActionJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ItemTriggerActionJson {
    pub action_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_resource: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_id_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_sec: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_hex: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RawAttachmentProp {
    pub id: u32,
    pub hex: String,
}

pub fn parse_f32_safe(chunk: &[u8]) -> Option<f32> {
    if chunk.len() >= 4 {
        let val = f32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
        if val.is_finite() && !val.is_subnormal() && (1e-4..=500_000.0).contains(&val.abs()) {
            return Some(val);
        }
    }
    None
}

fn parse_embedded_string(chunk: &[u8]) -> Option<String> {
    if let Ok((_, elements)) = parse_chunk_elements(chunk) {
        for (id, data) in elements {
            if id == 20 {
                if data.is_empty() {
                    return Some("".to_string());
                }
                if let Some(s) = read_length_prefixed_string(&data) {
                    return Some(s);
                }
            }
        }
    }
    None
}

fn build_embedded_string(s: &str, endian: Endian) -> Vec<u8> {
    if s.is_empty() {
        vec![1, 20, 0, 0]
    } else {
        let sub = vec![(20, endian.write_length_prefixed_string(s))];
        build_chunk_from_elements_with_endian(false, &sub, endian)
    }
}

fn parse_item_trigger_actions(chunk: &[u8]) -> Vec<ItemTriggerActionJson> {
    let mut actions = Vec::new();
    if let Ok((_, sub_parts)) = parse_chunk_elements(chunk) {
        for (_, pdata) in sub_parts {
            let mut type_id_hex = None;
            let fields = if let Ok((type_id, f)) = parse_typed_container(&pdata) {
                type_id_hex = Some(format!("{:08X}", type_id));
                f
            } else if let Ok((_, f)) = parse_chunk_elements(&pdata) {
                f
            } else {
                vec![(0, pdata.clone())]
            };

            let mut action_name = String::new();
            let mut target_resource = None;
            let mut delay_sec = None;
            let mut is_active = None;

            for (fid, fdata) in fields {
                match fid {
                    25 if !fdata.is_empty() => is_active = Some(fdata[0] != 0),
                    26 | 28 => {
                        if let Some(f) = parse_f32_safe(&fdata) {
                            delay_sec = Some(f);
                        }
                    }
                    40 => {
                        if let Ok((_, s_fields)) = parse_chunk_elements(&fdata) {
                            for (sid, sdata) in s_fields {
                                if sid == 20
                                    && let Some(s) = read_length_prefixed_string(&sdata)
                                {
                                    action_name = s;
                                } else if sid == 21
                                    && let Some(s) = read_length_prefixed_string(&sdata)
                                {
                                    target_resource = Some(s);
                                }
                            }
                        }
                    }
                    20 => {
                        if action_name.is_empty()
                            && let Some(s) = read_length_prefixed_string(&fdata)
                        {
                            action_name = s;
                        }
                    }
                    21 => {
                        if target_resource.is_none()
                            && let Some(s) = read_length_prefixed_string(&fdata)
                        {
                            target_resource = Some(s);
                        }
                    }
                    _ => {}
                }
            }

            if action_name.is_empty() {
                action_name = "Trigger".into();
            }

            actions.push(ItemTriggerActionJson {
                action_name,
                target_resource,
                type_id_hex,
                delay_sec,
                is_active,
                raw_hex: None,
            });
        }
    }
    actions
}

fn rebuild_item_trigger_actions(
    actions: &[ItemTriggerActionJson],
    endian: Endian,
) -> Result<Vec<u8>> {
    let mut sub_chunks = Vec::new();

    for (i, act) in actions.iter().enumerate() {
        if let Some(ref raw_h) = act.raw_hex
            && let Ok(raw_b) = hex::decode(raw_h)
        {
            sub_chunks.push((i as u32, raw_b));
            continue;
        }

        let type_id = act
            .type_id_hex
            .as_deref()
            .and_then(|h| u32::from_str_radix(h.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0x00460758);

        let mut fields = Vec::new();
        if let Some(active) = act.is_active {
            fields.push((25, vec![if active { 1 } else { 0 }, 0, 0, 0]));
        }

        if let Some(delay) = act.delay_sec {
            fields.push((28, endian.f32_to_bytes(delay).to_vec()));
        }

        let mut name_sub = Vec::new();
        name_sub.push((20, endian.write_length_prefixed_string(&act.action_name)));
        if let Some(ref tr) = act.target_resource {
            name_sub.push((21, endian.write_length_prefixed_string(tr)));
        }

        let name_blob = build_chunk_from_elements_with_endian(false, &name_sub, endian);
        fields.push((40, name_blob));

        let act_blob = build_typed_container_with_endian(type_id, &fields, endian);
        sub_chunks.push((i as u32, act_blob));
    }

    Ok(build_chunk_from_elements_with_endian(
        true,
        &sub_chunks,
        endian,
    ))
}

pub fn export_attachment_to_json(data: &[u8]) -> Result<String> {
    if data.len() < 7 {
        bail!("Data too short for Item / Weapon Attachment");
    }

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
    let mut is_direct_container = false;

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

    let mut equip = ItemEquipmentConfigJson::default();
    let mut has_equip_data = false;

    let mut weapon = ItemWeaponConfigJson::default();
    let mut has_weapon_data = false;

    let mut breakable = ItemBreakableConfigJson::default();
    let mut has_breakable_data = false;

    let mut unmapped_properties = Vec::new();

    let direct_type = if data.len() >= 4 {
        u32::from_le_bytes(data[0..4].try_into().unwrap_or_default())
    } else {
        0
    };

    if (direct_type >> 8) == 0x004620 {
        is_direct_container = true;
        item_type_id = direct_type;

        if let Ok((_, elements)) = parse_typed_container(data) {
            for (id, chunk) in elements {
                match id {
                    20 => {
                        if let Some(s) = read_length_prefixed_string(&chunk) {
                            resource_tag = s.clone();
                            if internal_model_slot.is_empty() {
                                internal_model_slot = s;
                            }
                        }
                    }
                    21 => {
                        if let Some(s) = read_length_prefixed_string(&chunk)
                            && item_name == "Item"
                        {
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
                    25 => {
                        if let Some(s) = read_length_prefixed_string(&chunk) {
                            item_name = s;
                        }
                    }
                    26 if chunk.len() >= 4 => {
                        equip.power_rating = Some(u32::from_le_bytes(
                            chunk[0..4].try_into().unwrap_or_default(),
                        ));
                        has_equip_data = true;
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
                    34 if !chunk.is_empty() => {
                        equip.equipment_category = Some(chunk[0]);
                        has_equip_data = true;
                    }
                    35 if chunk.len() >= 4 => {
                        physics.category_id =
                            u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
                    }
                    40 => {
                        if item_type_id == 0x0046203F && chunk.len() >= 4 {
                            breakable.base_health = parse_f32_safe(&chunk);
                            has_breakable_data = true;
                        } else if !chunk.is_empty() {
                            weapon.is_two_handed = Some(chunk[0] != 0);
                            has_weapon_data = true;
                        }
                    }
                    41 => {
                        if item_type_id == 0x0046203F {
                            breakable.break_sound = parse_embedded_string(&chunk);
                            has_breakable_data = true;
                        } else {
                            weapon.impact_actions = parse_item_trigger_actions(&chunk);
                            has_weapon_data = true;
                        }
                    }
                    42 if item_type_id == 0x0046203F => {
                        breakable.fuse_sound = parse_embedded_string(&chunk);
                        has_breakable_data = true;
                    }
                    43 if item_type_id == 0x0046203F && chunk.len() >= 4 => {
                        breakable.physics_material_id = Some(u32::from_le_bytes(
                            chunk[0..4].try_into().unwrap_or_default(),
                        ));
                        has_breakable_data = true;
                    }
                    44 if item_type_id == 0x0046203F && !chunk.is_empty() => {
                        breakable.spawn_particles = Some(chunk[0] != 0);
                        has_breakable_data = true;
                    }
                    45 if item_type_id == 0x0046203F => {
                        breakable.impact_sound_alt = parse_embedded_string(&chunk);
                        has_breakable_data = true;
                    }
                    48 if item_type_id == 0x0046203F => {
                        breakable.fuse_actions = parse_item_trigger_actions(&chunk);
                        has_breakable_data = true;
                    }
                    49 if item_type_id == 0x0046203F => {
                        breakable.break_actions = parse_item_trigger_actions(&chunk);
                        has_breakable_data = true;
                    }
                    50 if item_type_id == 0x0046203F => {
                        breakable.debris_config_hex = Some(hex::encode_upper(&chunk));
                        has_breakable_data = true;
                    }
                    51 if item_type_id == 0x0046203F && !chunk.is_empty() => {
                        breakable.auto_fuse = Some(chunk[0] != 0);
                        has_breakable_data = true;
                    }
                    70 if chunk.len() >= 4 => {
                        weapon.damage_multiplier = parse_f32_safe(&chunk);
                        has_weapon_data = true;
                    }
                    71 if !chunk.is_empty() => {
                        let flag_val = if chunk.len() >= 4 {
                            u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default())
                        } else {
                            chunk[0] as u32
                        };
                        weapon.weapon_flags = Some(flag_val);
                        has_weapon_data = true;
                    }
                    72 if chunk.len() >= 4 => {
                        weapon.speed_modifier = parse_f32_safe(&chunk);
                        has_weapon_data = true;
                    }
                    74 if chunk.len() >= 4 => {
                        weapon.attack_range = parse_f32_safe(&chunk);
                        has_weapon_data = true;
                    }
                    83 if item_type_id == 0x0046200B && chunk.len() >= 4 => {
                        equip.armor_value = Some(u32::from_le_bytes(
                            chunk[0..4].try_into().unwrap_or_default(),
                        ));
                        has_equip_data = true;
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
                        if ![23, 28, 36, 75, 81, 140, 141, 19, 1].contains(&id) {
                            unmapped_properties.push(RawAttachmentProp {
                                id,
                                hex: hex::encode_upper(&chunk),
                            });
                        }
                    }
                }
            }
        }
    } else {
        let (_, root_elements) =
            parse_chunk_elements(data).context("Failed to parse root Attachment container")?;

        if let Some((_, tag_bytes)) = root_elements.iter().find(|(id, _)| *id == 20)
            && let Some(s) = read_length_prefixed_string(tag_bytes)
        {
            resource_tag = s;
        }

        for (root_id, chunk_bytes) in &root_elements {
            // Root element 30 is the sound container (0x04000057), parsed separately below
            if *root_id == 30 {
                continue;
            }

            let payload_opt =
                if chunk_bytes.len() >= 4 && chunk_bytes[2] == 0x46 && chunk_bytes[1] == 0x20 {
                    item_type_id =
                        u32::from_le_bytes(chunk_bytes[0..4].try_into().unwrap_or_default());
                    Some(chunk_bytes.as_slice())
                } else if chunk_bytes.starts_with(b"\x01\x01\x00") && chunk_bytes.len() > 6 {
                    Some(&chunk_bytes[6..])
                } else {
                    None
                };

            if let Some(payload) = payload_opt
                && let Ok((type_id, elements)) = parse_typed_container(payload)
            {
                // Fix: Only update item_type_id if it's from the item/prop family (0x004620xx)
                if (type_id >> 8) == 0x004620 {
                    item_type_id = type_id;
                }

                for (id, chunk) in elements {
                    match id {
                        20 => {
                            if let Some(s) = read_length_prefixed_string(&chunk) {
                                internal_model_slot = s;
                            }
                        }
                        21 => {
                            if let Some(s) = read_length_prefixed_string(&chunk)
                                && item_name == "Item"
                            {
                                item_name = s;
                            }
                        }
                        22 if chunk.len() >= 4 => {
                            let mask =
                                u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
                            flags = ItemFlagsJson {
                                is_pickable: (mask & 0x01) != 0,
                                cast_shadows: (mask & 0x08) != 0,
                                drop_physics: (mask & 0x0040_0000) != 0,
                                raw_mask_hex: format!(
                                    "0x{:08X}",
                                    mask & !(0x01 | 0x08 | 0x0040_0000)
                                ),
                            };
                        }
                        26 if chunk.len() >= 4 => {
                            equip.power_rating = Some(u32::from_le_bytes(
                                chunk[0..4].try_into().unwrap_or_default(),
                            ));
                            has_equip_data = true;
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
                        34 if !chunk.is_empty() => {
                            equip.equipment_category = Some(chunk[0]);
                            has_equip_data = true;
                        }
                        35 if chunk.len() >= 4 => {
                            physics.category_id =
                                u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
                        }
                        40 => {
                            if item_type_id == 0x0046203F && chunk.len() >= 4 {
                                breakable.base_health = parse_f32_safe(&chunk);
                                has_breakable_data = true;
                            } else if !chunk.is_empty() {
                                weapon.is_two_handed = Some(chunk[0] != 0);
                                has_weapon_data = true;
                            }
                        }
                        41 => {
                            if item_type_id == 0x0046203F {
                                breakable.break_sound = parse_embedded_string(&chunk);
                                has_breakable_data = true;
                            } else {
                                weapon.impact_actions = parse_item_trigger_actions(&chunk);
                                has_weapon_data = true;
                            }
                        }
                        42 if item_type_id == 0x0046203F => {
                            breakable.fuse_sound = parse_embedded_string(&chunk);
                            has_breakable_data = true;
                        }
                        43 if item_type_id == 0x0046203F && chunk.len() >= 4 => {
                            breakable.physics_material_id = Some(u32::from_le_bytes(
                                chunk[0..4].try_into().unwrap_or_default(),
                            ));
                            has_breakable_data = true;
                        }
                        44 if item_type_id == 0x0046203F && !chunk.is_empty() => {
                            breakable.spawn_particles = Some(chunk[0] != 0);
                            has_breakable_data = true;
                        }
                        45 if item_type_id == 0x0046203F => {
                            breakable.impact_sound_alt = parse_embedded_string(&chunk);
                            has_breakable_data = true;
                        }
                        48 if item_type_id == 0x0046203F => {
                            breakable.fuse_actions = parse_item_trigger_actions(&chunk);
                            has_breakable_data = true;
                        }
                        49 if item_type_id == 0x0046203F => {
                            breakable.break_actions = parse_item_trigger_actions(&chunk);
                            has_breakable_data = true;
                        }
                        50 if item_type_id == 0x0046203F => {
                            breakable.debris_config_hex = Some(hex::encode_upper(&chunk));
                            has_breakable_data = true;
                        }
                        51 if item_type_id == 0x0046203F && !chunk.is_empty() => {
                            breakable.auto_fuse = Some(chunk[0] != 0);
                            has_breakable_data = true;
                        }
                        70 if chunk.len() >= 4 => {
                            weapon.damage_multiplier = parse_f32_safe(&chunk);
                            has_weapon_data = true;
                        }
                        71 if !chunk.is_empty() => {
                            let flag_val = if chunk.len() >= 4 {
                                u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default())
                            } else {
                                chunk[0] as u32
                            };
                            weapon.weapon_flags = Some(flag_val);
                            has_weapon_data = true;
                        }
                        72 if chunk.len() >= 4 => {
                            weapon.speed_modifier = parse_f32_safe(&chunk);
                            has_weapon_data = true;
                        }
                        74 if chunk.len() >= 4 => {
                            weapon.attack_range = parse_f32_safe(&chunk);
                            has_weapon_data = true;
                        }
                        83 if item_type_id == 0x0046200B && chunk.len() >= 4 => {
                            equip.armor_value = Some(u32::from_le_bytes(
                                chunk[0..4].try_into().unwrap_or_default(),
                            ));
                            has_equip_data = true;
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
                            if ![23, 28, 36, 75, 81, 140, 141, 19, 1].contains(&id) {
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
    }

    // Fallback item name to submesh name if raw name is placeholder
    if (item_name == "Item" || item_name == "noname") && !submesh_name.is_empty() {
        item_name = submesh_name.clone();
    }

    let metadata = AttachmentEngineMetadataJson {
        resource_tag,
        type_id_hex: format!("{:08X}", item_type_id),
        is_direct_container,
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
        equipment_config: if has_equip_data { Some(equip) } else { None },
        weapon_config: if has_weapon_data { Some(weapon) } else { None },
        breakable_config: if has_breakable_data {
            Some(breakable)
        } else {
            None
        },
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

    if let Some(ref e) = parsed.equipment_config {
        if let Some(pr) = e.power_rating {
            item_sub.push((26, endian.u32_to_bytes(pr).to_vec()));
        }
        if let Some(cat) = e.equipment_category {
            item_sub.push((34, vec![cat]));
        }
        if let Some(av) = e.armor_value {
            item_sub.push((83, endian.u32_to_bytes(av).to_vec()));
        }
    }

    if let Some(ref w) = parsed.weapon_config {
        if let Some(t2h) = w.is_two_handed {
            item_sub.push((40, vec![if t2h { 1 } else { 0 }]));
        }
        if !w.impact_actions.is_empty() {
            item_sub.push((
                41,
                rebuild_item_trigger_actions(&w.impact_actions, endian).unwrap_or_default(),
            ));
        }
        if let Some(dmg) = w.damage_multiplier {
            item_sub.push((70, endian.f32_to_bytes(dmg).to_vec()));
        }
        if let Some(wf) = w.weapon_flags {
            if wf <= 0xFF {
                item_sub.push((71, vec![wf as u8]));
            } else {
                item_sub.push((71, endian.u32_to_bytes(wf).to_vec()));
            }
        }
        if let Some(spd) = w.speed_modifier {
            item_sub.push((72, endian.f32_to_bytes(spd).to_vec()));
        }
        if let Some(rng) = w.attack_range {
            item_sub.push((74, endian.f32_to_bytes(rng).to_vec()));
        }
    }

    if let Some(ref b) = parsed.breakable_config {
        if let Some(h) = b.base_health {
            item_sub.push((40, endian.f32_to_bytes(h).to_vec()));
        }
        if let Some(ref s) = b.break_sound {
            item_sub.push((41, build_embedded_string(s, endian)));
        }
        if let Some(ref s) = b.fuse_sound {
            item_sub.push((42, build_embedded_string(s, endian)));
        }
        if let Some(id) = b.physics_material_id {
            item_sub.push((43, endian.u32_to_bytes(id).to_vec()));
        }
        if let Some(sp) = b.spawn_particles {
            item_sub.push((44, vec![if sp { 1 } else { 0 }]));
        }
        if let Some(ref s) = b.impact_sound_alt {
            item_sub.push((45, build_embedded_string(s, endian)));
        }
        if !b.fuse_actions.is_empty() {
            item_sub.push((
                48,
                rebuild_item_trigger_actions(&b.fuse_actions, endian).unwrap_or_default(),
            ));
        }
        if !b.break_actions.is_empty() {
            item_sub.push((
                49,
                rebuild_item_trigger_actions(&b.break_actions, endian).unwrap_or_default(),
            ));
        }
        if let Some(ref hex) = b.debris_config_hex
            && let Ok(bytes) = hex::decode(hex)
        {
            item_sub.push((50, bytes));
        }
        if let Some(af) = b.auto_fuse {
            item_sub.push((51, vec![if af { 1 } else { 0 }]));
        }
    }

    // Automatically reconstruct empty structural framing containers (IDs 75 & 81) if missing
    if matches!(
        type_id,
        0x00462011 | 0x00462015 | 0x00462017 | 0x0046201B | 0x00462021
    ) && !item_sub.iter().any(|(id, _)| *id == 75)
    {
        item_sub.push((75, vec![1, 1, 0, 0]));
    }
    if type_id == 0x0046200B && !item_sub.iter().any(|(id, _)| *id == 81) {
        item_sub.push((81, vec![1, 1, 0, 0]));
    }

    for prop in parsed._engine_metadata.unmapped_properties {
        if let Ok(b) = hex::decode(&prop.hex) {
            item_sub.push((prop.id, b));
        }
    }

    item_sub.push((19, vec![0xFF, 0xFF, 0xFF, 0xFF]));
    item_sub.push((1, vec![0u8]));
    item_sub.sort_by_key(|&(id, _)| id);

    let item_resource_blob = build_typed_container_with_endian(type_id, &item_sub, endian);

    if parsed._engine_metadata.is_direct_container {
        return Ok(item_resource_blob);
    }

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

pub fn parse_socket_data(chunk: &[u8]) -> ItemSocketConfigJson {
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

pub fn build_socket_data(socket: &ItemSocketConfigJson) -> Vec<u8> {
    if socket.secondary_slot.is_some() {
        vec![1, 40, 0, 2, 40, 0, 43, 4, 1, 1, 0, 0, 0]
    } else {
        vec![1, 40, 0, 1, 40, 0, 1, 1, 0, 0]
    }
}
