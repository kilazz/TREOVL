pub mod actor;
pub mod breakable;

pub use actor::*;
pub use breakable::*;

use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;
use std::path::Path;

use crate::engine::assets::{
    build_typed_container_with_endian, parse_chunk_elements, parse_typed_container,
};
use crate::engine::common::{Endian, read_length_prefixed_string};

/// Result of in-memory character extraction without filesystem side-effects
pub struct ExtractedCharacter {
    pub character: CharacterActorJson,
    pub embedded_lua_source: Option<String>,
    pub embedded_lua_bytecode: Vec<u8>,
    pub embedded_facefx: Option<Vec<u8>>,
}

/// Pure in-memory extraction of a character actor or breakable chunk
pub fn export_character(data: &[u8], _stem: &str) -> Result<ExtractedCharacter> {
    if data.len() < 5 {
        bail!("Data too short for a Character / Actor container");
    }

    let (type_id, elements) =
        parse_typed_container(data).context("Failed to parse Character typed container")?;

    let is_breakable = type_id == 0x00463018;

    let mut character_name = String::from("Unnamed_Character");
    let mut resource_tag = None;
    let mut is_baby = None;
    let mut model_binding = None;
    let mut collapse_target_model = None;
    let mut facefx_actor = None;
    let mut embedded_facefx_bytes = None;
    let mut lifeforce_color = None;
    let mut embedded_lua_script = None;
    let mut embedded_lua_bytecode = Vec::new();
    let mut animation_states = Vec::new();
    let mut ai_behaviors = Vec::new();
    let mut transformations = Vec::new();
    let mut effect_receptors = Vec::new();
    let mut minion_grapple_bones = Vec::new();
    let mut attributes = CharacterAttributesJson::default();
    let mut flags = CharacterFlagsJson::default();
    let mut raw_flags_hex_val = None;
    let mut timings = CharacterCombatTimingsJson::default();
    let mut knockback = None;
    let mut state_and_rewards = CharacterStateAndRewardsJson::default();
    let mut morph_params = CharacterMorphParamsJson::default();
    let mut equipment = None;
    let mut unmapped_raw_blocks = Vec::new();

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
            91 if chunk.len() >= 4 => {
                attributes.jump_force = parse_f32_safe(chunk);
            }
            112 => {
                ai_behaviors = parse_ai_behaviors(chunk);
            }
            115 | 200 => {
                if let Some((lua_source, bytecode)) = extract_embedded_lua_and_bytecode(chunk) {
                    collapse_target_model = extract_collapse_target_from_script(&lua_source);
                    embedded_lua_script = Some(lua_source);
                    embedded_lua_bytecode = bytecode;
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
                let dist = parse_f32_safe(&chunk[18..22]);
                let height = parse_f32_safe(&chunk[22..26]);
                let is_clean = |v: Option<f32>| v.is_some_and(|f| (0.001..=500.0).contains(&f));
                if is_clean(dist) && is_clean(height) {
                    knockback = Some(CharacterKnockbackJson {
                        knockback_distance: dist,
                        knockback_arc_height: height,
                    });
                }
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
            137 if chunk.len() >= 4 => {
                attributes.explosion_damage = parse_f32_safe(chunk);
            }
            139 => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    facefx_actor = Some(s);
                }
            }
            140 if chunk.len() >= 4 => {
                attributes.aggro_range = parse_f32_safe(chunk);
            }
            146 => {
                transformations = parse_transformation_container(chunk);
            }
            149 => {
                let (duration, scale) = parse_morph_parameters(chunk);
                morph_params.morph_duration_sec = duration;
                morph_params.blend_scale = scale;
            }
            155 if chunk.len() >= 4 => {
                attributes.damage_multiplier = parse_f32_safe(chunk);
            }
            162 if !chunk.is_empty() => {
                is_baby = Some(chunk[0] != 0);
            }
            166 => {
                if let Some(fxe_bytes) = extract_embedded_facefx(chunk) {
                    embedded_facefx_bytes = Some(fxe_bytes);
                }
            }
            88 | 92 | 96 | 97 | 98 | 102 | 111 | 114 | 128 | 129 | 134 | 144 | 147 | 148 | 152
            | 153 | 159 | 161 | 167 | 19 | 1 | 41 | 42 | 45 | 71 | 301 => {}
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

    let embedded_lua_source = embedded_lua_script.clone();

    let character = CharacterActorJson {
        _engine_metadata: metadata,
        character_name,
        resource_tag,
        is_baby,
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
        knockback_parameters: knockback,
        state_and_rewards: Some(state_and_rewards),
        morph_parameters: Some(morph_params),
        equipment,
        facefx_actor,
        embedded_facefx_file: None,
        lifeforce_color,
        lua_script_file: None,
        embedded_lua_script,
        animation_states,
        ai_behaviors,
        transformations,
        effect_receptors,
        minion_grapple_bones,
    };

    Ok(ExtractedCharacter {
        character,
        embedded_lua_source,
        embedded_lua_bytecode,
        embedded_facefx: embedded_facefx_bytes,
    })
}

