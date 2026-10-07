pub mod model;
pub mod physics;
pub mod placement;

pub use model::*;
pub use physics::*;
pub use placement::*;

use anyhow::{Context, Result, bail};

use crate::engine::assets::{build_typed_container_with_endian, parse_typed_container};
use crate::engine::common::Endian;

pub struct ExtractedObject {
    pub entity: ObjectEntityJson,
    pub bones: Vec<FullObjectBoneJson>,
}

pub fn export_object(data: &[u8]) -> Result<ExtractedObject> {
    if data.len() < 5 {
        bail!("Chunk data too short to be an Object Entity container.");
    }

    let (type_id, elements) = parse_typed_container(data)
        .context("Failed to parse Object Entity root typed container")?;

    let is_placement_object = type_id == 0x00464621;
    let class_name = match type_id {
        0x0041004B => Some("TREModelResource".to_string()),
        0x00464621 => Some("TREPlacementObject".to_string()),
        _ => None,
    };

    let mut group_tag = None;
    let mut entity_name = None;
    let mut scale = None;
    let mut bounding_box = None;
    let mut default_animation = None;
    let mut physics_state = None;
    let mut mesh_bindings = Vec::new();
    let mut bones = Vec::new();
    let mut ragdoll_bone_groups = Vec::new();
    let mut attachments = Vec::new();
    let mut has_sentinel_terminator = false;
    let mut raw_fallbacks = Vec::new();

    let mut stand_model = None;
    let mut placed_object = None;
    let mut placement_offset = None;
    let mut pl_material_id = 2u32;
    let mut pl_is_enabled = true;
    let mut pl_casts_shadows = true;
    let mut pl_can_be_carried = true;
    let mut pl_flags_hex = None;

    for (id, chunk) in &elements {
        match *id {
            20 => {
                if let Some(s) = crate::engine::common::read_length_prefixed_string(chunk) {
                    group_tag = Some(s);
                } else {
                    raw_fallbacks.push(RawFallbackComponentJson {
                        id: *id,
                        reason: "Corrupted string format".into(),
                        hex: hex::encode_upper(chunk),
                    });
                }
            }
            21 => {
                if let Some(s) = crate::engine::common::read_length_prefixed_string(chunk) {
                    entity_name = Some(s);
                } else {
                    raw_fallbacks.push(RawFallbackComponentJson {
                        id: *id,
                        reason: "Corrupted string format".into(),
                        hex: hex::encode_upper(chunk),
                    });
                }
            }
            22 if is_placement_object && chunk.len() >= 4 => {
                let mask = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
                pl_casts_shadows = (mask & 0x2000_0000) != 0;
                pl_flags_hex = Some(format!("0x{:08X}", mask));
            }
            23 if is_placement_object && !chunk.is_empty() => {
                pl_is_enabled = chunk[0] != 0;
            }
            29 if is_placement_object => {
                pl_can_be_carried = chunk.len() >= 4 && chunk[0] != 0;
            }
            31 if is_placement_object => {
                stand_model = parse_simple_model_binding(chunk);
            }
            32 => {
                if let Some(s) = read_scale_vector(chunk) {
                    scale = Some(s);
                } else {
                    raw_fallbacks.push(RawFallbackComponentJson {
                        id: *id,
                        reason: "Invalid scale vector (expected 12 bytes / 3 floats)".into(),
                        hex: hex::encode_upper(chunk),
                    });
                }
            }
            33 => {
                if !is_empty_container(chunk) {
                    let parsed_bones = parse_full_bones_container(chunk);
                    if !parsed_bones.is_empty() {
                        bones = parsed_bones;
                    } else {
                        raw_fallbacks.push(RawFallbackComponentJson {
                            id: *id,
                            reason: "Non-standard bone container format".into(),
                            hex: hex::encode_upper(chunk),
                        });
                    }
                }
            }
            42 if is_placement_object => {
                placed_object = parse_placed_object(chunk);
            }
            46 if is_placement_object && chunk.len() >= 4 => {
                pl_material_id = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
            }
            300 if is_placement_object && chunk.len() >= 15 => {
                placement_offset = parse_placement_offset(chunk);
            }
            19 => {
                has_sentinel_terminator = true;
            }
            _ => {}
        }
    }

    for (id, chunk) in &elements {
        match *id {
            30 => {
                let bindings = parse_mesh_material_bindings(chunk);
                if !bindings.is_empty() || is_empty_container(chunk) {
                    mesh_bindings = bindings;
                } else {
                    raw_fallbacks.push(RawFallbackComponentJson {
                        id: *id,
                        reason: "Failed to parse mesh-material bindings table".into(),
                        hex: hex::encode_upper(chunk),
                    });
                }
            }
            34 => {
                if let Some(bbox) = parse_bounding_box(chunk) {
                    bounding_box = Some(bbox);
                } else {
                    raw_fallbacks.push(RawFallbackComponentJson {
                        id: *id,
                        reason: "Non-standard bounding box format (expected 60 bytes)".into(),
                        hex: hex::encode_upper(chunk),
                    });
                }
            }
            35 => {
                if !is_empty_container(chunk) {
                    let groups = parse_ragdoll_bone_groups(chunk, &bones);
                    if !groups.is_empty() {
                        ragdoll_bone_groups = groups;
                    } else {
                        raw_fallbacks.push(RawFallbackComponentJson {
                            id: *id,
                            reason: "Failed to decode ragdoll bone groups container".into(),
                            hex: hex::encode_upper(chunk),
                        });
                    }
                }
            }
            36 => {
                if !is_empty_container(chunk) {
                    if let Some(anim) = parse_animation_linkage(chunk) {
                        default_animation = Some(anim);
                    } else {
                        raw_fallbacks.push(RawFallbackComponentJson {
                            id: *id,
                            reason: "Unrecognized animation linkage format".into(),
                            hex: hex::encode_upper(chunk),
                        });
                    }
                }
            }
            37 => {
                if chunk.len() == 16 {
                    physics_state = parse_physics_state(chunk);
                } else if let Some(anim) = parse_animation_linkage(chunk) {
                    default_animation = Some(anim);
                } else if !chunk.is_empty() {
                    raw_fallbacks.push(RawFallbackComponentJson {
                        id: *id,
                        reason: "Non-standard physics/state component".into(),
                        hex: hex::encode_upper(chunk),
                    });
                }
            }
            1 => {
                attachments = parse_attachment_slots(chunk);
            }
            20 | 21 | 32 | 33 | 19 => {}
            22 | 23 | 28 | 29 | 31 | 38 | 41 | 42 | 44 | 45 | 46 | 300 | 301
                if is_placement_object => {}
            _ => {
                raw_fallbacks.push(RawFallbackComponentJson {
                    id: *id,
                    reason: "Unmapped / unknown engine component".into(),
                    hex: hex::encode_upper(chunk),
                });
            }
        }
    }

    let metadata = ObjectEngineMetadataJson {
        type_id_hex: format!("{:08X}", type_id),
        class_name,
        raw_fallbacks,
    };

    let placement_config = if is_placement_object {
        Some(PlacementConfigJson {
            physics_material_id: pl_material_id,
            is_enabled: pl_is_enabled,
            casts_shadows: pl_casts_shadows,
            can_be_carried: pl_can_be_carried,
            raw_flags_hex: pl_flags_hex,
        })
    } else {
        None
    };

    let entity = ObjectEntityJson {
        _engine_metadata: metadata,
        group_tag,
        entity_name,
        scale,
        bounding_box,
        default_animation,
        physics_state,
        ragdoll_bone_groups,
        mesh_bindings,
        stand_model,
        placed_object,
        placement_offset,
        placement_config,
        bones: bones.clone(),
        attachments,
        has_sentinel_terminator,
    };

    Ok(ExtractedObject { entity, bones })
}

