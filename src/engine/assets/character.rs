use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Cursor;
use std::path::Path;

use super::{
    build_chunk_from_elements, build_typed_container, parse_chunk_elements, parse_typed_container,
};
use crate::engine::common::{read_length_prefixed_string, write_length_prefixed_string};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CharacterActorJson {
    pub _engine_metadata: CharacterEngineMetadataJson,
    pub character_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_tag: Option<String>,
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BreakablePropsConfigJson {
    pub base_health: f32,
    pub collapse_target_model: String,
    pub physics_material_id: u32,
    pub debris_pieces_count: usize,
    pub sound_cue_id: u32,
    pub trigger_collapse_on_hit: bool,
    pub spawn_debris_particles: bool,
    pub can_be_carried: bool,
    pub center_offset: [f32; 3],
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

fn validate_float_field(field_name: &str, val: Option<f32>) -> Result<()> {
    if let Some(v) = val {
        if !v.is_finite() {
            bail!(
                "Field '{}' must be a valid finite number, found: {}",
                field_name,
                v
            );
        }
        if v.abs() > 1_000_000.0 {
            bail!("Field '{}' value is unreasonably large: {}", field_name, v);
        }
    }
    Ok(())
}

fn extract_collapse_target_from_script(script: &str) -> Option<String> {
    for line in script.lines() {
        if line.contains("Collapse(")
            && let Some(first_quote) = line.find('"')
        {
            let tail = &line[first_quote + 1..];
            if let Some(second_quote) = tail.find('"') {
                let target = &tail[..second_quote];
                return Some(target.replace("\\\\", "\\"));
            }
        }
    }
    None
}

pub fn export_character_to_json(
    data: &[u8],
    assets_dir: Option<&Path>,
    stem: &str,
) -> Result<String> {
    if data.len() < 5 {
        bail!("Data too short for a Character / Actor container");
    }

    let (type_id, elements) =
        parse_typed_container(data).context("Failed to parse Character typed container")?;

    let is_breakable = type_id == 0x00463018;

    let mut character_name = String::from("Unnamed_Character");
    let mut resource_tag = None;
    let mut model_binding = None;
    let mut collapse_target_model = None;
    let mut facefx_actor = None;
    let mut embedded_facefx_file = None;
    let mut lifeforce_color = None;
    let mut lua_script_file = None;
    let mut embedded_lua_script = None;
    let mut animation_states = Vec::new();
    let mut ai_behaviors = Vec::new();
    let mut transformations = Vec::new();
    let mut effect_receptors = Vec::new();
    let mut minion_grapple_bones = Vec::new();
    let mut attributes = CharacterAttributesJson::default();
    let mut flags = CharacterFlagsJson::default();
    let mut raw_flags_hex_val = None;
    let mut timings = CharacterCombatTimingsJson::default();
    let mut knockback = CharacterKnockbackJson::default();
    let mut state_and_rewards = CharacterStateAndRewardsJson::default();
    let mut morph_params = CharacterMorphParamsJson::default();
    let mut equipment = None;
    let mut unmapped_raw_blocks = Vec::new();

    // Breakable-specific structured properties
    let mut brk_health = 5.0f32;
    let mut brk_material_id = 2u32;
    let mut brk_debris_count = 8usize;
    let mut brk_sound_cue = 112u32;
    let mut brk_trigger_collapse = true;
    let mut brk_spawn_particles = true;
    let mut brk_can_be_carried = true;
    let mut brk_center_offset = [0.0f32, 0.0f32, 0.0f32];

    for (id, chunk) in &elements {
        match *id {
            20 => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    resource_tag = Some(s);
                }
            }
            21 | 25 => {
                if let Some(s) = read_length_prefixed_string(chunk)
                    && character_name == "Unnamed_Character"
                {
                    character_name = s;
                }
            }
            22 if chunk.len() >= 4 => {
                let mask = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
                flags = CharacterFlagsJson {
                    casts_dynamic_shadows: (mask & 0x2000_0000) != 0,
                    can_be_targeted: (mask & 0x0100_0000) != 0,
                    ragdoll_on_death: (mask & 0x0040_0000) != 0,
                    is_civilian: if is_breakable {
                        None
                    } else {
                        Some((mask & 0x0000_0002) != 0)
                    },
                };
                raw_flags_hex_val = Some(format!("0x{:08X}", mask));
            }
            23 if !chunk.is_empty() => {
                attributes.is_enabled = Some(chunk[0] != 0);
            }
            28 if !chunk.is_empty() => {
                state_and_rewards.stance_id = Some(chunk[0]);
            }
            29 => {
                if is_breakable {
                    brk_can_be_carried = chunk.len() >= 4 && chunk[0] != 0;
                } else if equipment.is_none() {
                    equipment = parse_equipment_definition(&elements);
                }
            }
            63 => {
                if !is_breakable && equipment.is_none() {
                    equipment = parse_equipment_definition(&elements);
                }
            }
            31 | 37 | 67 => {
                if model_binding.is_none()
                    && let Some(mb) = parse_model_binding(chunk)
                {
                    model_binding = Some(mb);
                }
            }
            46 if is_breakable && chunk.len() >= 4 => {
                brk_material_id = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
            }
            70 if is_breakable && !chunk.is_empty() => {
                brk_debris_count = chunk[0] as usize;
            }
            201 if is_breakable && chunk.len() >= 10 => {
                brk_sound_cue = chunk[6] as u32;
            }
            202 if is_breakable && !chunk.is_empty() => {
                brk_trigger_collapse = chunk[0] != 0;
            }
            203 if is_breakable && !chunk.is_empty() => {
                brk_spawn_particles = chunk[0] != 0;
            }
            300 if is_breakable && chunk.len() >= 15 => {
                let mut cur = Cursor::new(&chunk[3..15]);
                let x = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                let y = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                let z = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                if x.is_finite() && y.is_finite() && z.is_finite() {
                    brk_center_offset = [x, y, z];
                }
            }
            50 => {
                animation_states = parse_animation_graph(chunk);
            }
            61 if chunk.len() >= 4 => {
                attributes.move_speed_scale = parse_f32_safe(chunk);
            }
            62 if chunk.len() >= 4 => {
                attributes.turn_speed_scale = parse_f32_safe(chunk);
            }
            64 if chunk.len() >= 4 => {
                attributes.perception_radius = parse_f32_safe(chunk);
            }
            65 if chunk.len() >= 4 => {
                attributes.collision_radius = parse_f32_safe(chunk);
            }
            66 if chunk.len() >= 4 => {
                attributes.engagement_distance = parse_f32_safe(chunk);
            }
            74 if chunk.len() >= 4 => {
                attributes.faction_id = Some(u32::from_le_bytes(
                    chunk[0..4].try_into().unwrap_or_default(),
                ));
            }
            77 if !chunk.is_empty() => {
                attributes.is_active_on_spawn = Some(chunk[0] != 0);
            }
            80 if chunk.len() >= 4 => {
                attributes.mass = parse_f32_safe(chunk);
            }
            83 if chunk.len() >= 4 => {
                attributes.base_health = parse_f32_safe(chunk);
            }
            112 => {
                ai_behaviors = parse_ai_behaviors(chunk);
            }
            115 | 200 => {
                if let Some((lua_source, bytecode)) = extract_embedded_lua_and_bytecode(chunk) {
                    collapse_target_model = extract_collapse_target_from_script(&lua_source);
                    embedded_lua_script = Some(lua_source.clone());

                    if let Some(dir) = assets_dir {
                        let scripts_dir = dir.join("scripts");
                        let _ = fs::create_dir_all(&scripts_dir);
                        let file_base = format!("{}_logic", stem);
                        let lua_path = scripts_dir.join(format!("{}.lua", file_base));
                        let _ = fs::write(&lua_path, lua_source.as_bytes());

                        if !bytecode.is_empty() {
                            let luac_path = scripts_dir.join(format!("{}.luac", file_base));
                            let _ = fs::write(luac_path, &bytecode);
                        }
                        lua_script_file = Some(format!("assets/scripts/{}.lua", file_base));
                    }
                }

                if *id == 200 {
                    for window in chunk.windows(4) {
                        let val = f32::from_le_bytes(window.try_into().unwrap_or_default());
                        if val > 0.0
                            && val <= 50_000.0
                            && val.is_finite()
                            && (val == 5.0 || val == 10.0 || val == 1.0)
                        {
                            brk_health = val;
                            break;
                        }
                    }
                }
            }
            116 => {
                if let Ok((_, sub)) = parse_chunk_elements(chunk) {
                    for (sid, sdata) in sub {
                        if sid == 21 && sdata.len() >= 4 {
                            attributes.target_awareness_range = parse_f32_safe(&sdata);
                        }
                    }
                }
            }
            117 => {
                let (lf, grapples, receptors) = parse_all_effect_receptors(chunk);
                lifeforce_color = lf;
                minion_grapple_bones = grapples;
                effect_receptors = receptors;
            }
            119 if chunk.len() >= 4 => {
                timings.stumble_recovery_time_sec = parse_f32_safe(chunk);
            }
            120 if !chunk.is_empty() => {
                state_and_rewards.is_stunned = Some(chunk[0] != 0);
            }
            121 if chunk.len() >= 4 => {
                attributes.hit_reaction_force = parse_f32_safe(chunk);
            }
            122 if chunk.len() >= 4 => {
                state_and_rewards.experience_reward = Some(u32::from_le_bytes(
                    chunk[0..4].try_into().unwrap_or_default(),
                ));
            }
            123 if chunk.len() >= 4 => {
                state_and_rewards.loot_drop_multiplier = Some(u32::from_le_bytes(
                    chunk[0..4].try_into().unwrap_or_default(),
                ));
            }
            127 if chunk.len() >= 4 => {
                timings.attack_windup_time_sec = parse_f32_safe(chunk);
            }
            130 if chunk.len() >= 26 => {
                knockback.knockback_distance = parse_f32_safe(&chunk[18..22]);
                knockback.knockback_arc_height = parse_f32_safe(&chunk[22..26]);
            }
            131 if chunk.len() >= 4 => {
                timings.attack_cooldown_time_sec = parse_f32_safe(chunk);
            }
            132 if chunk.len() >= 4 => {
                timings.block_window_time_sec = parse_f32_safe(chunk);
            }
            133 if chunk.len() >= 4 => {
                timings.invulnerability_time_sec = parse_f32_safe(chunk);
            }
            139 => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    facefx_actor = Some(s);
                }
            }
            146 => {
                transformations = parse_transformation_container(chunk);
            }
            149 => {
                let (duration, scale) = parse_morph_parameters(chunk);
                morph_params.morph_duration_sec = duration;
                morph_params.blend_scale = scale;
            }
            166 => {
                if let Some(fxe_bytes) = extract_embedded_facefx(chunk)
                    && let Some(dir) = assets_dir
                {
                    let face_dir = dir.join("facefx");
                    let _ = fs::create_dir_all(&face_dir);
                    let fxe_name = format!("{}_face.fxe", stem);
                    let fxe_path = face_dir.join(&fxe_name);
                    let _ = fs::write(fxe_path, &fxe_bytes);
                    embedded_facefx_file = Some(format!("assets/facefx/{}", fxe_name));
                }
            }
            88 | 92 | 96 | 97 | 98 | 102 | 111 | 114 | 128 | 129 | 134 | 147 | 148 | 152 | 153
            | 159 | 161 | 167 | 19 | 1 | 41 | 42 | 45 | 71 | 301 => {}
            _ => {
                unmapped_raw_blocks.push(RawCharacterBlock {
                    id: *id,
                    hex: hex::encode_upper(chunk),
                });
            }
        }
    }

    let engine_class = match type_id {
        0x00463018 => "TREBreakable".to_string(),
        _ => "TREActorController".to_string(),
    };

    let metadata = CharacterEngineMetadataJson {
        type_id_hex: format!("{:08X}", type_id),
        engine_class,
        raw_flags_hex: raw_flags_hex_val,
        unmapped_raw_blocks,
    };

    let breakable_config = if is_breakable {
        attributes.base_health = Some(brk_health);
        Some(BreakablePropsConfigJson {
            base_health: brk_health,
            collapse_target_model: collapse_target_model.clone().unwrap_or_default(),
            physics_material_id: brk_material_id,
            debris_pieces_count: brk_debris_count,
            sound_cue_id: brk_sound_cue,
            trigger_collapse_on_hit: brk_trigger_collapse,
            spawn_debris_particles: brk_spawn_particles,
            can_be_carried: brk_can_be_carried,
            center_offset: brk_center_offset,
        })
    } else {
        None
    };

    let char_json = CharacterActorJson {
        _engine_metadata: metadata,
        character_name,
        resource_tag,
        model_binding,
        breakable_config,
        collapse_target_model: if is_breakable {
            None
        } else {
            collapse_target_model
        },
        actor_flags: Some(flags),
        combat_attributes: Some(attributes),
        timing_parameters: Some(timings),
        knockback_parameters: Some(knockback),
        state_and_rewards: Some(state_and_rewards),
        morph_parameters: Some(morph_params),
        equipment,
        facefx_actor,
        embedded_facefx_file,
        lifeforce_color,
        lua_script_file,
        embedded_lua_script,
        animation_states,
        ai_behaviors,
        transformations,
        effect_receptors,
        minion_grapple_bones,
    };

    serde_json::to_string_pretty(&char_json).map_err(|e| anyhow::anyhow!(e))
}