/// Pure in-memory export to formatted JSON string without writing companion files
pub fn export_character_to_json(data: &[u8], stem: &str) -> Result<String> {
    let extracted = export_character(data, stem)?;
    serde_json::to_string_pretty(&extracted.character).map_err(|e| anyhow::anyhow!(e))
}

pub fn import_character_from_json(json_str: &str) -> Result<Vec<u8>> {
    import_character_from_json_with_externals(json_str, None, None, Endian::Little)
}

pub fn import_character_from_json_files(
    json_str: &str,
    project_dir: Option<&Path>,
    endian: Endian,
) -> Result<Vec<u8>> {
    let parsed: CharacterActorJson = serde_json::from_str(json_str)?;

    let external_lua = if let Some(ref script_rel) = parsed.lua_script_file
        && let Some(base_dir) = project_dir
    {
        std::fs::read_to_string(base_dir.join(script_rel)).ok()
    } else {
        None
    };

    let external_facefx = if let Some(ref fxe_rel) = parsed.embedded_facefx_file
        && let Some(base_dir) = project_dir
    {
        std::fs::read(base_dir.join(fxe_rel)).ok()
    } else {
        None
    };

    import_character_from_json_with_externals(
        json_str,
        external_lua.as_deref(),
        external_facefx.as_deref(),
        endian,
    )
}

