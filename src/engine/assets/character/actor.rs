use serde::{Deserialize, Serialize};

use super::breakable::BreakablePropsConfigJson;
use crate::engine::assets::{build_chunk_from_elements_with_endian, parse_chunk_elements};
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
    pub is_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_active_on_spawn: Option<bool>,
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
pub struct RawCharacterBlock {
    pub id: u32,
    pub hex: String,
}

pub fn parse_f32_safe(chunk: &[u8]) -> Option<f32> {
    if chunk.len() >= 4 {
        let val = f32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
        if val.is_finite() && !val.is_nan() && val.abs() >= 1e-4 && val.abs() <= 500_000.0 {
            return Some(val);
        }
    }
    None
}

pub fn parse_equipment_definition(elements: &[(u32, Vec<u8>)]) -> Option<CharacterEquipmentJson> {
    let mut item_name = String::from("Standard Plate / Prop");

    if let Some((_, chunk63)) = elements.iter().find(|(id, _)| *id == 63) {
        let mut i = 0;
        while i + 4 <= chunk63.len() {
            if let Some(s) = read_length_prefixed_string(&chunk63[i..]) {
                if !s.is_empty() && !s.starts_with('[') {
                    item_name = s;
                    break;
                }
                i += 4 + s.len();
            } else {
                i += 1;
            }
        }
    }

    Some(CharacterEquipmentJson {
        item_name,
        mount_socket: "Right_Hand_Carry".to_string(),
        primary_slot: 40,
        support_slot: 43,
        equipped_slot: 40,
    })
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
    if let Ok((_, elements)) = parse_chunk_elements(data) {
        for (_, chunk) in elements {
            let mut i = 0;
            while i + 4 <= chunk.len() {
                if let Some(s) = read_length_prefixed_string(&chunk[i..]) {
                    if s.len() >= 3 && !s.starts_with('[') && !s.contains('\\') {
                        behaviors.push(s.clone());
                    }
                    i += 4 + s.len();
                } else {
                    i += 1;
                }
            }
        }
    }
    behaviors.dedup();
    behaviors
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