pub fn export_object_to_json(data: &[u8]) -> Result<String> {
    let extracted = export_object(data)?;
    serde_json::to_string_pretty(&extracted.entity).map_err(|e| anyhow::anyhow!(e))
}

pub fn import_object_from_json(json_str: &str) -> Result<Vec<u8>> {
    import_object_from_json_with_endian(json_str, Endian::Little)
}

pub fn import_object_from_json_with_endian(json_str: &str, endian: Endian) -> Result<Vec<u8>> {
    let parsed: ObjectEntityJson = serde_json::from_str(json_str)?;
    let type_id = u32::from_str_radix(&parsed._engine_metadata.type_id_hex, 16)
        .context("Invalid TypeID hex in Object JSON metadata")?;

    let is_placement_object = type_id == 0x00464621;
    let mut elements = Vec::new();

    let get_fallback = |id: u32| -> Option<Vec<u8>> {
        parsed
            ._engine_metadata
            .raw_fallbacks
            .iter()
            .find(|fb| fb.id == id)
            .and_then(|fb| hex::decode(&fb.hex).ok())
    };

    if let Some(ref tag) = parsed.group_tag {
        elements.push((20, endian.write_length_prefixed_string(tag)));
    } else if let Some(raw) = get_fallback(20) {
        elements.push((20, raw));
    }

    if let Some(ref name) = parsed.entity_name {
        elements.push((21, endian.write_length_prefixed_string(name)));
    } else if let Some(raw) = get_fallback(21) {
        elements.push((21, raw));
    }

    if is_placement_object {
        let pl = parsed.placement_config.as_ref();
        let mat_id = pl.map(|p| p.physics_material_id).unwrap_or(2);
        let enabled = pl.is_none_or(|p| p.is_enabled);
        let carry = pl.is_none_or(|p| p.can_be_carried);
        let shadows = pl.is_none_or(|p| p.casts_shadows);

        let mut mask = if let Some(raw_h) = pl.and_then(|p| p.raw_flags_hex.as_ref()) {
            let clean = raw_h
                .trim()
                .trim_start_matches("0x")
                .trim_start_matches("0X");
            u32::from_str_radix(clean, 16).unwrap_or(0x2440_000C)
        } else {
            0x2440_000C
        };
        if shadows {
            mask |= 0x2000_0000;
        } else {
            mask &= !0x2000_0000;
        }
        elements.push((22, endian.u32_to_bytes(mask).to_vec()));

        elements.push((23, vec![if enabled { 1 } else { 0 }]));
        elements.push((28, vec![0u8]));

        if carry {
            elements.push((29, vec![1, 40, 0, 3, 40, 0, 43, 4, 44, 5, 1, 1, 0, 0, 0, 1]));
        } else {
            elements.push((29, vec![0u8]));
        }

        if let Some(ref stand) = parsed.stand_model {
            elements.push((
                31,
                build_simple_model_binding(&stand.object_path, &stand.model_name, endian),
            ));
        } else if let Some(raw) = get_fallback(31) {
            elements.push((31, raw));
        }

        elements.push((38, vec![1u8]));
        elements.push((41, vec![1, 1, 0, 0]));

        if let Some(ref placed) = parsed.placed_object {
            elements.push((42, build_placed_object(placed, endian)));
        } else if let Some(raw) = get_fallback(42) {
            elements.push((42, raw));
        }

        elements.push((44, vec![0u8]));
        elements.push((45, vec![1, 1, 0, 0]));
        elements.push((46, endian.u32_to_bytes(mat_id).to_vec()));

        if let Some(offset) = parsed.placement_offset {
            elements.push((300, build_placement_offset(offset, endian)));
        } else if let Some(raw) = get_fallback(300) {
            elements.push((300, raw));
        } else {
            elements.push((300, build_placement_offset([0.0, 0.5, 0.0], endian)));
        }
        elements.push((301, vec![0u8]));
    } else {
        if !parsed.mesh_bindings.is_empty() {
            let chunk_30 = rebuild_mesh_material_bindings(&parsed.mesh_bindings, endian)?;
            elements.push((30, chunk_30));
        } else if let Some(raw) = get_fallback(30) {
            elements.push((30, raw));
        }

        if let Some(s) = parsed.scale {
            elements.push((32, write_scale_vector(s, endian)));
        } else if let Some(raw) = get_fallback(32) {
            elements.push((32, raw));
        }

        if !parsed.bones.is_empty() {
            let chunk_33 = rebuild_full_bones_container(&parsed.bones, endian)?;
            elements.push((33, chunk_33));
        } else if let Some(raw) = get_fallback(33) {
            elements.push((33, raw));
        } else {
            elements.push((33, vec![0u8]));
        }

        if let Some(ref bbox) = parsed.bounding_box {
            elements.push((34, rebuild_bounding_box(bbox, endian)));
        } else if let Some(raw) = get_fallback(34) {
            elements.push((34, raw));
        }

        if !parsed.ragdoll_bone_groups.is_empty() {
            let chunk_35 = rebuild_ragdoll_bone_groups(&parsed.ragdoll_bone_groups, endian)?;
            elements.push((35, chunk_35));
        } else if let Some(raw) = get_fallback(35) {
            elements.push((35, raw));
        }

        if let Some(ref anim) = parsed.default_animation {
            elements.push((36, rebuild_animation_linkage(anim, endian)));
        } else if let Some(raw) = get_fallback(36) {
            elements.push((36, raw));
        } else {
            elements.push((36, vec![0u8]));
        }

        if let Some(ref phys) = parsed.physics_state {
            elements.push((37, rebuild_physics_state(phys, endian)));
        } else if let Some(raw) = get_fallback(37) {
            elements.push((37, raw));
        }
    }

    if parsed.has_sentinel_terminator {
        elements.push((19, vec![0xFF, 0xFF, 0xFF, 0xFF]));
    }

    if !parsed.attachments.is_empty() {
        elements.push((1, rebuild_attachment_slots(&parsed.attachments, endian)));
    } else if let Some(raw) = get_fallback(1) {
        elements.push((1, raw));
    } else {
        elements.push((1, vec![0u8]));
    }

    for fb in &parsed._engine_metadata.raw_fallbacks {
        if ![
            20, 21, 30, 32, 33, 34, 35, 36, 37, 19, 1, 22, 23, 28, 29, 31, 38, 41, 42, 44, 45, 46,
            300, 301,
        ]
        .contains(&fb.id)
            && let Ok(raw) = hex::decode(&fb.hex)
        {
            elements.push((fb.id, raw));
        }
    }

    elements.sort_by_key(|&(id, _)| id);
    Ok(build_typed_container_with_endian(
        type_id, &elements, endian,
    ))
}