pub fn import_character_from_json_with_externals(
    json_str: &str,
    external_lua: Option<&str>,
    external_facefx: Option<&[u8]>,
    endian: Endian,
) -> Result<Vec<u8>> {
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
    let mut elements = Vec::new();

    let updated_lua = external_lua
        .map(|s| s.to_string())
        .or_else(|| parsed.embedded_lua_script.clone());

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
        elements.push((20, endian.write_length_prefixed_string(tag)));
    }

    elements.push((
        21,
        endian.write_length_prefixed_string(&parsed.character_name),
    ));
    elements.push((
        25,
        endian.write_length_prefixed_string(&parsed.character_name),
    ));

    if let Some(baby) = parsed.is_baby {
        elements.push((162, vec![if baby { 1 } else { 0 }]));
    }

    if let Some(ref flags) = parsed.actor_flags {
        let mut mask = if let Some(ref raw_h) = parsed._engine_metadata.raw_flags_hex {
            let clean_hex = raw_h
                .trim()
                .trim_start_matches("0x")
                .trim_start_matches("0X");
            u32::from_str_radix(clean_hex, 16).unwrap_or(0x2140_0000)
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

        elements.push((22, endian.u32_to_bytes(mask).to_vec()));
    }

    let is_en = parsed
        .combat_attributes
        .as_ref()
        .and_then(|a| a.is_enabled)
        .unwrap_or(true);
    elements.push((23, vec![if is_en { 1 } else { 0 }]));

    if is_breakable {
        let stance_byte = parsed
            .state_and_rewards
            .as_ref()
            .and_then(|sr| sr.stance_id)
            .unwrap_or(0);
        elements.push((28, vec![stance_byte]));

        let base_health = parsed
            .combat_attributes
            .as_ref()
            .and_then(|a| a.base_health);
        let breakable_elems = build_breakable_blocks(
            parsed.breakable_config.as_ref(),
            base_health,
            updated_lua.as_deref(),
            endian,
        );
        elements.extend(breakable_elems);
    } else {
        if let Some(ref sr) = parsed.state_and_rewards {
            if let Some(s) = sr.stance_id {
                elements.push((28, vec![s]));
            }
            if let Some(st) = sr.is_stunned {
                elements.push((120, vec![if st { 1 } else { 0 }]));
            }
            if let Some(exp) = sr.experience_reward {
                elements.push((122, endian.u32_to_bytes(exp).to_vec()));
            }
            if let Some(loot) = sr.loot_drop_multiplier {
                elements.push((123, endian.u32_to_bytes(loot).to_vec()));
            }
        }

        if let Some(ref attrs) = parsed.combat_attributes {
            if let Some(v) = attrs.move_speed_scale {
                elements.push((61, endian.f32_to_bytes(v).to_vec()));
            }
            if let Some(v) = attrs.turn_speed_scale {
                elements.push((62, endian.f32_to_bytes(v).to_vec()));
            }
            if let Some(v) = attrs.perception_radius {
                elements.push((64, endian.f32_to_bytes(v).to_vec()));
            }
            if let Some(v) = attrs.collision_radius {
                elements.push((65, endian.f32_to_bytes(v).to_vec()));
            }
            if let Some(v) = attrs.engagement_distance {
                elements.push((66, endian.f32_to_bytes(v).to_vec()));
            }
            if let Some(fid) = attrs.faction_id {
                elements.push((74, endian.u32_to_bytes(fid).to_vec()));
            }
            if let Some(active) = attrs.is_active_on_spawn {
                elements.push((77, vec![if active { 1 } else { 0 }]));
            }
            if let Some(v) = attrs.mass {
                elements.push((80, endian.f32_to_bytes(v).to_vec()));
            }
            if let Some(v) = attrs.base_health {
                elements.push((83, endian.f32_to_bytes(v).to_vec()));
            }
            if let Some(v) = attrs.jump_force {
                elements.push((91, endian.f32_to_bytes(v).to_vec()));
            }
            if let Some(v) = attrs.hit_reaction_force {
                elements.push((121, endian.f32_to_bytes(v).to_vec()));
            }
            if let Some(v) = attrs.explosion_damage {
                elements.push((137, endian.f32_to_bytes(v).to_vec()));
            }
            if let Some(v) = attrs.aggro_range {
                elements.push((140, endian.f32_to_bytes(v).to_vec()));
            }
            if let Some(v) = attrs.damage_multiplier {
                elements.push((155, endian.f32_to_bytes(v).to_vec()));
            }
        }

        if let Some(ref eq) = parsed.equipment {
            elements.push((29, vec![1, 40, 0, 2, 40, 0, 43, 4, 1, 1, 0, 0, 0]));
            let item_sub = vec![(20, endian.write_length_prefixed_string(&eq.item_name))];
            let item_blob = build_typed_container_with_endian(0x00464010, &item_sub, endian);
            elements.push((63, item_blob));
        }

        if let Some(ref timings) = parsed.timing_parameters {
            if let Some(v) = timings.stumble_recovery_time_sec {
                elements.push((119, endian.f32_to_bytes(v).to_vec()));
            }
            if let Some(v) = timings.attack_windup_time_sec {
                elements.push((127, endian.f32_to_bytes(v).to_vec()));
            }
            if let Some(v) = timings.attack_cooldown_time_sec {
                elements.push((131, endian.f32_to_bytes(v).to_vec()));
            }
            if let Some(v) = timings.block_window_time_sec {
                elements.push((132, endian.f32_to_bytes(v).to_vec()));
            }
            if let Some(v) = timings.invulnerability_time_sec {
                elements.push((133, endian.f32_to_bytes(v).to_vec()));
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
        elements.push((144, vec![0, 0, 0x80, 0x3F]));
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
            build_model_binding(&mb.object_path, &mb.model_name, endian),
        ));
    }

    elements.push((19, vec![0xFF, 0xFF, 0xFF, 0xFF]));
    elements.push((1, vec![0]));

    if let Some(ref fxa) = parsed.facefx_actor {
        elements.push((139, endian.write_length_prefixed_string(fxa)));
    }

    if let Some(new_fxe) = external_facefx {
        elements.push((166, rebuild_embedded_facefx(&[], new_fxe, endian)));
    }

    elements.sort_by_key(|(id, _)| *id);
    Ok(build_typed_container_with_endian(
        type_id, &elements, endian,
    ))
}
