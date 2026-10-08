pub mod model;
pub mod physics;
pub mod placement;

pub use model::*;
pub use physics::*;
pub use placement::*;

use anyhow::{Context, Result, bail};

use crate::engine::assets::{
    build_typed_container_with_endian, parse_chunk_elements, parse_typed_container,
};
use crate::engine::common::{
    Endian, EntityHandleJson, ObjectChunkId, ObjectTypeId, parse_entity_handle,
};

pub struct ExtractedObject {
    pub entity: ObjectEntityJson,
    pub bones: Vec<FullObjectBoneJson>,
}

struct ObjectEntityBuilder {
    entity: ObjectEntityJson,
    bones: Vec<FullObjectBoneJson>,
    type_id: ObjectTypeId,
    pl_material_id: u32,
    pl_is_enabled: bool,
    pl_casts_shadows: bool,
    pl_can_be_carried: bool,
    pl_entity_handle: Option<EntityHandleJson>,
    pl_state_count: Option<u32>,
    pl_default_state: Option<u32>,
    pl_trigger_active: Option<bool>,
    pl_stance_id: Option<u8>,
    pl_interaction_flags: Option<String>,
    pl_secondary_flags: Option<String>,
    pl_flags_hex: Option<String>,
}

impl ComponentParser for ObjectEntityBuilder {
    fn parse_chunk(&mut self, id: u32, chunk: &[u8]) -> Result<()> {
        let chunk_id = ObjectChunkId::from(id);

        let is_door = self.type_id == ObjectTypeId::DoorController;
        let is_point_light = self.type_id == ObjectTypeId::PointLight;
        let is_placement_object = matches!(
            self.type_id,
            ObjectTypeId::PlacementObject
                | ObjectTypeId::SoundMarker
                | ObjectTypeId::PushableWheel1
                | ObjectTypeId::PushableWheel2
                | ObjectTypeId::LogicMarker
                | ObjectTypeId::PointLight
                | ObjectTypeId::MinionGate
                | ObjectTypeId::UpgradePortal
                | ObjectTypeId::DoorController
                | ObjectTypeId::LightMarker
                | ObjectTypeId::Mechanism
        );

        match chunk_id {
            ObjectChunkId::GroupTag => {
                self.entity.group_tag = crate::engine::common::read_length_prefixed_string(chunk);
            }
            ObjectChunkId::EntityName => {
                if self.entity.entity_name.is_none() {
                    self.entity.entity_name =
                        crate::engine::common::read_length_prefixed_string(chunk);
                }
            }
            ObjectChunkId::DisplayName => {
                if let Some(s) = crate::engine::common::read_length_prefixed_string(chunk) {
                    self.entity.entity_name = Some(s);
                }
            }
            ObjectChunkId::Flags => {
                if chunk.len() >= 4 {
                    let mask = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
                    let handle = parse_entity_handle(mask);
                    if handle.is_some() {
                        self.entity.entity_handle = handle.clone();
                        self.pl_entity_handle = handle;
                    }
                    if is_placement_object {
                        self.pl_casts_shadows = (mask & 0x2000_0000) != 0;
                        self.pl_flags_hex = Some(format!("0x{:08X}", mask));
                    }
                }
            }
            ObjectChunkId::Enabled => {
                if is_placement_object && !chunk.is_empty() {
                    self.pl_is_enabled = chunk[0] != 0;
                }
            }
            ObjectChunkId::StanceId => {
                if is_placement_object && !chunk.is_empty() {
                    self.pl_stance_id = Some(chunk[0]);
                }
            }
            ObjectChunkId::CanBeCarried => {
                if is_placement_object {
                    self.pl_can_be_carried = chunk.len() >= 4 && chunk[0] != 0;
                }
            }
            ObjectChunkId::StandModel => {
                if is_placement_object {
                    self.entity.stand_model = parse_simple_model_binding(chunk);
                }
            }
            ObjectChunkId::Scale => {
                self.entity.scale = Some(read_scale_vector(chunk)?);
            }
            ObjectChunkId::Bones => {
                if !is_empty_container(chunk) && !is_door {
                    self.bones = parse_full_bones_container(chunk);
                }
            }
            ObjectChunkId::BoundingBox => {
                if !is_door {
                    self.entity.bounding_box = parse_bounding_box(chunk);
                }
            }
            ObjectChunkId::InteractionActions41 => {
                if is_placement_object {
                    let acts = parse_interaction_actions_safe(chunk);
                    if !acts.is_empty() {
                        self.entity.interaction_actions.extend(acts);
                    } else if self.type_id == ObjectTypeId::LogicMarker {
                        self.entity.stand_model = parse_simple_model_binding(chunk);
                    } else {
                        self.pl_interaction_flags = Some(hex::encode_upper(chunk));
                    }
                }
            }
            ObjectChunkId::PlacedObject => {
                if is_placement_object {
                    self.entity.placed_object = parse_placed_object(chunk);
                }
            }
            ObjectChunkId::TriggerActive => {
                if self.type_id == ObjectTypeId::LogicMarker && !chunk.is_empty() {
                    self.pl_trigger_active = Some(chunk[0] != 0);
                }
            }
            ObjectChunkId::SecondaryFlags => {
                if is_placement_object {
                    self.pl_secondary_flags = Some(hex::encode_upper(chunk));
                }
            }
            ObjectChunkId::MaterialId => {
                if is_placement_object && chunk.len() >= 4 {
                    self.pl_material_id =
                        u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
                }
            }
            ObjectChunkId::LogicEventLink => {
                if self.type_id == ObjectTypeId::LogicMarker {
                    if let Ok((_, sub_elems)) = parse_chunk_elements(chunk) {
                        let mut target_event = String::new();
                        let mut raw_hex_id = None;
                        let mut linked_environment_handle = None;

                        for (sid, sdata) in sub_elems {
                            if sid == 20 {
                                if let Some(s) =
                                    crate::engine::common::read_length_prefixed_string(&sdata)
                                {
                                    target_event = s;
                                }
                            } else if sid == 19 {
                                raw_hex_id = Some(hex::encode_upper(&sdata));
                                if sdata.len() >= 4 {
                                    let val = u32::from_le_bytes(
                                        sdata[0..4].try_into().unwrap_or_default(),
                                    );
                                    if let Some(h) = parse_entity_handle(val) {
                                        linked_environment_handle = Some(h.uid);
                                    }
                                }
                            }
                        }
                        if !target_event.is_empty() {
                            self.entity.logic_event_link = Some(LogicEventLinkJson {
                                target_event,
                                raw_hex_id,
                                linked_environment_handle,
                            });
                        }
                    }
                } else if is_door {
                    self.entity.stand_model = parse_simple_model_binding(chunk);
                }
            }
            ObjectChunkId::DoorStateCount => {
                if is_door && chunk.len() >= 4 {
                    self.pl_state_count = Some(u32::from_le_bytes(
                        chunk[0..4].try_into().unwrap_or_default(),
                    ));
                }
            }
            ObjectChunkId::PointLightParams => {
                if is_door {
                    self.entity.door_states = parse_door_states(chunk);
                } else if is_point_light {
                    self.entity.point_light = Some(model::parse_point_light_params(chunk)?);
                } else {
                    let acts = parse_interaction_actions_safe(chunk);
                    if !acts.is_empty() {
                        self.entity.interaction_actions.extend(acts);
                    } else {
                        self.entity
                            ._engine_metadata
                            .raw_fallbacks
                            .push(RawFallbackComponentJson {
                                id,
                                reason: "Unmapped component".into(),
                                hex: hex::encode_upper(chunk),
                            });
                    }
                }
            }
            ObjectChunkId::DoorDefaultState => {
                if is_door && chunk.len() >= 4 {
                    self.pl_default_state = Some(u32::from_le_bytes(
                        chunk[0..4].try_into().unwrap_or_default(),
                    ));
                } else {
                    let acts = parse_interaction_actions_safe(chunk);
                    if !acts.is_empty() {
                        self.entity.interaction_actions.extend(acts);
                    }
                }
            }
            ObjectChunkId::InteractionActions72
            | ObjectChunkId::InteractionActions73
            | ObjectChunkId::InteractionActions74
            | ObjectChunkId::InteractionActions75 => {
                if is_placement_object {
                    let acts = parse_interaction_actions_safe(chunk);
                    if !acts.is_empty() {
                        self.entity.interaction_actions.extend(acts);
                    }
                }
            }
            ObjectChunkId::OpenCollision => {
                if is_door {
                    self.entity.open_collision_model = parse_subcontainer_model_binding(chunk);
                }
            }
            ObjectChunkId::ClosedCollision => {
                if is_door {
                    self.entity.closed_collision_model = parse_subcontainer_model_binding(chunk);
                }
            }
            ObjectChunkId::PlacementOffset => {
                if is_placement_object {
                    self.entity.placement_offset = Some(model::parse_placement_offset(chunk)?);
                }
            }
            ObjectChunkId::Padding301 => {}
            ObjectChunkId::Terminator => {
                self.entity.has_sentinel_terminator = true;
            }
            ObjectChunkId::MeshBindings => {
                self.entity.mesh_bindings = parse_mesh_material_bindings(chunk);
            }
            ObjectChunkId::RagdollBones => {
                if !is_empty_container(chunk) {
                    self.entity.ragdoll_bone_groups = parse_ragdoll_bone_groups(chunk, &self.bones);
                }
            }
            ObjectChunkId::DefaultAnimation => {
                if !is_empty_container(chunk) {
                    self.entity.default_animation = parse_animation_linkage(chunk);
                }
            }
            ObjectChunkId::PhysicsState => {
                if chunk.len() == 16 {
                    self.entity.physics_state = parse_physics_state(chunk);
                }
            }
            ObjectChunkId::Attachments60
            | ObjectChunkId::Attachments86
            | ObjectChunkId::Attachments128 => {}
            ObjectChunkId::AttachmentSlots => {
                self.entity.attachments = parse_attachment_slots(chunk);
            }
            ObjectChunkId::Unknown(_) => {
                let acts = parse_interaction_actions_safe(chunk);
                if !acts.is_empty() {
                    self.entity.interaction_actions.extend(acts);
                } else {
                    self.entity
                        ._engine_metadata
                        .raw_fallbacks
                        .push(RawFallbackComponentJson {
                            id,
                            reason: "Unmapped component".into(),
                            hex: hex::encode_upper(chunk),
                        });
                }
            }
        }
        Ok(())
    }
}

