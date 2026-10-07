use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::breakable::BreakablePropsConfigJson;
use crate::engine::assets::{
    build_chunk_from_elements_with_endian, build_typed_container_with_endian, parse_chunk_elements,
    parse_typed_container,
};
use crate::engine::common::{Endian, read_length_prefixed_string};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CharacterActorJson {
    pub _engine_metadata: CharacterEngineMetadataJson,
    pub character_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_baby: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_binding: Option<CharacterModelBinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breakable_config: Option<BreakablePropsConfigJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collapse_target_model: Option<String>,
    pub actor_flags: Option<CharacterFlagsJson>,
    pub combat_attributes: Option<CharacterAttributesJson>,
    pub timing_parameters: Option<CharacterCombatTimingsJson>,
    pub knockback_parameters: Option<CharacterKnockbackJson>,
    pub state_and_rewards: Option<CharacterStateAndRewardsJson>,
    pub morph_parameters: Option<CharacterMorphParamsJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ragdoll_config: Option<ActorRagdollConfigJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collision_filter: Option<ActorCollisionFilterJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub equipment: Option<CharacterEquipmentJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facefx_actor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedded_facefx_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifeforce_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lua_script_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedded_lua_script: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub animation_states: Vec<CharacterAnimStateJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub ai_behaviors: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub ai_actions: Vec<ActorAiActionJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub socket_offsets: Vec<ActorSocketOffsetJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub transformations: Vec<CharacterTransformationJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub effect_receptors: Vec<CharacterAttachmentReceptorJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub minion_grapple_bones: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CharacterEngineMetadataJson {
    pub type_id_hex: String,
    pub engine_class: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_flags_hex: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub unmapped_raw_blocks: Vec<RawCharacterBlock>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CharacterFlagsJson {
    pub casts_dynamic_shadows: bool,
    pub can_be_targeted: bool,
    pub ragdoll_on_death: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_civilian: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CharacterAttributesJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub faction_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archetype_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threat_level: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_elite_or_boss: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_active_on_spawn: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_be_interrupted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_swim: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_foot_ik: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forward_aim_vector: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub move_speed_scale: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_speed_scale: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub perception_radius: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collision_radius: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engagement_distance: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert_distance: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mass: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_health: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hit_reaction_force: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_awareness_range: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jump_force: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggro_range: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_multiplier: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explosion_damage: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision_cone_degrees: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_attackers_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggro_decay_rate: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_chase_distance: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behavior_state_flags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub melee_attack_range: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranged_attack_range: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranged_cooldown_sec: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CharacterEquipmentJson {
    pub item_name: String,
    pub mount_socket: String,
    pub primary_slot: u32,
    pub support_slot: u32,
    pub equipped_slot: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CharacterCombatTimingsJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stumble_recovery_time_sec: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack_windup_time_sec: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack_cooldown_time_sec: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_window_time_sec: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invulnerability_time_sec: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hit_recovery_cooldown_sec: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ActorRagdollConfigJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ragdoll_mode: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blend_time_sec: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ragdoll_flags: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact_reaction_mode: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ActorCollisionFilterJson {
    pub layer_mask_a: u8,
    pub layer_mask_b: u8,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CharacterKnockbackJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub knockback_distance: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub knockback_arc_height: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CharacterStateAndRewardsJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stance_id: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experience_reward: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loot_drop_multiplier: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_stunned: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CharacterMorphParamsJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub morph_duration_sec: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blend_scale: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CharacterModelBinding {
    pub object_path: String,
    pub model_name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CharacterTransformationJson {
    pub name: String,
    pub target_root: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CharacterAttachmentReceptorJson {
    pub event_type: String,
    pub target_bone: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CharacterAnimStateJson {
    pub state_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anim_clip: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub sound_cues: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ActorAiActionJson {
    pub action_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_or_speed: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_value: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_hex: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ActorSocketOffsetJson {
    pub socket_name: String,
    pub translation: [f32; 3],
    pub rotation_quat: [f32; 4],
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RawCharacterBlock {
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

pub fn parse_alert_distance(chunk: &[u8]) -> Option<f32> {
    if let Ok((_, sub)) = parse_chunk_elements(chunk) {
        for (sid, sdata) in sub {
            if sid == 34 && sdata.len() >= 4 {
                return parse_f32_safe(&sdata);
            }
        }
    }
    if chunk.len() >= 7 {
        return parse_f32_safe(&chunk[3..7]);
    }
    None
}

pub fn build_alert_distance(dist: f32, endian: Endian) -> Vec<u8> {
    let sub = vec![(34, endian.f32_to_bytes(dist).to_vec())];
    build_chunk_from_elements_with_endian(false, &sub, endian)
}

pub fn parse_ragdoll_config(data: &[u8]) -> Option<ActorRagdollConfigJson> {
    if let Ok((_, elements)) = parse_chunk_elements(data) {
        let mut cfg = ActorRagdollConfigJson::default();
        for (id, val) in elements {
            match id {
                30 if !val.is_empty() => cfg.ragdoll_mode = Some(val[0]),
                31 if val.len() >= 4 => cfg.blend_time_sec = parse_f32_safe(&val),
                34 if !val.is_empty() => cfg.ragdoll_flags = Some(val[0]),
                35 if val.len() >= 4 => {
                    cfg.impact_reaction_mode =
                        Some(u32::from_le_bytes(val[0..4].try_into().unwrap_or_default()))
                }
                _ => {}
            }
        }
        return Some(cfg);
    }
    None
}

pub fn build_ragdoll_config(cfg: &ActorRagdollConfigJson, endian: Endian) -> Vec<u8> {
    let mut sub = Vec::new();
    sub.push((30, vec![cfg.ragdoll_mode.unwrap_or(0)]));
    if let Some(t) = cfg.blend_time_sec {
        sub.push((31, endian.f32_to_bytes(t).to_vec()));
    }
    sub.push((34, vec![cfg.ragdoll_flags.unwrap_or(2)]));
    sub.push((
        35,
        endian
            .u32_to_bytes(cfg.impact_reaction_mode.unwrap_or(0))
            .to_vec(),
    ));
    build_chunk_from_elements_with_endian(false, &sub, endian)
}

pub fn parse_collision_filter(data: &[u8]) -> Option<ActorCollisionFilterJson> {
    if let Ok((_, elements)) = parse_chunk_elements(data) {
        let mut f = ActorCollisionFilterJson::default();
        for (id, val) in elements {
            match id {
                42 if !val.is_empty() => f.layer_mask_a = val[0],
                43 if !val.is_empty() => f.layer_mask_b = val[0],
                _ => {}
            }
        }
        return Some(f);
    }
    None
}

pub fn build_collision_filter(f: &ActorCollisionFilterJson, endian: Endian) -> Vec<u8> {
    let sub = vec![(42, vec![f.layer_mask_a]), (43, vec![f.layer_mask_b])];
    build_chunk_from_elements_with_endian(false, &sub, endian)
}

pub fn parse_equipment_definition(
    elements: &[(u32, Vec<u8>)],
    character_name: &str,
) -> Option<CharacterEquipmentJson> {
    if let Some((_, chunk63)) = elements.iter().find(|(id, _)| *id == 63) {
        let mut i = 0;
        while i + 4 <= chunk63.len() {
            if let Some(s) = read_length_prefixed_string(&chunk63[i..]) {
                if !s.is_empty() && !s.starts_with('[') && s != character_name {
                    return Some(CharacterEquipmentJson {
                        item_name: s,
                        mount_socket: "Right_Hand_Carry".to_string(),
                        primary_slot: 40,
                        support_slot: 43,
                        equipped_slot: 40,
                    });
                }
                i += 4 + s.len();
            } else {
                i += 1;
            }
        }
    }
    None
}

pub fn parse_morph_parameters(chunk: &[u8]) -> (Option<f32>, Option<f32>) {
    let mut duration = Some(5.0f32);
    let mut scale = Some(1.0f32);

    for window in chunk.windows(4) {
        let val = f32::from_le_bytes(window.try_into().unwrap_or_default());
        if val == 5.0 {
            duration = Some(5.0);
        } else if val == 1.0 {
            scale = Some(1.0);
        }
    }

    (duration, scale)
}

pub fn parse_model_binding(data: &[u8]) -> Option<CharacterModelBinding> {
    if let Ok((_, elements)) = parse_chunk_elements(data) {
        let mut obj_path = String::new();
        let mut model_name = String::new();

        for (id, chunk) in elements {
            if id == 20
                && let Some(s) = read_length_prefixed_string(&chunk)
            {
                obj_path = s;
            } else if id == 21
                && let Some(s) = read_length_prefixed_string(&chunk)
            {
                model_name = s;
            }
        }

        if !obj_path.is_empty() {
            return Some(CharacterModelBinding {
                object_path: obj_path,
                model_name,
            });
        }
    }
    None
}

pub fn build_model_binding(obj_path: &str, model_name: &str, endian: Endian) -> Vec<u8> {
    let elements = vec![
        (20, endian.write_length_prefixed_string(obj_path)),
        (21, endian.write_length_prefixed_string(model_name)),
    ];
    build_chunk_from_elements_with_endian(false, &elements, endian)
}

pub fn parse_transformation_container(data: &[u8]) -> Vec<CharacterTransformationJson> {
    let mut out = Vec::new();
    let mut str_list = Vec::new();
    let mut i = 0;

    while i + 4 <= data.len() {
        if let Some(s) = read_length_prefixed_string(&data[i..]) {
            if !s.is_empty() && !s.starts_with('[') {
                str_list.push(s.clone());
            }
            i += 4 + s.len();
        } else {
            i += 1;
        }
    }

    if str_list.len() >= 2 {
        out.push(CharacterTransformationJson {
            name: str_list[0].clone(),
            target_root: str_list[1].clone(),
        });
    }

    out
}

pub fn parse_animation_graph(data: &[u8]) -> Vec<CharacterAnimStateJson> {
    let mut states = Vec::new();
    let (_, entries) = match parse_chunk_elements(data) {
        Ok(res) => res,
        Err(_) => return states,
    };

    for (_, chunk) in entries {
        let mut state_name = String::new();
        let mut anim_clip = None;
        let mut sound_cues = Vec::new();

        let mut i = 0;
        while i + 4 <= chunk.len() {
            if let Some(s) = read_length_prefixed_string(&chunk[i..]) {
                if s.contains("ANIM\\") {
                    anim_clip = Some(s.clone());
                } else if s.starts_with("Drop ")
                    || s.starts_with("Peasant ")
                    || s.starts_with("foot")
                    || s.contains("Hit")
                    || s == "Archie Hit"
                {
                    sound_cues.push(s.clone());
                } else if state_name.is_empty() && s.len() >= 3 && !s.starts_with('[') {
                    state_name = s.clone();
                }
                i += 4 + s.len();
            } else {
                i += 1;
            }
        }

        if !state_name.is_empty() {
            sound_cues.dedup();
            states.push(CharacterAnimStateJson {
                state_name,
                anim_clip,
                sound_cues,
            });
        }
    }

    states
}

pub fn parse_ai_behaviors(data: &[u8]) -> Vec<String> {
    let mut behaviors = Vec::new();

    fn scan_behaviors_recursive(slice: &[u8], out: &mut Vec<String>) {
        if let Ok((type_id, fields)) = parse_typed_container(slice) {
            let hi = type_id >> 16;
            let mid = (type_id >> 8) & 0xFF;

            if hi == 0x0046 && (mid == 0x40 || mid == 0x49) {
                for (fid, fdata) in &fields {
                    if (*fid == 20 || *fid == 21)
                        && let Some(s) = read_length_prefixed_string(fdata)
                    {
                        let clean = s.trim();
                        if clean.len() >= 2
                            && !clean.starts_with('[')
                            && !clean.contains("@F")
                            && !clean.contains("@I")
                            && !clean.contains("@G")
                        {
                            out.push(clean.to_string());
                        }
                    }
                }
            }

            for (_, fdata) in fields {
                scan_behaviors_recursive(&fdata, out);
            }
        } else if let Ok((_, elements)) = parse_chunk_elements(slice) {
            for (_, cdata) in elements {
                scan_behaviors_recursive(&cdata, out);
            }
        }
    }

    scan_behaviors_recursive(data, &mut behaviors);

    if behaviors.is_empty() {
        let mut i = 0;
        while i + 4 <= data.len() {
            if let Some(s) = read_length_prefixed_string(&data[i..]) {
                let clean = s.trim();
                if clean.len() >= 3
                    && !clean.starts_with('[')
                    && !clean.contains("@F")
                    && !clean.contains("@I")
                    && !clean.contains("@G")
                {
                    behaviors.push(clean.to_string());
                }
                i += 4 + s.len();
            } else {
                i += 1;
            }
        }
    }

    behaviors.dedup();
    behaviors
}

pub fn parse_ai_actions(data: &[u8]) -> Vec<ActorAiActionJson> {
    let mut actions = Vec::new();

    fn scan_actions_recursive(slice: &[u8], out: &mut Vec<ActorAiActionJson>) {
        if let Ok((type_id, fields)) = parse_typed_container(slice) {
            let hi = type_id >> 16;
            let mid = (type_id >> 8) & 0xFF;

            if hi == 0x0046 && (mid == 0x47 || mid == 0x07) {
                let mut name = String::new();
                let mut range_val = None;
                let mut sec_val = None;

                for (fid, fdata) in &fields {
                    if (*fid == 20 || *fid == 21)
                        && name.is_empty()
                        && let Some(s) = read_length_prefixed_string(fdata)
                    {
                        let clean = s.trim();
                        if clean.len() >= 2 && !clean.starts_with('[') && !clean.contains("@F") {
                            name = clean.to_string();
                        }
                    }

                    if (*fid == 26 || *fid == 28 || *fid == 30)
                        && fdata.len() >= 4
                        && let Some(f) = parse_f32_safe(fdata)
                        && (0.05..=1000.0).contains(&f)
                    {
                        if range_val.is_none() {
                            range_val = Some(f);
                        } else if sec_val.is_none() {
                            sec_val = Some(f);
                        }
                    }
                }

                if !name.is_empty() {
                    out.push(ActorAiActionJson {
                        action_name: name,
                        range_or_speed: range_val,
                        secondary_value: sec_val,
                        raw_hex: None,
                    });
                }
            }

            for (_, fdata) in fields {
                scan_actions_recursive(&fdata, out);
            }
        } else if let Ok((_, elements)) = parse_chunk_elements(slice) {
            for (_, cdata) in elements {
                scan_actions_recursive(&cdata, out);
            }
        }
    }

    scan_actions_recursive(data, &mut actions);

    if actions.is_empty() {
        let mut i = 0;
        while i + 4 <= data.len() {
            if let Some(s) = read_length_prefixed_string(&data[i..]) {
                let clean = s.trim();
                if clean.len() >= 2 && !clean.starts_with('[') && !clean.contains("@F") {
                    let mut range_val = None;
                    let mut sec_val = None;

                    let search_end = (i + 4 + s.len() + 32).min(data.len());
                    let search_window = &data[i + 4 + s.len()..search_end];
                    for window in search_window.windows(4) {
                        let val = f32::from_le_bytes(window.try_into().unwrap_or_default());
                        if val.is_finite() && !val.is_subnormal() && (0.05..=1000.0).contains(&val)
                        {
                            if range_val.is_none() {
                                range_val = Some(val);
                            } else if sec_val.is_none() {
                                sec_val = Some(val);
                                break;
                            }
                        }
                    }

                    actions.push(ActorAiActionJson {
                        action_name: clean.to_string(),
                        range_or_speed: range_val,
                        secondary_value: sec_val,
                        raw_hex: None,
                    });
                }
                i += 4 + s.len();
            } else {
                i += 1;
            }
        }
    }

    actions.dedup_by(|a, b| a.action_name == b.action_name);
    actions
}

pub fn parse_actor_attachments(data: &[u8]) -> Vec<ActorSocketOffsetJson> {
    let mut out = Vec::new();
    let mut socket_blobs = Vec::new();

    if let Ok((_, elements)) = parse_chunk_elements(data) {
        for (_, chunk) in elements {
            if let Ok((_, sub)) = parse_chunk_elements(&chunk) {
                for (_, s) in sub {
                    if s.starts_with(b"\x5A\x10\x46\x00") {
                        socket_blobs.push(s);
                    }
                }
            } else if chunk.starts_with(b"\x5A\x10\x46\x00") {
                socket_blobs.push(chunk);
            }
        }
    }

    if socket_blobs.is_empty() {
        let target = b"\x5A\x10\x46\x00";
        let mut pos = 0;
        while pos + 4 <= data.len() {
            if let Some(rel) = data[pos..].windows(4).position(|w| w == target) {
                let start = pos + rel;
                socket_blobs.push(data[start..].to_vec());
                pos = start + 4;
            } else {
                break;
            }
        }
    }

    for blob in socket_blobs {
        if let Ok((type_id, fields)) = parse_typed_container(&blob)
            && type_id == 0x0046105A
        {
            let mut name = String::new();
            let mut translation = [0.0f32; 3];
            let mut rotation_quat = [0.0f32; 4];

            for (fid, fdata) in fields {
                match fid {
                    // ID 30 (0x1E): Local position translation vector
                    30 if fdata.len() >= 12 => {
                        let mut cur = Cursor::new(&fdata);
                        let tx = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                        let ty = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                        let tz = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                        if tx.is_finite() && ty.is_finite() && tz.is_finite() {
                            translation = [tx, ty, tz];
                        }
                    }
                    // ID 34 (0x22): Local rotation orientation quaternion
                    34 if fdata.len() >= 16 => {
                        let mut cur = Cursor::new(&fdata);
                        let qx = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                        let qy = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                        let qz = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                        let qw = cur.read_f32::<LittleEndian>().unwrap_or(1.0);
                        if qx.is_finite() && qy.is_finite() && qz.is_finite() && qw.is_finite() {
                            rotation_quat = [qx, qy, qz, qw];
                        }
                    }
                    // ID 35 (0x23): Socket descriptor name
                    35 => {
                        if let Some(s) = read_length_prefixed_string(&fdata) {
                            name = s.trim().to_string();
                        }
                    }
                    _ => {}
                }
            }

            if !name.is_empty() {
                out.push(ActorSocketOffsetJson {
                    socket_name: name,
                    translation,
                    rotation_quat,
                });
            }
        }
    }

    if out.is_empty() {
        let mut i = 0;
        while i + 4 <= data.len() {
            if let Some(s) = read_length_prefixed_string(&data[i..]) {
                let clean = s.trim();
                if (clean.ends_with("_item")
                    || clean.starts_with("Hookup")
                    || clean.contains("hand"))
                    && i + 4 + s.len() + 28 <= data.len()
                {
                    let mut cur = Cursor::new(&data[i + 4 + s.len()..]);
                    let tx = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    let ty = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    let tz = cur.read_f32::<LittleEndian>().unwrap_or(0.0);

                    let qx = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    let qy = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    let qz = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    let qw = cur.read_f32::<LittleEndian>().unwrap_or(1.0);

                    if tx.is_finite()
                        && ty.is_finite()
                        && tz.is_finite()
                        && !tx.is_subnormal()
                        && !ty.is_subnormal()
                        && !tz.is_subnormal()
                    {
                        out.push(ActorSocketOffsetJson {
                            socket_name: clean.to_string(),
                            translation: [tx, ty, tz],
                            rotation_quat: [qx, qy, qz, qw],
                        });
                    }
                }
                i += 4 + s.len();
            } else {
                i += 1;
            }
        }
    }

    out
}

pub fn build_actor_attachments(sockets: &[ActorSocketOffsetJson], endian: Endian) -> Vec<u8> {
    if sockets.is_empty() {
        return vec![1, 1, 0, 0];
    }

    let mut socket_chunks = Vec::new();
    for (i, s) in sockets.iter().enumerate() {
        let mut pos_bytes = Vec::with_capacity(12);
        let _ = endian.write_f32(&mut pos_bytes, s.translation[0]);
        let _ = endian.write_f32(&mut pos_bytes, s.translation[1]);
        let _ = endian.write_f32(&mut pos_bytes, s.translation[2]);

        let mut rot_bytes = Vec::with_capacity(16);
        let _ = endian.write_f32(&mut rot_bytes, s.rotation_quat[0]);
        let _ = endian.write_f32(&mut rot_bytes, s.rotation_quat[1]);
        let _ = endian.write_f32(&mut rot_bytes, s.rotation_quat[2]);
        let _ = endian.write_f32(&mut rot_bytes, s.rotation_quat[3]);

        let sub_fields = vec![
            (30, pos_bytes),
            (31, vec![0; 12]),
            (32, endian.u32_to_bytes(17 + i as u32).to_vec()),
            (34, rot_bytes),
            (35, endian.write_length_prefixed_string(&s.socket_name)),
        ];

        let blob = build_typed_container_with_endian(0x0046105A, &sub_fields, endian);
        socket_chunks.push((i as u32, blob));
    }

    let sub_container = build_chunk_from_elements_with_endian(true, &socket_chunks, endian);
    let top_layer = vec![(10, vec![1, 0, 0, 0]), (1, sub_container)];
    build_chunk_from_elements_with_endian(false, &top_layer, endian)
}

pub fn extract_embedded_lua_and_bytecode(data: &[u8]) -> Option<(String, Vec<u8>)> {
    let limit = if let Some(pos) = data.windows(4).position(|w| w == b"\x1bLua") {
        pos.saturating_sub(4)
    } else {
        data.len()
    };

    let mut lines = Vec::new();
    let mut i = 0;

    while i + 4 <= limit {
        let len = u32::from_le_bytes(data[i..i + 4].try_into().unwrap_or_default()) as usize;
        if len > 0 && len < 2048 && i + 4 + len <= limit {
            let slice = &data[i + 4..i + 4 + len];
            if slice
                .iter()
                .all(|&b| (0x20..=0x7E).contains(&b) || b == b'\t' || b == b' ')
            {
                if let Ok(s) = std::str::from_utf8(slice) {
                    let clean = s.trim_matches(char::from(0));
                    if !clean.is_empty() {
                        lines.push(clean.to_string());
                    }
                }
                i += 4 + len;
                continue;
            }
        }
        i += 1;
    }

    let bytecode = if let Some(pos) = data.windows(4).position(|w| w == b"\x1bLua") {
        data[pos..].to_vec()
    } else {
        Vec::new()
    };

    if !lines.is_empty() {
        Some((lines.join("\n"), bytecode))
    } else {
        None
    }
}

pub fn rebuild_lua_component(original_bytes: &[u8], new_lua: &str) -> Vec<u8> {
    let mut new_text_blob = Vec::new();
    for line in new_lua.lines() {
        let bytes = line.as_bytes();
        new_text_blob.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        new_text_blob.extend_from_slice(bytes);
    }

    if let Some(pos) = original_bytes.windows(4).position(|w| w == b"\x1bLua") {
        let prefix_len = 4.min(original_bytes.len());
        let mut out = original_bytes[..prefix_len].to_vec();
        out.extend_from_slice(&new_text_blob);
        out.extend_from_slice(&original_bytes[pos.saturating_sub(4)..]);
        return out;
    }

    new_text_blob
}

pub fn extract_embedded_facefx(data: &[u8]) -> Option<Vec<u8>> {
    if let Ok((_, sub_elems)) = parse_chunk_elements(data) {
        for (sub_id, face_data) in sub_elems {
            if sub_id == 11 && face_data.starts_with(b"FACE") {
                return Some(face_data);
            }
            if sub_id == 11 && face_data.len() > 4 && &face_data[4..8] == b"FACE" {
                return Some(face_data[4..].to_vec());
            }
        }
    }
    None
}

pub fn rebuild_embedded_facefx(
    _original_chunk: &[u8],
    new_fxe_bytes: &[u8],
    endian: Endian,
) -> Vec<u8> {
    let size_bytes = endian.u32_to_bytes(new_fxe_bytes.len() as u32).to_vec();
    let sub_elements = vec![(10, size_bytes), (11, new_fxe_bytes.to_vec())];
    build_chunk_from_elements_with_endian(false, &sub_elements, endian)
}

pub fn parse_all_effect_receptors(
    data: &[u8],
) -> (
    Option<String>,
    Vec<String>,
    Vec<CharacterAttachmentReceptorJson>,
) {
    let mut lifeforce = None;
    let mut grapple_bones = Vec::new();
    let mut effect_receptors = Vec::new();

    if let Some(pos) = data.windows(15).position(|w| w == b"Lifeforce_Emit_") {
        let slice = &data[pos + 15..pos + 25.min(data.len())];
        if let Ok(s) = std::str::from_utf8(slice) {
            let color: String = s
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            if !color.is_empty() {
                lifeforce = Some(color);
            }
        }
    }

    let mut i = 0;
    let mut last_event = String::new();

    while i + 4 <= data.len() {
        if let Some(s) = read_length_prefixed_string(&data[i..]) {
            let clean = s.trim_matches(char::from(0)).trim().to_string();

            if clean == "Held (Target)"
                || clean == "Hold"
                || clean == "Character_Burn"
                || clean == "Ouch"
                || clean == "FleshHit"
                || clean == "Flesh Impact"
                || clean == "ClothHit"
                || clean == "Clothing Impact"
                || clean.starts_with("Head Slowdown")
                || clean.starts_with("Head Confused")
                || clean.starts_with("Betrayal")
                || clean.starts_with("Head Submission")
                || clean == "lichaam confused"
                || clean == "klein"
            {
                last_event = clean.clone();
            } else if !last_event.is_empty()
                && (clean.ends_with("_Hip")
                    || clean.ends_with("_Shoulder")
                    || clean.ends_with("_Elbow")
                    || clean.ends_with("_Wrist")
                    || clean.ends_with("_Knee")
                    || clean.ends_with("_Ankle")
                    || clean.ends_with("_Ball")
                    || clean.starts_with("Back_")
                    || clean == "Back Low"
                    || clean == "Back High"
                    || clean == "Head"
                    || clean == "Neck"
                    || clean == "head_item")
            {
                if last_event == "Held (Target)" && clean != "head_item" {
                    grapple_bones.push(clean.clone());
                }

                effect_receptors.push(CharacterAttachmentReceptorJson {
                    event_type: last_event.clone(),
                    target_bone: clean,
                });
            }

            i += 4 + s.len();
        } else {
            i += 1;
        }
    }

    grapple_bones.dedup();
    (lifeforce, grapple_bones, effect_receptors)
}

pub fn update_lifeforce_color(data: &[u8], new_color: &str) -> Vec<u8> {
    let target = b"Lifeforce_Emit_";
    if let Some(pos) = data.windows(target.len()).position(|w| w == target) {
        let color_start = pos + target.len();
        if let Some(null_rel) = data[color_start..].iter().position(|&b| b == 0) {
            let null_pos = color_start + null_rel;
            let mut out = data[..color_start].to_vec();
            out.extend_from_slice(new_color.as_bytes());
            out.extend_from_slice(&data[null_pos..]);
            return out;
        }
    }
    data.to_vec()
}
