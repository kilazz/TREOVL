use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::engine::assets::attachment::{
    ItemSocketConfigJson, build_socket_data, parse_socket_data,
};
use crate::engine::assets::character::CharacterModelBinding;
use crate::engine::assets::{
    build_chunk_from_elements_with_endian, build_typed_container_with_endian, parse_chunk_elements,
    parse_typed_container,
};
use crate::engine::common::{Endian, read_length_prefixed_string};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProjectileJson {
    pub _engine_metadata: ProjectileMetadataJson,
    pub projectile_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firing_mode: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socket_config: Option<ItemSocketConfigJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_binding: Option<CharacterModelBinding>,
    pub physics: ProjectilePhysicsJson,
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
    let mut is_enabled = None;
    let mut firing_mode = None;
    let mut socket_config = None;
    let mut model_binding = None;
    let mut raw_flags_hex = None;
    let mut physics = ProjectilePhysicsJson::default();
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
            30 => {
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
            }
            36 if chunk.len() >= 4 => {
                physics.damage_scale = parse_f32_safe(chunk);
            }
            37 if chunk.len() >= 4 => {
                physics.penetration_count = Some(u32::from_le_bytes(
                    chunk[0..4].try_into().unwrap_or_default(),
                ));
            }
            38 if chunk.len() >= 4 => {
                physics.flight_speed = parse_f32_safe(chunk);
            }
            39 if !chunk.is_empty() => {
                physics.despawn_mode = Some(chunk[0] as u32);
            }
            42 if chunk.len() >= 4 => {
                physics.flight_duration_sec = parse_f32_safe(chunk);
            }
            43 if chunk.len() >= 4 => {
                physics.hitbox_radius = parse_f32_safe(chunk);
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
            }
            47 => {
                if chunk.len() >= 4 {
                    physics.max_range = parse_f32_from_variable_chunk(chunk);
                }
            }
            48 if !chunk.is_empty() => {
                physics.pierces_targets = Some(chunk[0] != 0);
            }
            49 if !chunk.is_empty() => {
                physics.homing_enabled = Some(chunk[0] != 0);
            }
            50 => {
                trigger_events = parse_projectile_trigger_events(chunk);
            }
            52 => {
                trigger_actions = parse_projectile_trigger_actions(chunk);
            }
            53 => {
                sub_spell_link = parse_sub_spell_link(chunk);
            }
            54 if chunk.len() >= 4 => {
                physics.aoe_explosion_radius = parse_f32_safe(chunk);
            }
            56 if !chunk.is_empty() => {
                destroy_on_impact = Some(chunk[0] != 0);
            }
            19 | 1 => {}
            _ => {
                unmapped_raw_blocks.push(RawProjectileBlock {
                    id: *id,
                    hex: hex::encode_upper(chunk),
                });
            }
        }
    }

    let metadata = ProjectileMetadataJson {
        type_id_hex: format!("{:08X}", type_id),
        engine_class: "TREProjectile".to_string(),
        raw_flags_hex,
        unmapped_raw_blocks,
    };

    let proj_json = ProjectileJson {
        _engine_metadata: metadata,
        projectile_name,
        resource_tag,
        is_enabled,
        firing_mode,
        socket_config,
        model_binding,
        physics,
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

    let mask = if let Some(ref raw_h) = parsed._engine_metadata.raw_flags_hex {
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
        elements.push((30, mb_blob));
    } else {
        elements.push((30, vec![0u8]));
    }

    if let Some(col) = parsed.physics.collision_type {
        elements.push((32, endian.u32_to_bytes(col).to_vec()));
    }
    if let Some(dmg) = parsed.physics.damage_scale {
        elements.push((36, endian.f32_to_bytes(dmg).to_vec()));
    }
    if let Some(pen) = parsed.physics.penetration_count {
        elements.push((37, endian.u32_to_bytes(pen).to_vec()));
    } else {
        elements.push((37, vec![0, 0, 0, 0]));
    }
    if let Some(spd) = parsed.physics.flight_speed {
        elements.push((38, endian.f32_to_bytes(spd).to_vec()));
    }
    if let Some(desp) = parsed.physics.despawn_mode {
        elements.push((39, vec![desp as u8]));
    }
    if let Some(dur) = parsed.physics.flight_duration_sec {
        elements.push((42, endian.f32_to_bytes(dur).to_vec()));
    }
    if let Some(rad) = parsed.physics.hitbox_radius {
        elements.push((43, endian.f32_to_bytes(rad).to_vec()));
    }

    if let Some(ref vfx) = parsed.impact_vfx {
        let s_bytes = endian.write_length_prefixed_string(vfx);
        let mut blk44 = vec![1u8, 20, 0, 2, 19, 0, 20, 4, 0xEC, 0, 0, 0];
        blk44.extend_from_slice(&s_bytes);
        elements.push((44, blk44));
    } else {
        elements.push((44, vec![1u8, 20, 0, 0]));
    }

    if let Some(ref svfx) = parsed.secondary_vfx {
        let s_bytes = endian.write_length_prefixed_string(svfx);
        let mut blk45 = vec![1u8, 20, 0];
        blk45.extend_from_slice(&s_bytes);
        elements.push((45, blk45));
    } else {
        elements.push((45, vec![1u8, 20, 0, 0]));
    }

    if let Some(grav) = parsed.physics.gravity_scale {
        elements.push((46, endian.f32_to_bytes(grav).to_vec()));
    }
    if let Some(range) = parsed.physics.max_range {
        let mut blk47 = vec![1u8, 35, 0];
        let _ = endian.write_f32(&mut blk47, range);
        elements.push((47, blk47));
    }
    if let Some(pierce) = parsed.physics.pierces_targets {
        elements.push((48, vec![if pierce { 1 } else { 0 }]));
    }
    if let Some(homing) = parsed.physics.homing_enabled {
        elements.push((49, vec![if homing { 1 } else { 0 }]));
    }

    if !parsed.trigger_events.is_empty() {
        elements.push((
            50,
            rebuild_projectile_trigger_events(&parsed.trigger_events, endian)?,
        ));
    } else {
        elements.push((50, vec![1, 1, 0, 0]));
    }

    if !parsed.trigger_actions.is_empty() {
        elements.push((
            52,
            rebuild_projectile_trigger_actions(&parsed.trigger_actions, endian)?,
        ));
    } else {
        elements.push((52, vec![1, 1, 0, 0]));
    }

    if let Some(ref sub_spell) = parsed.sub_spell_link {
        elements.push((53, rebuild_sub_spell_link(sub_spell, endian)?));
    } else {
        elements.push((53, vec![1, 1, 0, 0]));
    }

    if let Some(aoe) = parsed.physics.aoe_explosion_radius {
        elements.push((54, endian.f32_to_bytes(aoe).to_vec()));
    }
    if let Some(destr) = parsed.destroy_on_impact {
        elements.push((56, vec![if destr { 1 } else { 0 }]));
    }

    elements.push((19, vec![0xFF, 0xFF, 0xFF, 0xFF]));
    elements.push((1, vec![0u8]));

    elements.sort_by_key(|&(id, _)| id);
    Ok(build_typed_container_with_endian(
        type_id, &elements, endian,
    ))
}

fn parse_f32_safe(chunk: &[u8]) -> Option<f32> {
    if chunk.len() >= 4 {
        let val = f32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
        if val.is_finite() && !val.is_nan() && val.abs() >= 1e-4 && val.abs() <= 500_000.0 {
            return Some(val);
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
        && !s.starts_with('[')
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
                && !s.starts_with('[')
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
            let fields = if let Ok((_, f)) = parse_typed_container(&pdata) {
                f
            } else if let Ok((_, f)) = parse_chunk_elements(&pdata) {
                f
            } else {
                vec![(0, pdata.clone())]
            };

            let mut action_name = String::new();
            let mut delay_sec = None;
            let mut target_socket = None;
            let mut is_active = None;

            for (fid, fdata) in fields {
                match fid {
                    25 if !fdata.is_empty() => is_active = Some(fdata[0] != 0),
                    26 if fdata.len() >= 4 => delay_sec = parse_f32_safe(&fdata),
                    40 | 20 | 21 => {
                        if let Some(s) = extract_first_valid_string(&fdata)
                            && action_name.is_empty()
                        {
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
                type_id_hex: Some("00460756".to_string()),
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
