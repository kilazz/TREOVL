use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use crate::engine::assets::character::CharacterModelBinding;
use crate::engine::assets::{
    build_chunk_from_elements_with_endian, build_typed_container_with_endian, parse_chunk_elements,
    parse_typed_container,
};
use crate::engine::common::{
    Endian, EntityHandleJson, ItemSocketConfigJson, build_socket_data, parse_entity_handle,
    parse_f32_safe, parse_socket_data, read_length_prefixed_string,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProjectileJson {
    pub _engine_metadata: ProjectileMetadataJson,
    pub projectile_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_handle: Option<EntityHandleJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firing_mode: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socket_config: Option<ItemSocketConfigJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_binding: Option<CharacterModelBinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hold_offset: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physics: Option<ProjectilePhysicsJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hazard_config: Option<HazardZoneConfigJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact_vfx: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_vfx: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destroy_on_impact: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub trigger_events: Vec<ProjectileTriggerEventJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub trigger_actions: Vec<ProjectileTriggerActionJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_spell_link: Option<SubSpellLinkJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ProjectileMetadataJson {
    pub type_id_hex: String,
    pub engine_class: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_flags_hex: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub unmapped_raw_blocks: Vec<RawProjectileBlock>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct HazardZoneConfigJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_per_sec: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tick_interval_sec: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inner_radius: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outer_radius: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact_force: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sound_cue_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_on_contact: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawn_particles: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProjectileTriggerEventJson {
    pub event_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_id_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_bone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_hex: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProjectileTriggerActionJson {
    pub action_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_id_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_sec: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_socket: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_hex: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SubSpellLinkJson {
    pub type_id_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spell_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_hex: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ProjectilePhysicsJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collision_type: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flight_speed: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_range: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hitbox_radius: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flight_duration_sec: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penetration_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub despawn_mode: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aoe_explosion_radius: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_scale: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pierces_targets: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homing_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gravity_scale: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RawProjectileBlock {
    pub id: u32,
    pub hex: String,
}

pub fn export_projectile_to_json(data: &[u8], _stem: &str) -> Result<String> {
    if data.len() < 5 {
        bail!("Data too short for TREProjectile container");
    }

    let (type_id, elements) =
        parse_typed_container(data).context("Failed to parse Projectile typed container")?;

    let mut projectile_name = String::from("Unnamed_Projectile");
    let mut resource_tag = None;
    let mut entity_handle = None;
    let mut is_enabled = None;
    let mut firing_mode = None;
    let mut socket_config = None;
    let mut model_binding = None;
    let mut hold_offset = None;
    let mut raw_flags_hex = None;
    let mut physics = ProjectilePhysicsJson::default();
    let mut has_physics_data = false;
    let mut hazard = HazardZoneConfigJson::default();
    let mut has_hazard_data = false;
    let mut impact_vfx = None;
    let mut secondary_vfx = None;
    let mut destroy_on_impact = None;
    let mut trigger_events = Vec::new();
    let mut trigger_actions = Vec::new();
    let mut sub_spell_link = None;
    let mut unmapped_raw_blocks = Vec::new();

    for (id, chunk) in &elements {
        match *id {
            20 => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    resource_tag = Some(s);
                }
            }
            21 => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    projectile_name = s;
                }
            }
            22 if chunk.len() >= 4 => {
                let mask = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
                entity_handle = parse_entity_handle(mask);
                raw_flags_hex = Some(format!("0x{:08X}", mask));
            }
            23 if !chunk.is_empty() => {
                is_enabled = Some(chunk[0] != 0);
            }
            28 if !chunk.is_empty() => {
                firing_mode = Some(chunk[0]);
            }
            29 => {
                socket_config = Some(parse_socket_data(chunk));
            }
            30 | 31 => {
                if let Ok((_, sub_elems)) = parse_chunk_elements(chunk) {
                    let mut obj_path = String::new();
                    let mut model_name = String::new();
                    for (sid, sdata) in sub_elems {
                        if sid == 20
                            && let Some(s) = read_length_prefixed_string(&sdata)
                        {
                            obj_path = s;
                        } else if sid == 21
                            && let Some(s) = read_length_prefixed_string(&sdata)
                        {
                            model_name = s;
                        }
                    }
                    if !obj_path.is_empty() {
                        model_binding = Some(CharacterModelBinding {
                            object_path: obj_path,
                            model_name,
                        });
                    }
                }
            }
            32 if chunk.len() >= 4 => {
                physics.collision_type = Some(u32::from_le_bytes(
                    chunk[0..4].try_into().unwrap_or_default(),
                ));
                has_physics_data = true;
            }
            33 if chunk.len() >= 4 => {
                hazard.impact_force = parse_f32_safe(chunk);
                has_hazard_data = true;
            }
            36 if chunk.len() >= 4 => {
                physics.damage_scale = parse_f32_safe(chunk);
                has_physics_data = true;
            }
            37 if chunk.len() >= 4 => {
                physics.penetration_count = Some(u32::from_le_bytes(
                    chunk[0..4].try_into().unwrap_or_default(),
                ));
                has_physics_data = true;
            }
            38 if chunk.len() >= 4 => {
                physics.flight_speed = parse_f32_safe(chunk);
                has_physics_data = true;
            }
            39 if !chunk.is_empty() => {
                physics.despawn_mode = Some(chunk[0] as u32);
                has_physics_data = true;
            }
            41 | 52 => {
                let mut acts = parse_projectile_trigger_actions(chunk);
                trigger_actions.append(&mut acts);
            }
            42 if chunk.len() >= 4 => {
                physics.flight_duration_sec = parse_f32_safe(chunk);
                has_physics_data = true;
            }
            43 => {
                if chunk.len() == 1 {
                    hazard.is_active = Some(chunk[0] != 0);
                    has_hazard_data = true;
                } else if chunk.len() >= 4 {
                    physics.hitbox_radius = parse_f32_safe(chunk);
                    has_physics_data = true;
                }
            }
            44 => {
                if let Some(s) = extract_string_from_raw_block(chunk) {
                    impact_vfx = Some(s);
                }
            }
            45 => {
                if let Some(s) = extract_string_from_raw_block(chunk) {
                    secondary_vfx = Some(s);
                }
            }
            46 if chunk.len() >= 4 => {
                physics.gravity_scale = parse_f32_safe(chunk);
                has_physics_data = true;
            }
            47 => {
                if chunk.len() >= 4 {
                    physics.max_range = parse_f32_from_variable_chunk(chunk);
                    has_physics_data = true;
                }
            }
            48 if !chunk.is_empty() => {
                physics.pierces_targets = Some(chunk[0] != 0);
                has_physics_data = true;
            }
            49 if !chunk.is_empty() => {
                physics.homing_enabled = Some(chunk[0] != 0);
                has_physics_data = true;
            }
            50 => {
                trigger_events = parse_projectile_trigger_events(chunk);
            }
            53 => {
                sub_spell_link = parse_sub_spell_link(chunk);
            }
            54 if chunk.len() >= 4 => {
                physics.aoe_explosion_radius = parse_f32_safe(chunk);
                has_physics_data = true;
            }
            56 if !chunk.is_empty() => {
                destroy_on_impact = Some(chunk[0] != 0);
            }
            201 if chunk.len() >= 4 => {
                hazard.sound_cue_id = Some(u32::from_le_bytes(
                    chunk[0..4].try_into().unwrap_or_default(),
                ));
                has_hazard_data = true;
            }
            202 if !chunk.is_empty() => {
                hazard.trigger_on_contact = Some(chunk[0] != 0);
                has_hazard_data = true;
            }
            203 if !chunk.is_empty() => {
                hazard.spawn_particles = Some(chunk[0] != 0);
                has_hazard_data = true;
            }
            250 if chunk.len() >= 4 => {
                hazard.damage_per_sec = parse_f32_safe(chunk);
                has_hazard_data = true;
            }
            251 if chunk.len() >= 4 => {
                hazard.tick_interval_sec = parse_f32_safe(chunk);
                has_hazard_data = true;
            }
            253 if chunk.len() >= 4 => {
                hazard.inner_radius = parse_f32_safe(chunk);
                has_hazard_data = true;
            }
            254 => {
                hazard.outer_radius = parse_f32_from_variable_chunk(chunk);
                has_hazard_data = true;
            }
            300 => {
                hold_offset = parse_hold_offset(chunk);
            }
            19 | 1 | 70 | 71 | 200 | 301 => {}
            _ => {
                unmapped_raw_blocks.push(RawProjectileBlock {
                    id: *id,
                    hex: hex::encode_upper(chunk),
                });
            }
        }
    }

    let engine_class = match type_id {
        0x00463063 => "TRELure".to_string(),
        0x00463065 => "TRESpell".to_string(),
        0x00463028 => "TREDamageDealer".to_string(),
        _ => "TREProjectile".to_string(),
    };

    let metadata = ProjectileMetadataJson {
        type_id_hex: format!("{:08X}", type_id),
        engine_class,
        raw_flags_hex,
        unmapped_raw_blocks,
    };

    let proj_json = ProjectileJson {
        _engine_metadata: metadata,
        projectile_name,
        resource_tag,
        entity_handle,
        is_enabled,
        firing_mode,
        socket_config,
        model_binding,
        hold_offset,
        physics: if has_physics_data {
            Some(physics)
        } else {
            None
        },
        hazard_config: if has_hazard_data { Some(hazard) } else { None },
        impact_vfx,
        secondary_vfx,
        destroy_on_impact,
        trigger_events,
        trigger_actions,
        sub_spell_link,
    };

    serde_json::to_string_pretty(&proj_json).map_err(|e| anyhow::anyhow!(e))
}

pub fn import_projectile_from_json(json_str: &str, endian: Endian) -> Result<Vec<u8>> {
    let parsed: ProjectileJson =
        serde_json::from_str(json_str).context("Syntax error in Projectile JSON format")?;

    let clean_type_id = parsed
        ._engine_metadata
        .type_id_hex
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X");
    let type_id = u32::from_str_radix(clean_type_id, 16).unwrap_or(0x00463006);

    let mut elements = Vec::new();

    for block in &parsed._engine_metadata.unmapped_raw_blocks {
        if let Ok(chunk_bytes) = hex::decode(&block.hex) {
            elements.push((block.id, chunk_bytes));
        }
    }

    if let Some(ref tag) = parsed.resource_tag {
        elements.push((20, endian.write_length_prefixed_string(tag)));
    }
    elements.push((
        21,
        endian.write_length_prefixed_string(&parsed.projectile_name),
    ));

    let mask = if let Some(ref handle) = parsed.entity_handle {
        let tag_byte = handle
            .domain_tag
            .as_bytes()
            .first()
            .copied()
            .unwrap_or(b'M');
        (tag_byte as u32) << 24 | (handle.uid & 0x00FF_FFFF)
    } else if let Some(ref raw_h) = parsed._engine_metadata.raw_flags_hex {
        let clean = raw_h
            .trim()
            .trim_start_matches("0x")
            .trim_start_matches("0X");
        u32::from_str_radix(clean, 16).unwrap_or(0x0580000C)
    } else {
        0x0580000C
    };
    elements.push((22, endian.u32_to_bytes(mask).to_vec()));

    let en = parsed.is_enabled.unwrap_or(true);
    elements.push((23, vec![if en { 1 } else { 0 }]));

    if let Some(fmode) = parsed.firing_mode {
        elements.push((28, vec![fmode]));
    } else {
        elements.push((28, vec![0u8]));
    }

    if let Some(ref sock) = parsed.socket_config {
        elements.push((29, build_socket_data(sock)));
    } else {
        elements.push((29, vec![1, 40, 0, 2, 40, 0, 43, 4, 1, 1, 0, 0, 0]));
    }

    if let Some(ref mb) = parsed.model_binding {
        let sub_elems = vec![
            (20, endian.write_length_prefixed_string(&mb.object_path)),
            (21, endian.write_length_prefixed_string(&mb.model_name)),
        ];
        let mb_blob = build_chunk_from_elements_with_endian(false, &sub_elems, endian);
        let mb_id = if type_id == 0x00463028 { 31 } else { 30 };
        elements.push((mb_id, mb_blob));
    } else {
        let mb_id = if type_id == 0x00463028 { 31 } else { 30 };
        elements.push((mb_id, vec![0u8]));
    }

    if let Some(ref phys) = parsed.physics {
        if let Some(col) = phys.collision_type {
            elements.push((32, endian.u32_to_bytes(col).to_vec()));
        }
        if let Some(dmg) = phys.damage_scale {
            elements.push((36, endian.f32_to_bytes(dmg).to_vec()));
        }
        if let Some(pen) = phys.penetration_count {
            elements.push((37, endian.u32_to_bytes(pen).to_vec()));
        } else {
            elements.push((37, vec![0, 0, 0, 0]));
        }
        if let Some(spd) = phys.flight_speed {
            elements.push((38, endian.f32_to_bytes(spd).to_vec()));
        }
        if let Some(desp) = phys.despawn_mode {
            elements.push((39, vec![desp as u8]));
        }
        if let Some(dur) = phys.flight_duration_sec {
            elements.push((42, endian.f32_to_bytes(dur).to_vec()));
        }
        if let Some(rad) = phys.hitbox_radius {
            elements.push((43, endian.f32_to_bytes(rad).to_vec()));
        }
        if let Some(grav) = phys.gravity_scale {
            elements.push((46, endian.f32_to_bytes(grav).to_vec()));
        }
        if let Some(range) = phys.max_range {
            let mut blk47 = vec![1u8, 35, 0];
            let _ = endian.write_f32(&mut blk47, range);
            elements.push((47, blk47));
        }
        if let Some(pierce) = phys.pierces_targets {
            elements.push((48, vec![if pierce { 1 } else { 0 }]));
        }
        if let Some(homing) = phys.homing_enabled {
            elements.push((49, vec![if homing { 1 } else { 0 }]));
        }
        if let Some(aoe) = phys.aoe_explosion_radius {
            elements.push((54, endian.f32_to_bytes(aoe).to_vec()));
        }
    }

    if let Some(ref haz) = parsed.hazard_config {
        if let Some(imp) = haz.impact_force {
            elements.push((33, endian.f32_to_bytes(imp).to_vec()));
        }
        if let Some(act) = haz.is_active {
            elements.push((43, vec![if act { 1 } else { 0 }]));
        }
        if let Some(cue) = haz.sound_cue_id {
            elements.push((201, endian.u32_to_bytes(cue).to_vec()));
        } else if !elements.iter().any(|(id, _)| *id == 201) {
            elements.push((201, vec![1, 0x16, 0, 0]));
        }
        if let Some(trig) = haz.trigger_on_contact {
            elements.push((202, vec![if trig { 1 } else { 0 }, 1, 0, 0]));
        }
        if let Some(part) = haz.spawn_particles {
            elements.push((203, vec![if part { 1 } else { 0 }, 1, 0, 0]));
        }
        if let Some(dps) = haz.damage_per_sec {
            elements.push((250, endian.f32_to_bytes(dps).to_vec()));
        }
        if let Some(interval) = haz.tick_interval_sec {
            elements.push((251, endian.f32_to_bytes(interval).to_vec()));
        }
        if let Some(inner) = haz.inner_radius {
            elements.push((253, endian.f32_to_bytes(inner).to_vec()));
        }
        if let Some(outer) = haz.outer_radius {
            let mut blk254 = vec![1u8, 0x1E, 0];
            let _ = endian.write_f32(&mut blk254, outer);
            elements.push((254, blk254));
        }
    }

    if let Some(ho) = parsed.hold_offset {
        let mut blk300 = vec![1u8, 20, 0];
        let _ = endian.write_f32(&mut blk300, ho[0]);
        let _ = endian.write_f32(&mut blk300, ho[1]);
        let _ = endian.write_f32(&mut blk300, ho[2]);
        elements.push((300, blk300));
    }

    if let Some(ref vfx) = parsed.impact_vfx {
        let s_bytes = endian.write_length_prefixed_string(vfx);
        let mut blk44 = vec![1u8, 20, 0, 2, 19, 0, 20, 4, 0xEC, 0, 0, 0];
        blk44.extend_from_slice(&s_bytes);
        elements.push((44, blk44));
    } else if type_id != 0x00463028 {
        elements.push((44, vec![1u8, 20, 0, 0]));
    }

    if let Some(ref svfx) = parsed.secondary_vfx {
        let s_bytes = endian.write_length_prefixed_string(svfx);
        let mut blk45 = vec![1u8, 20, 0];
        blk45.extend_from_slice(&s_bytes);
        elements.push((45, blk45));
    } else if type_id != 0x00463028 {
        elements.push((45, vec![1u8, 20, 0, 0]));
    }

    if !parsed.trigger_events.is_empty() {
        elements.push((
            50,
            rebuild_projectile_trigger_events(&parsed.trigger_events, endian)?,
        ));
    } else if type_id != 0x00463028 {
        elements.push((50, vec![1, 1, 0, 0]));
    }

    if !parsed.trigger_actions.is_empty() {
        let act_blob = rebuild_projectile_trigger_actions(&parsed.trigger_actions, endian)?;
        let act_id = if type_id == 0x00463028 { 41 } else { 52 };
        elements.push((act_id, act_blob));
    } else if type_id != 0x00463028 {
        elements.push((52, vec![1, 1, 0, 0]));
    }

    if let Some(ref sub_spell) = parsed.sub_spell_link {
        elements.push((53, rebuild_sub_spell_link(sub_spell, endian)?));
    } else if type_id != 0x00463028 {
        elements.push((53, vec![1, 1, 0, 0]));
    }

    if let Some(destr) = parsed.destroy_on_impact {
        elements.push((56, vec![if destr { 1 } else { 0 }]));
    }

    if type_id == 0x00463028 {
        if !elements.iter().any(|(id, _)| *id == 70) {
            elements.push((
                70,
                vec![
                    8, 0x28, 0, 0x29, 0x1C, 0x2A, 0x38, 0x2B, 0x54, 0x2C, 0x70, 0x2D, 0x8C, 0x2E,
                    0xA8, 0x2F, 0xC4,
                ],
            ));
        }
        if !elements.iter().any(|(id, _)| *id == 71) {
            elements.push((
                71,
                vec![
                    13, 0x29, 0, 0x2A, 7, 0x2B, 8, 0x2C, 9, 0x2D, 10, 0x64, 11, 0x65, 24, 0x66, 37,
                    0x67, 50, 0x68, 63, 0x69, 76, 0x6A, 89, 0x6B, 102, 1, 10, 0, 0, 0, 0, 0, 0, 0,
                    0, 0,
                ],
            ));
        }
        if !elements.iter().any(|(id, _)| *id == 200) {
            elements.push((
                200,
                vec![
                    7, 0x0C, 0, 0x0D, 4, 0x0E, 5, 0x10, 6, 0x13, 7, 0x16, 8, 0x17, 9, 1, 0x0A, 0,
                    0xBF, 0, 0, 0, 0, 0, 0,
                ],
            ));
        }
        if !elements.iter().any(|(id, _)| *id == 301) {
            elements.push((301, vec![0u8]));
        }
    }

    elements.push((19, vec![0xFF, 0xFF, 0xFF, 0xFF]));
    elements.push((1, vec![0u8]));

    elements.sort_by_key(|&(id, _)| id);
    Ok(build_typed_container_with_endian(
        type_id, &elements, endian,
    ))
}

fn parse_hold_offset(chunk: &[u8]) -> Option<[f32; 3]> {
    if chunk.len() >= 15 && chunk[0] == 1 && chunk[1] == 20 {
        let mut cur = Cursor::new(&chunk[3..15]);
        let x = cur.read_f32::<LittleEndian>().ok()?;
        let y = cur.read_f32::<LittleEndian>().ok()?;
        let z = cur.read_f32::<LittleEndian>().ok()?;
        if x.is_finite() && y.is_finite() && z.is_finite() {
            return Some([x, y, z]);
        }
    } else if chunk.len() >= 12 {
        let mut cur = Cursor::new(&chunk[0..12]);
        let x = cur.read_f32::<LittleEndian>().ok()?;
        let y = cur.read_f32::<LittleEndian>().ok()?;
        let z = cur.read_f32::<LittleEndian>().ok()?;
        if x.is_finite() && y.is_finite() && z.is_finite() {
            return Some([x, y, z]);
        }
    }
    None
}

fn parse_f32_from_variable_chunk(chunk: &[u8]) -> Option<f32> {
    for window in chunk.windows(4) {
        let val = f32::from_le_bytes(window.try_into().unwrap_or_default());
        if val.is_finite() && !val.is_nan() && (1.0..=500.0).contains(&val) {
            return Some(val);
        }
    }
    None
}

fn extract_string_from_raw_block(chunk: &[u8]) -> Option<String> {
    let mut i = 0;
    while i + 4 <= chunk.len() {
        if let Some(s) = read_length_prefixed_string(&chunk[i..]) {
            if s.len() >= 3 && !s.starts_with('[') {
                return Some(s);
            }
            i += 4 + s.len();
        } else {
            i += 1;
        }
    }
    None
}

fn extract_first_valid_string(data: &[u8]) -> Option<String> {
    if let Some(s) = read_length_prefixed_string(data)
        && s.len() >= 2
        && (!s.starts_with('[') || s.contains("Grating") || s.contains("Fire"))
    {
        return Some(s);
    }

    if let Ok((_, elements)) = parse_chunk_elements(data) {
        for (_, chunk) in elements {
            if let Some(s) = extract_first_valid_string(&chunk) {
                return Some(s);
            }
        }
    }

    if let Ok((_, elements)) = parse_typed_container(data) {
        for (_, chunk) in elements {
            if let Some(s) = extract_first_valid_string(&chunk) {
                return Some(s);
            }
        }
    }

    let mut i = 0;
    while i + 4 <= data.len() {
        if let Some(s) = read_length_prefixed_string(&data[i..]) {
            if s.len() >= 3
                && (!s.starts_with('[') || s.contains("Grating") || s.contains("Fire"))
                && s.chars().all(|c| c.is_ascii_graphic() || c == ' ')
            {
                return Some(s);
            }
            i += 4 + s.len();
        } else {
            i += 1;
        }
    }

    None
}

fn parse_projectile_trigger_events(chunk: &[u8]) -> Vec<ProjectileTriggerEventJson> {
    let mut events = Vec::new();

    if let Ok((_, sub_parts)) = parse_chunk_elements(chunk) {
        for (_, pdata) in sub_parts {
            if let Ok((type_id, fields)) = parse_typed_container(&pdata) {
                let mut event_name = String::new();
                let mut target_bone = None;
                let mut radius = None;

                for (fid, fdata) in fields {
                    if let Some(s) = extract_first_valid_string(&fdata) {
                        if s.ends_with("_item")
                            || s.contains("hand")
                            || s.contains("Head")
                            || s.contains("R_")
                        {
                            target_bone = Some(s);
                        } else if event_name.is_empty() {
                            event_name = s;
                        }
                    } else if fid == 28 && fdata.len() == 4 && radius.is_none() {
                        radius = parse_f32_safe(&fdata);
                    }
                }

                if !event_name.is_empty() {
                    events.push(ProjectileTriggerEventJson {
                        event_name,
                        type_id_hex: Some(format!("{:08X}", type_id)),
                        radius,
                        target_bone,
                        raw_hex: None,
                    });
                }
            }
        }
    }

    events
}

fn rebuild_projectile_trigger_events(
    events: &[ProjectileTriggerEventJson],
    endian: Endian,
) -> Result<Vec<u8>> {
    let mut sub_chunks = Vec::new();

    for (i, ev) in events.iter().enumerate() {
        if let Some(ref raw_h) = ev.raw_hex
            && let Ok(raw_b) = hex::decode(raw_h)
        {
            sub_chunks.push((i as u32, raw_b));
            continue;
        }

        let type_id = ev
            .type_id_hex
            .as_deref()
            .and_then(|h| u32::from_str_radix(h.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0x00460758);

        let mut fields = Vec::new();
        fields.push((20, endian.write_length_prefixed_string(&ev.event_name)));
        if let Some(r) = ev.radius {
            fields.push((28, endian.f32_to_bytes(r).to_vec()));
        }
        if let Some(ref bone) = ev.target_bone {
            fields.push((21, endian.write_length_prefixed_string(bone)));
        }

        let ev_blob = build_typed_container_with_endian(type_id, &fields, endian);
        sub_chunks.push((i as u32, ev_blob));
    }

    Ok(build_chunk_from_elements_with_endian(
        true,
        &sub_chunks,
        endian,
    ))
}

fn parse_projectile_trigger_actions(chunk: &[u8]) -> Vec<ProjectileTriggerActionJson> {
    let mut actions = Vec::new();

    if let Ok((_, sub_parts)) = parse_chunk_elements(chunk) {
        for (_, pdata) in sub_parts {
            let (type_id, fields) = if let Ok((t, f)) = parse_typed_container(&pdata) {
                (t, f)
            } else if let Ok((_, f)) = parse_chunk_elements(&pdata) {
                (0x00460756, f)
            } else {
                (0x00460756, vec![(0, pdata.clone())])
            };

            let mut action_name = String::new();
            let mut delay_sec = None;
            let mut target_socket = None;
            let mut is_active = None;

            for (fid, fdata) in fields {
                match fid {
                    25 if !fdata.is_empty() => is_active = Some(fdata[0] != 0),
                    26 | 28 if fdata.len() >= 4 => delay_sec = parse_f32_safe(&fdata),
                    19 if fdata.len() >= 4 && delay_sec.is_none() => {
                        delay_sec = parse_f32_safe(&fdata);
                    }
                    40 | 20 | 21 => {
                        if let Some(s) = extract_first_valid_string(&fdata)
                            && (action_name.is_empty() || action_name == "TriggerAction")
                        {
                            action_name = s;
                        }
                    }
                    41 => {
                        if let Some(s) = extract_first_valid_string(&fdata) {
                            action_name = s;
                        }
                    }
                    43 | 22 | 23 => {
                        if let Some(s) = extract_first_valid_string(&fdata) {
                            target_socket = Some(s);
                        }
                    }
                    _ => {
                        if action_name.is_empty()
                            && let Some(s) = extract_first_valid_string(&fdata)
                        {
                            action_name = s;
                        }
                    }
                }
            }

            if action_name.is_empty() {
                if let Some(s) = extract_first_valid_string(&pdata) {
                    action_name = s;
                } else {
                    action_name = "TriggerAction".into();
                }
            }

            actions.push(ProjectileTriggerActionJson {
                action_name,
                type_id_hex: Some(format!("{:08X}", type_id)),
                delay_sec,
                target_socket,
                is_active,
                raw_hex: None,
            });
        }
    } else if let Ok((type_id, fields)) = parse_typed_container(chunk) {
        let mut action_name = String::new();
        let mut delay_sec = None;
        let mut target_socket = None;
        let mut is_active = None;

        for (fid, fdata) in fields {
            match fid {
                25 if !fdata.is_empty() => is_active = Some(fdata[0] != 0),
                26 | 28 if fdata.len() >= 4 => delay_sec = parse_f32_safe(&fdata),
                19 if fdata.len() >= 4 && delay_sec.is_none() => {
                    delay_sec = parse_f32_safe(&fdata);
                }
                40 | 20 | 21 | 41 => {
                    if let Some(s) = extract_first_valid_string(&fdata)
                        && (action_name.is_empty() || action_name == "TriggerAction")
                    {
                        action_name = s;
                    }
                }
                43 | 22 | 23 => {
                    if let Some(s) = extract_first_valid_string(&fdata) {
                        target_socket = Some(s);
                    }
                }
                _ => {}
            }
        }

        if !action_name.is_empty() {
            actions.push(ProjectileTriggerActionJson {
                action_name,
                type_id_hex: Some(format!("{:08X}", type_id)),
                delay_sec,
                target_socket,
                is_active,
                raw_hex: None,
            });
        }
    }

    actions
}

fn rebuild_projectile_trigger_actions(
    actions: &[ProjectileTriggerActionJson],
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
            .unwrap_or(0x00460756);

        let mut fields = Vec::new();
        fields.push((
            25,
            vec![if act.is_active.unwrap_or(true) { 1 } else { 0 }, 0, 0, 0],
        ));

        let delay = act.delay_sec.unwrap_or(0.01);
        fields.push((26, endian.f32_to_bytes(delay).to_vec()));

        let name_sub = vec![(20, endian.write_length_prefixed_string(&act.action_name))];
        let name_blob = build_chunk_from_elements_with_endian(false, &name_sub, endian);
        fields.push((40, name_blob));

        fields.push((41, vec![1, 0, 0, 0, 0, 0, 0, 0]));

        if let Some(ref sock) = act.target_socket {
            let sock_sub = vec![(20, endian.write_length_prefixed_string(sock))];
            let sock_blob = build_chunk_from_elements_with_endian(false, &sock_sub, endian);
            fields.push((43, sock_blob));
        }

        let act_blob = build_typed_container_with_endian(type_id, &fields, endian);
        sub_chunks.push((i as u32, act_blob));
    }

    Ok(build_chunk_from_elements_with_endian(
        true,
        &sub_chunks,
        endian,
    ))
}

fn parse_sub_spell_link(chunk: &[u8]) -> Option<SubSpellLinkJson> {
    if chunk.is_empty() || chunk == [1, 1, 0, 0] || chunk == [0] {
        return None;
    }

    if let Ok((_, elements)) = parse_chunk_elements(chunk) {
        for (id, data) in elements {
            if id == 0
                && let Ok((type_id, fields)) = parse_typed_container(&data)
            {
                let mut spell_id = None;
                for (fid, fdata) in fields {
                    if fid == 20 && fdata.len() >= 4 {
                        spell_id = Some(u32::from_le_bytes(
                            fdata[0..4].try_into().unwrap_or_default(),
                        ));
                    }
                }
                return Some(SubSpellLinkJson {
                    type_id_hex: format!("{:08X}", type_id),
                    spell_id,
                    payload_hex: None,
                });
            }
        }
    }

    if chunk.len() >= 4 && !chunk.starts_with(b"\x01\x01\x00") {
        let type_id = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
        return Some(SubSpellLinkJson {
            type_id_hex: format!("{:08X}", type_id),
            spell_id: None,
            payload_hex: Some(hex::encode_upper(chunk)),
        });
    }

    Some(SubSpellLinkJson {
        type_id_hex: "0046B013".into(),
        spell_id: None,
        payload_hex: Some(hex::encode_upper(chunk)),
    })
}

fn rebuild_sub_spell_link(link: &SubSpellLinkJson, endian: Endian) -> Result<Vec<u8>> {
    if let Some(ref h) = link.payload_hex
        && let Ok(b) = hex::decode(h)
    {
        return Ok(b);
    }

    let type_id =
        u32::from_str_radix(link.type_id_hex.trim_start_matches("0x"), 16).unwrap_or(0x0046B013);

    let mut fields = Vec::new();
    if let Some(id) = link.spell_id {
        fields.push((20, endian.u32_to_bytes(id).to_vec()));
    }

    let inner_blob = build_typed_container_with_endian(type_id, &fields, endian);
    let root_sub = vec![(0, inner_blob)];

    Ok(build_chunk_from_elements_with_endian(
        true, &root_sub, endian,
    ))
}