pub fn export_object(data: &[u8]) -> Result<ExtractedObject> {
    if data.len() < 5 {
        bail!("Chunk data too short to be an Object Entity container.");
    }

    let direct_type = u32::from_le_bytes(data[0..4].try_into()?);
    let hi = direct_type >> 8;
    let is_door = hi == 0x004650;
    let is_known_object = matches!(
        direct_type,
        0x0041004B
            | 0x00464621
            | 0x00464661
            | 0x00464669
            | 0x00464665
            | 0x00462103
            | 0x00462107
            | 0x00464181
            | 0x00464681
    ) || is_door
        || hi == 0x004646
        || hi == 0x004621;

    let (raw_type_id, elements) = if is_known_object {
        parse_typed_container(data)?
    } else {
        let (_, root_elements) = parse_chunk_elements(data)?;
        let mut found_payload: Option<Vec<u8>> = None;
        let mut found_type = 0x00464621;

        for (_, chunk_bytes) in &root_elements {
            if chunk_bytes.len() >= 4 {
                let t = u32::from_le_bytes(chunk_bytes[0..4].try_into().unwrap_or_default());
                let t_hi = t >> 8;
                if matches!(
                    t,
                    0x0041004B
                        | 0x00464621
                        | 0x00464661
                        | 0x00464669
                        | 0x00464665
                        | 0x00462103
                        | 0x00462107
                        | 0x00464181
                        | 0x00464681
                ) || t_hi == 0x004650
                    || t_hi == 0x004646
                    || t_hi == 0x004621
                {
                    found_type = t;
                    found_payload = Some(chunk_bytes.clone());
                    break;
                }
            }
            if let Ok((_, inner_elems)) = parse_chunk_elements(chunk_bytes) {
                for (_, ic) in inner_elems {
                    if ic.len() >= 4 {
                        let t = u32::from_le_bytes(ic[0..4].try_into().unwrap_or_default());
                        let t_hi = t >> 8;
                        if matches!(
                            t,
                            0x0041004B
                                | 0x00464621
                                | 0x00464661
                                | 0x00464669
                                | 0x00464665
                                | 0x00462103
                                | 0x00462107
                                | 0x00464181
                                | 0x00464681
                        ) || t_hi == 0x004650
                            || t_hi == 0x004646
                            || t_hi == 0x004621
                        {
                            found_type = t;
                            found_payload = Some(ic.clone());
                            break;
                        }
                    }
                }
            }
            if found_payload.is_some() {
                break;
            }
        }

        let payload =
            found_payload.context("Failed to locate inner Object typed container payload")?;
        let (_, elems) = parse_typed_container(&payload)?;
        (found_type, elems)
    };

    let type_id = ObjectTypeId::from(raw_type_id);
    let is_placement_object = matches!(
        type_id,
        ObjectTypeId::PlacementObject
            | ObjectTypeId::SoundMarker
            | ObjectTypeId::PushableWheel1
            | ObjectTypeId::PushableWheel2
            | ObjectTypeId::LogicMarker
            | ObjectTypeId::PointLight
            | ObjectTypeId::MinionGate
            | ObjectTypeId::UpgradePortal
            | ObjectTypeId::DoorController
            | ObjectTypeId::LightMarker
            | ObjectTypeId::Mechanism
    );

    let mut builder = ObjectEntityBuilder {
        entity: ObjectEntityJson {
            _engine_metadata: ObjectEngineMetadataJson {
                type_id_hex: format!("{:08X}", raw_type_id),
                class_name: Some(format!("{:?}", type_id)),
                raw_fallbacks: Vec::new(),
            },
            ..Default::default()
        },
        bones: Vec::new(),
        type_id,
        pl_material_id: 2,
        pl_is_enabled: true,
        pl_casts_shadows: false,
        pl_can_be_carried: true,
        pl_entity_handle: None,
        pl_state_count: None,
        pl_default_state: None,
        pl_trigger_active: None,
        pl_stance_id: None,
        pl_interaction_flags: None,
        pl_secondary_flags: None,
        pl_flags_hex: None,
    };

    for (id, chunk) in elements {
        builder
            .parse_chunk(id, &chunk)
            .with_context(|| format!("Failed to parse Object Chunk ID {}", id))?;
    }

    if is_placement_object {
        builder.entity.placement_config = Some(PlacementConfigJson {
            physics_material_id: builder.pl_material_id,
            is_enabled: builder.pl_is_enabled,
            casts_shadows: builder.pl_casts_shadows,
            can_be_carried: builder.pl_can_be_carried,
            state_count: builder.pl_state_count,
            default_state: builder.pl_default_state,
            trigger_active: builder.pl_trigger_active,
            stance_id: builder.pl_stance_id,
            interaction_flags: builder.pl_interaction_flags,
            secondary_flags: builder.pl_secondary_flags,
            raw_flags_hex: builder.pl_flags_hex,
        });
    }

    Ok(ExtractedObject {
        entity: builder.entity,
        bones: builder.bones,
    })
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
    let raw_type_id = u32::from_str_radix(&parsed._engine_metadata.type_id_hex, 16)
        .context("Invalid TypeID hex in Object JSON metadata")?;
    let type_id = ObjectTypeId::from(raw_type_id);

    let is_door = type_id == ObjectTypeId::DoorController;
    let is_point_light = type_id == ObjectTypeId::PointLight;
    let is_placement_object = matches!(
        type_id,
        ObjectTypeId::PlacementObject
            | ObjectTypeId::SoundMarker
            | ObjectTypeId::PushableWheel1
            | ObjectTypeId::PushableWheel2
            | ObjectTypeId::LogicMarker
            | ObjectTypeId::PointLight
            | ObjectTypeId::MinionGate
            | ObjectTypeId::UpgradePortal
            | ObjectTypeId::DoorController
            | ObjectTypeId::LightMarker
            | ObjectTypeId::Mechanism
    );

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
        if is_placement_object {
            elements.push((25, endian.write_length_prefixed_string(name)));
        }
    } else {
        if let Some(raw) = get_fallback(21) {
            elements.push((21, raw));
        }
        if let Some(raw) = get_fallback(25) {
            elements.push((25, raw));
        }
    }

    if is_door {
        let pl = parsed.placement_config.as_ref();
        let enabled = pl.is_none_or(|p| p.is_enabled);

        let handle_val = if let Some(ref h) = parsed.entity_handle {
            let tag_byte = h.domain_tag.as_bytes().first().copied().unwrap_or(b'M');
            (tag_byte as u32) << 24 | (h.uid & 0x00FF_FFFF)
        } else if let Some(raw_h) = pl.and_then(|p| p.raw_flags_hex.as_ref()) {
            let clean = raw_h
                .trim()
                .trim_start_matches("0x")
                .trim_start_matches("0X");
            u32::from_str_radix(clean, 16).unwrap_or(0x4D00_0001)
        } else {
            0x4D00_0001
        };
        elements.push((22, endian.u32_to_bytes(handle_val).to_vec()));

        elements.push((23, vec![if enabled { 1 } else { 0 }]));

        let stance = pl.and_then(|p| p.stance_id).unwrap_or(0);
        elements.push((28, vec![stance]));

        if let Some(raw) = get_fallback(29) {
            elements.push((29, raw));
        } else {
            elements.push((
                29,
                vec![1, 40, 0, 2, 40, 0, 43, 4, 1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0],
            ));
        }

        elements.push((33, vec![1, 1, 0, 0]));
        elements.push((34, vec![0]));
        elements.push((36, vec![0]));

        if let Some(ref stand) = parsed.stand_model {
            elements.push((
                50,
                build_simple_model_binding(&stand.object_path, &stand.model_name, endian),
            ));
        } else if let Some(raw) = get_fallback(50) {
            elements.push((50, raw));
        }

        let sc = pl.and_then(|p| p.state_count).unwrap_or(2);
        elements.push((55, endian.u32_to_bytes(sc).to_vec()));

        if let Some(raw) = get_fallback(70) {
            elements.push((70, raw));
        }

        let def_s = pl.and_then(|p| p.default_state).unwrap_or(1);
        elements.push((71, endian.u32_to_bytes(def_s).to_vec()));

        for i in 80..=87 {
            if let Some(raw) = get_fallback(i) {
                elements.push((i, raw));
            } else {
                elements.push((i, vec![3, 10, 0, 11, 1, 13, 5, 0, 1, 1, 0, 0, 0]));
            }
        }

        for i in 90..=97 {
            if let Some(raw) = get_fallback(i) {
                elements.push((i, raw));
            } else {
                elements.push((
                    i,
                    vec![
                        6, 20, 0, 21, 1, 22, 2, 24, 6, 25, 7, 26, 8, 0, 0, 1, 1, 0, 0, 0, 0, 0,
                    ],
                ));
            }
        }

        if let Some(ref col) = parsed.open_collision_model {
            elements.push((100, build_subcontainer_model_binding(col, endian)));
        } else if let Some(raw) = get_fallback(100) {
            elements.push((100, raw));
        }

        if let Some(ref col) = parsed.closed_collision_model {
            elements.push((101, build_subcontainer_model_binding(col, endian)));
        } else if let Some(raw) = get_fallback(101) {
            elements.push((101, raw));
        }

        for i in 102..=107 {
            if let Some(raw) = get_fallback(i) {
                elements.push((i, raw));
            } else {
                elements.push((
                    i,
                    vec![
                        6, 20, 0, 21, 1, 22, 2, 24, 6, 25, 7, 26, 8, 0, 0, 1, 1, 0, 0, 0, 0, 0,
                    ],
                ));
            }
        }
    } else if is_point_light {
        let pl = parsed.placement_config.as_ref();
        let enabled = pl.is_none_or(|p| p.is_enabled);

        let handle_val = if let Some(ref h) = parsed.entity_handle {
            let tag_byte = h.domain_tag.as_bytes().first().copied().unwrap_or(b'M');
            (tag_byte as u32) << 24 | (h.uid & 0x00FF_FFFF)
        } else if let Some(raw_h) = pl.and_then(|p| p.raw_flags_hex.as_ref()) {
            let clean = raw_h
                .trim()
                .trim_start_matches("0x")
                .trim_start_matches("0X");
            u32::from_str_radix(clean, 16).unwrap_or(0x4D00_0001)
        } else {
            0x4D00_0001
        };
        elements.push((22, endian.u32_to_bytes(handle_val).to_vec()));

        elements.push((23, vec![if enabled { 1 } else { 0 }]));

        let stance = pl.and_then(|p| p.stance_id).unwrap_or(1);
        elements.push((28, vec![stance]));

        if let Some(raw) = get_fallback(29) {
            elements.push((29, raw));
        } else {
            elements.push((
                29,
                vec![
                    1, 40, 0, 2, 40, 0, 43, 4, 1, 1, 0, 0, 0, 1, 20, 0, 0, 0, 0, 0, 0, 0, 0x3F, 0,
                    0, 0, 0, 0,
                ],
            ));
        }

        if let Some(ref stand) = parsed.stand_model {
            elements.push((
                31,
                build_simple_model_binding(&stand.object_path, &stand.model_name, endian),
            ));
        } else if let Some(raw) = get_fallback(31) {
            elements.push((31, raw));
        }

        elements.push((41, vec![1, 1, 0, 0]));
        elements.push((42, vec![2, 0x1E, 0, 0x23, 1, 0, 0, 0, 0, 0, 1, 1, 0, 0]));
        elements.push((45, vec![2, 0, 0, 0]));
        elements.push((
            46,
            endian
                .u32_to_bytes(pl.map(|p| p.physics_material_id).unwrap_or(2176))
                .to_vec(),
        ));

        if let Some(ref light) = parsed.point_light {
            elements.push((70, model::build_point_light_params(light, endian)));
        } else if let Some(raw) = get_fallback(70) {
            elements.push((70, raw));
        }
    } else if is_placement_object {
        let pl = parsed.placement_config.as_ref();
        let mat_id = pl.map(|p| p.physics_material_id).unwrap_or(2);
        let enabled = pl.is_none_or(|p| p.is_enabled);
        let carry = pl.is_none_or(|p| p.can_be_carried);
        let shadows = pl.is_none_or(|p| p.casts_shadows);

        let handle_val = if let Some(ref h) = parsed.entity_handle {
            let tag_byte = h.domain_tag.as_bytes().first().copied().unwrap_or(b'M');
            let mut mask = (tag_byte as u32) << 24 | (h.uid & 0x00FF_FFFF);
            if shadows {
                mask |= 0x2000_0000;
            }
            mask
        } else if let Some(raw_h) = pl.and_then(|p| p.raw_flags_hex.as_ref()) {
            let clean = raw_h
                .trim()
                .trim_start_matches("0x")
                .trim_start_matches("0X");
            let mut mask = u32::from_str_radix(clean, 16).unwrap_or(0x4D00_0001);
            if shadows {
                mask |= 0x2000_0000;
            } else {
                mask &= !0x2000_0000;
            }
            mask
        } else {
            0x4D00_0001
        };
        elements.push((22, endian.u32_to_bytes(handle_val).to_vec()));

        elements.push((23, vec![if enabled { 1 } else { 0 }]));

        let stance = pl.and_then(|p| p.stance_id).unwrap_or(0);
        elements.push((28, vec![stance]));

        if carry {
            elements.push((29, vec![1, 40, 0, 3, 40, 0, 43, 4, 44, 5, 1, 1, 0, 0, 0, 1]));
        } else {
            elements.push((29, vec![0u8]));
        }

        if type_id == ObjectTypeId::LogicMarker {
            if let Some(ref stand) = parsed.stand_model {
                elements.push((
                    31,
                    build_simple_model_binding(&stand.object_path, &stand.model_name, endian),
                ));
            } else if let Some(raw) = get_fallback(31) {
                elements.push((31, raw));
            }

            let trig_act = pl.and_then(|p| p.trigger_active).unwrap_or(true);
            elements.push((43, vec![if trig_act { 1 } else { 0 }]));

            if let Some(ref le) = parsed.logic_event_link {
                let mut sub_elems =
                    vec![(20, endian.write_length_prefixed_string(&le.target_event))];
                if let Some(h) = le.linked_environment_handle {
                    let hex_bytes = endian
                        .u32_to_bytes(0x4D00_0000 | (h & 0x00FF_FFFF))
                        .to_vec();
                    sub_elems.push((19, hex_bytes));
                } else if let Some(ref hex_str) = le.raw_hex_id
                    && let Ok(b) = hex::decode(hex_str)
                {
                    sub_elems.push((19, b));
                }
                let chunk_50 = crate::engine::assets::build_chunk_from_elements_with_endian(
                    false, &sub_elems, endian,
                );
                elements.push((50, chunk_50));
            } else if let Some(raw) = get_fallback(50) {
                elements.push((50, raw));
            }

            if let Some(raw) = get_fallback(45) {
                elements.push((45, raw));
            } else {
                let flag_45 = pl
                    .and_then(|p| p.secondary_flags.as_ref())
                    .and_then(|h| hex::decode(h).ok())
                    .unwrap_or_else(|| vec![1, 1, 0, 0]);
                elements.push((45, flag_45));
            }
        } else {
            if let Some(ref stand) = parsed.stand_model {
                elements.push((
                    31,
                    build_simple_model_binding(&stand.object_path, &stand.model_name, endian),
                ));
            } else if let Some(raw) = get_fallback(31) {
                elements.push((31, raw));
            }

            elements.push((38, vec![1u8]));

            let flag_41 = pl
                .and_then(|p| p.interaction_flags.as_ref())
                .and_then(|h| hex::decode(h).ok())
                .unwrap_or_else(|| vec![1, 1, 0, 0]);
            elements.push((41, flag_41));

            let flag_45 = pl
                .and_then(|p| p.secondary_flags.as_ref())
                .and_then(|h| hex::decode(h).ok())
                .unwrap_or_else(|| vec![1, 1, 0, 0]);
            elements.push((45, flag_45));
        }

        if let Some(ref placed) = parsed.placed_object {
            elements.push((42, build_placed_object(placed, endian)));
        } else if let Some(raw) = get_fallback(42) {
            elements.push((42, raw));
        }

        elements.push((44, vec![0u8]));
        elements.push((46, endian.u32_to_bytes(mat_id).to_vec()));

        if let Some(offset) = parsed.placement_offset {
            elements.push((300, model::build_placement_offset(offset, endian)));
        } else if let Some(raw) = get_fallback(300) {
            elements.push((300, raw));
        } else {
            elements.push((300, model::build_placement_offset([0.0, 0.5, 0.0], endian)));
        }
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
        if let Ok(raw) = hex::decode(&fb.hex) {
            elements.push((fb.id, raw));
        }
    }

    elements.sort_by_key(|&(id, _)| id);
    Ok(build_typed_container_with_endian(
        raw_type_id,
        &elements,
        endian,
    ))
}