pub fn import_character_from_json(json_str: &str, project_dir: Option<&Path>) -> Result<Vec<u8>> {
    let parsed: CharacterActorJson =
        serde_json::from_str(json_str).context("Syntax error in Character Actor JSON format")?;

    let clean_type_id = parsed
        ._engine_metadata
        .type_id_hex
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X");
    let type_id = u32::from_str_radix(clean_type_id, 16).with_context(|| {
        format!(
            "Invalid TypeID hex: '{}'",
            parsed._engine_metadata.type_id_hex
        )
    })?;

    let is_breakable = type_id == 0x00463018;

    if let Some(ref attrs) = parsed.combat_attributes {
        validate_float_field("move_speed_scale", attrs.move_speed_scale)?;
        validate_float_field("turn_speed_scale", attrs.turn_speed_scale)?;
        validate_float_field("perception_radius", attrs.perception_radius)?;
        validate_float_field("collision_radius", attrs.collision_radius)?;
        validate_float_field("engagement_distance", attrs.engagement_distance)?;
        validate_float_field("mass", attrs.mass)?;
        validate_float_field("base_health", attrs.base_health)?;
        validate_float_field("hit_reaction_force", attrs.hit_reaction_force)?;
        validate_float_field("target_awareness_range", attrs.target_awareness_range)?;
    }

    if let Some(ref timings) = parsed.timing_parameters {
        validate_float_field(
            "stumble_recovery_time_sec",
            timings.stumble_recovery_time_sec,
        )?;
        validate_float_field("attack_windup_time_sec", timings.attack_windup_time_sec)?;
        validate_float_field("attack_cooldown_time_sec", timings.attack_cooldown_time_sec)?;
        validate_float_field("block_window_time_sec", timings.block_window_time_sec)?;
        validate_float_field("invulnerability_time_sec", timings.invulnerability_time_sec)?;
    }

    let mut elements = Vec::new();

    let updated_lua = if let Some(ref script_rel) = parsed.lua_script_file
        && let Some(base_dir) = project_dir
    {
        fs::read_to_string(base_dir.join(script_rel)).ok()
    } else {
        parsed.embedded_lua_script.clone()
    };

    // Reconstruct raw unmapped components with validation
    for block in &parsed._engine_metadata.unmapped_raw_blocks {
        let mut chunk_bytes = hex::decode(&block.hex).with_context(|| {
            format!(
                "Invalid hex payload in block ID {}: '{}'",
                block.id, block.hex
            )
        })?;

        if block.id == 117
            && let Some(ref color) = parsed.lifeforce_color
        {
            chunk_bytes = update_lifeforce_color(&chunk_bytes, color);
        }

        elements.push((block.id, chunk_bytes));
    }

    if let Some(ref tag) = parsed.resource_tag {
        elements.push((20, write_length_prefixed_string(tag)));
    }

    elements.push((21, write_length_prefixed_string(&parsed.character_name)));
    elements.push((25, write_length_prefixed_string(&parsed.character_name)));

    if let Some(ref flags) = parsed.actor_flags {
        let mut mask = if let Some(ref raw_h) = parsed._engine_metadata.raw_flags_hex {
            let clean_hex = raw_h
                .trim()
                .trim_start_matches("0x")
                .trim_start_matches("0X");
            u32::from_str_radix(clean_hex, 16).with_context(|| {
                format!("Invalid hex representation in raw_flags_hex: '{}'", raw_h)
            })?
        } else {
            0x2140_0000
        };

        if flags.casts_dynamic_shadows {
            mask |= 0x2000_0000;
        } else {
            mask &= !0x2000_0000;
        }
        if flags.can_be_targeted {
            mask |= 0x0100_0000;
        } else {
            mask &= !0x0100_0000;
        }
        if flags.ragdoll_on_death {
            mask |= 0x0040_0000;
        } else {
            mask &= !0x0040_0000;
        }
        if !is_breakable && let Some(civilian) = flags.is_civilian {
            if civilian {
                mask |= 0x0000_0002;
            } else {
                mask &= !0x0000_0002;
            }
        }

        elements.push((22, mask.to_le_bytes().to_vec()));
    }

    let is_en = parsed
        .combat_attributes
        .as_ref()
        .and_then(|a| a.is_enabled)
        .unwrap_or(true);
    elements.push((23, vec![if is_en { 1 } else { 0 }]));

    if is_breakable {
        let brk = parsed.breakable_config.as_ref();
        let mat_id = brk.map(|b| b.physics_material_id).unwrap_or(2);
        let health = brk
            .map(|b| b.base_health)
            .or_else(|| {
                parsed
                    .combat_attributes
                    .as_ref()
                    .and_then(|a| a.base_health)
            })
            .unwrap_or(5.0);
        let debris_count = brk.map(|b| b.debris_pieces_count).unwrap_or(8);
        let cue_id = brk.map(|b| b.sound_cue_id).unwrap_or(112);
        let trigger = brk.is_none_or(|b| b.trigger_collapse_on_hit);
        let particles = brk.is_none_or(|b| b.spawn_debris_particles);
        let carry = brk.is_none_or(|b| b.can_be_carried);
        let center = brk.map(|b| b.center_offset).unwrap_or([0.0, 0.5, 0.0]);

        let stance_byte = parsed
            .state_and_rewards
            .as_ref()
            .and_then(|sr| sr.stance_id)
            .unwrap_or(0);
        elements.push((28, vec![stance_byte]));

        if carry {
            elements.push((29, vec![1, 40, 0, 2, 40, 0, 43, 4, 1, 1, 0, 0, 0]));
        } else {
            elements.push((29, vec![0u8]));
        }

        elements.push((41, vec![1, 1, 0, 0]));
        elements.push((42, vec![2, 0x1E, 0, 0x23, 1, 0, 0, 0, 0, 0]));
        elements.push((45, vec![1, 1, 0, 0]));
        elements.push((46, mat_id.to_le_bytes().to_vec()));

        // Synthesize block 70 (Havok debris pieces hierarchy)
        let mut blk70 = Vec::with_capacity(1 + debris_count * 2 + debris_count * 28);
        blk70.push(debris_count as u8);
        for i in 0..debris_count {
            blk70.push((40 + i) as u8);
            blk70.push((i * 28) as u8);
        }
        for _ in 0..debris_count {
            blk70.extend_from_slice(&[
                6, 0x28, 0, 0x29, 1, 0x2A, 2, 0x2B, 3, 0x2C, 7, 0x2D, 11, 0, 0, 0, 1, 0x14, 0, 0,
                1, 1, 0, 0, 1, 1, 0, 0,
            ]);
        }
        elements.push((70, blk70));

        // Block 71 (Debris impulse and physics properties)
        elements.push((
            71,
            vec![
                13, 0x29, 0, 0x2A, 7, 0x2B, 8, 0x2C, 9, 0x2D, 10, 0x64, 11, 0x65, 24, 0x66, 37,
                0x67, 50, 0x68, 63, 0x69, 76, 0x6A, 89, 0x6B, 102, 1, 10, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 4, 0x29, 0, 0x2A, 1, 0x2C, 2, 0x30, 3, 0, 0, 0, 0, 4, 0x29, 0, 0x2A, 1, 0x2C, 2,
                0x30, 3, 0, 0, 0, 0, 4, 0x29, 0, 0x2A, 1, 0x2C, 2, 0x30, 3, 0, 0, 0, 0, 4, 0x29, 0,
                0x2A, 1, 0x2C, 2, 0x30, 3, 0, 0, 0, 0, 4, 0x29, 0, 0x2A, 1, 0x2C, 2, 0x30, 3, 0, 0,
                0, 0, 4, 0x29, 0, 0x2A, 1, 0x2C, 2, 0x30, 3, 0, 0, 0, 0, 4, 0x29, 0, 0x2A, 1, 0x2C,
                2, 0x30, 3, 0, 0, 0, 0, 4, 0x29, 0, 0x2A, 1, 0x2C, 2, 0x30, 3, 0, 0, 0, 0,
            ],
        ));

        // Synthesize block 200 (Health + Lua Script)
        let lua_text = updated_lua.clone().unwrap_or_else(|| {
            format!(
                "local AliasName = GetAlias()\nCollapse(AliasName, \"{}\")",
                brk.map(|b| b.collapse_target_model.as_str()).unwrap_or("")
            )
        });

        let mut script_sub = Vec::new();
        for line in lua_text.lines() {
            let b = line.as_bytes();
            script_sub.extend_from_slice(&(b.len() as u32).to_le_bytes());
            script_sub.extend_from_slice(b);
        }

        let mut script_container = Vec::new();
        script_container.extend_from_slice(&[3, 0x15, 0, 0x16, 0x54, 0x17, 0x58, 2, 0, 0, 0]);
        script_container.extend_from_slice(&script_sub);
        script_container.extend_from_slice(&[0xBF, 0, 0, 0]);
        script_container.extend_from_slice(b"\x1bLua\x50\x01\x04\x04\x04\x06\x08\x09\x09\x08\xB6\x09\x93\x68\xE7\xF5\x7D\x41\x01\0\0\0\0\0\0\0\0\0\0\0\x04\x07\0\0\0\x01\0\0\0\x01\0\0\0\x02\0\0\0\x02\0\0\0\x02\0\0\0\x02\0\0\0\x02\0\0\0\x01\0\0\0\x0A\0\0\0AliasName\0\x02\0\0\0\x06\0\0\0\0\0\0\0\x03\0\0\0\x04\x09\0\0\0GetAlias\0\x04\x09\0\0\0Collapse\0\x04\x14\0\0\0[Props_Sewers]OBJ\\6\0\0\0\0\0\x07\0\0\0\x05\0\0\0\x99\x80\0\0\x45\0\0\x01\0\0\0\x02\x81\0\0\x03\x59\x80\x01\x01\x1B\x80\0\0\0\0\0\0");

        let mut blk200 = Vec::new();
        blk200.extend_from_slice(&[
            0x83, 4, 0, 0, 0, 10, 0, 12, 4, 13, 5, 14, 0, 0, 0, 0x21, 1, 0, 0, 19, 0, 0, 0, 0x22,
            1, 0, 0, 22, 0, 0, 0, 0x23, 1, 0, 0, 23, 0, 0, 0, 0x24, 1, 0, 0,
        ]);
        blk200.extend_from_slice(&health.to_le_bytes());
        blk200.push(0);
        blk200.extend_from_slice(&script_container);
        elements.push((200, blk200));

        elements.push((201, vec![1, 0x16, 0, 1, 0x14, 0, cue_id as u8, 0, 0, 0]));
        elements.push((202, vec![if trigger { 1 } else { 0 }, 1, 0, 0]));
        elements.push((203, vec![if particles { 1 } else { 0 }, 1, 0, 0]));

        let mut blk300 = vec![1u8, 20, 0];
        let _ = blk300.write_f32::<LittleEndian>(center[0]);
        let _ = blk300.write_f32::<LittleEndian>(center[1]);
        let _ = blk300.write_f32::<LittleEndian>(center[2]);
        elements.push((300, blk300));
        elements.push((301, vec![0u8]));
    } else {
        if let Some(ref sr) = parsed.state_and_rewards {
            if let Some(s) = sr.stance_id {
                elements.push((28, vec![s]));
            }
            if let Some(st) = sr.is_stunned {
                elements.push((120, vec![if st { 1 } else { 0 }]));
            }
            if let Some(exp) = sr.experience_reward {
                elements.push((122, exp.to_le_bytes().to_vec()));
            }
            if let Some(loot) = sr.loot_drop_multiplier {
                elements.push((123, loot.to_le_bytes().to_vec()));
            }
        }

        if let Some(ref attrs) = parsed.combat_attributes {
            if let Some(v) = attrs.move_speed_scale {
                elements.push((61, v.to_le_bytes().to_vec()));
            }
            if let Some(v) = attrs.turn_speed_scale {
                elements.push((62, v.to_le_bytes().to_vec()));
            }
            if let Some(v) = attrs.perception_radius {
                elements.push((64, v.to_le_bytes().to_vec()));
            }
            if let Some(v) = attrs.collision_radius {
                elements.push((65, v.to_le_bytes().to_vec()));
            }
            if let Some(v) = attrs.engagement_distance {
                elements.push((66, v.to_le_bytes().to_vec()));
            }
            if let Some(fid) = attrs.faction_id {
                elements.push((74, fid.to_le_bytes().to_vec()));
            }
            if let Some(active) = attrs.is_active_on_spawn {
                elements.push((77, vec![if active { 1 } else { 0 }]));
            }
            if let Some(v) = attrs.mass {
                elements.push((80, v.to_le_bytes().to_vec()));
            }
            if let Some(v) = attrs.base_health {
                elements.push((83, v.to_le_bytes().to_vec()));
            }
            if let Some(v) = attrs.hit_reaction_force {
                elements.push((121, v.to_le_bytes().to_vec()));
            }
        }

        if let Some(ref eq) = parsed.equipment {
            elements.push((29, vec![1, 40, 0, 2, 40, 0, 43, 4, 1, 1, 0, 0, 0]));
            let item_sub = vec![(20, write_length_prefixed_string(&eq.item_name))];
            let item_blob = build_typed_container(0x00464010, &item_sub);
            elements.push((63, item_blob));
        }

        if let Some(ref timings) = parsed.timing_parameters {
            if let Some(v) = timings.stumble_recovery_time_sec {
                elements.push((119, v.to_le_bytes().to_vec()));
            }
            if let Some(v) = timings.attack_windup_time_sec {
                elements.push((127, v.to_le_bytes().to_vec()));
            }
            if let Some(v) = timings.attack_cooldown_time_sec {
                elements.push((131, v.to_le_bytes().to_vec()));
            }
            if let Some(v) = timings.block_window_time_sec {
                elements.push((132, v.to_le_bytes().to_vec()));
            }
            if let Some(v) = timings.invulnerability_time_sec {
                elements.push((133, v.to_le_bytes().to_vec()));
            }
        }

        elements.push((88, vec![1, 1, 0, 0]));
        elements.push((92, vec![0, 0, 0, 0]));
        elements.push((96, vec![0]));
        elements.push((97, vec![0]));
        elements.push((98, vec![0]));
        elements.push((102, vec![0, 0, 0, 0]));
        elements.push((111, vec![0xFF, 0xFF, 0xFF, 0xFF]));
        elements.push((114, vec![1, 0x22, 0, 0, 0, 0, 0x40]));
        elements.push((134, vec![1]));
        elements.push((167, vec![0]));

        if let Some(ref lua_code) = updated_lua
            && !elements.iter().any(|(id, _)| *id == 115)
        {
            elements.push((115, rebuild_lua_component(&[], lua_code)));
        }
    }

    if let Some(ref mb) = parsed.model_binding {
        let binding_id = if is_breakable { 31 } else { 67 };
        elements.push((
            binding_id,
            build_model_binding(&mb.object_path, &mb.model_name),
        ));
    }

    elements.push((19, vec![0xFF, 0xFF, 0xFF, 0xFF]));
    elements.push((1, vec![0]));

    if let Some(ref fxa) = parsed.facefx_actor {
        elements.push((139, write_length_prefixed_string(fxa)));
    }

    if let Some(ref fxe_rel) = parsed.embedded_facefx_file
        && let Some(base_dir) = project_dir
        && let Ok(new_fxe) = fs::read(base_dir.join(fxe_rel))
    {
        elements.push((166, rebuild_embedded_facefx(&[], &new_fxe)));
    }

    elements.sort_by_key(|(id, _)| *id);
    Ok(build_typed_container(type_id, &elements))
}

// -----------------------------------------------------------------------------
// Internal Sub-container Parsers & Builders
// -----------------------------------------------------------------------------

fn parse_f32_safe(chunk: &[u8]) -> Option<f32> {
    if chunk.len() >= 4 {
        let val = f32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
        if val.is_finite() {
            return Some(val);
        }
    }
    None
}

fn parse_equipment_definition(elements: &[(u32, Vec<u8>)]) -> Option<CharacterEquipmentJson> {
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

fn parse_morph_parameters(chunk: &[u8]) -> (Option<f32>, Option<f32>) {
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

fn parse_model_binding(data: &[u8]) -> Option<CharacterModelBinding> {
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

fn build_model_binding(obj_path: &str, model_name: &str) -> Vec<u8> {
    let elements = vec![
        (20, write_length_prefixed_string(obj_path)),
        (21, write_length_prefixed_string(model_name)),
    ];
    build_chunk_from_elements(false, &elements)
}

fn parse_transformation_container(data: &[u8]) -> Vec<CharacterTransformationJson> {
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

fn parse_animation_graph(data: &[u8]) -> Vec<CharacterAnimStateJson> {
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

fn parse_ai_behaviors(data: &[u8]) -> Vec<String> {
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

fn extract_embedded_lua_and_bytecode(data: &[u8]) -> Option<(String, Vec<u8>)> {
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

fn rebuild_lua_component(original_bytes: &[u8], new_lua: &str) -> Vec<u8> {
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

fn extract_embedded_facefx(data: &[u8]) -> Option<Vec<u8>> {
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

fn rebuild_embedded_facefx(_original_chunk: &[u8], new_fxe_bytes: &[u8]) -> Vec<u8> {
    let size_bytes = (new_fxe_bytes.len() as u32).to_le_bytes().to_vec();
    let sub_elements = vec![(10, size_bytes), (11, new_fxe_bytes.to_vec())];
    build_chunk_from_elements(false, &sub_elements)
}

fn parse_all_effect_receptors(
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

fn update_lifeforce_color(data: &[u8], new_color: &str) -> Vec<u8> {
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
