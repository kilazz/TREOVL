use serde::{Deserialize, Serialize};

use super::model::DoorStateJson;
use crate::engine::assets::attachment::{ItemTriggerActionJson, parse_item_trigger_actions};
use crate::engine::assets::{build_chunk_from_elements_with_endian, parse_chunk_elements};
use crate::engine::common::{Endian, EntityHandleJson, read_length_prefixed_string};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ObjectModelBindingJson {
    pub object_path: String,
    pub model_name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct PlacementConfigJson {
    pub physics_material_id: u32,
    pub is_enabled: bool,
    pub casts_shadows: bool,
    pub can_be_carried: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_handle: Option<EntityHandleJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_state: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stance_id: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction_flags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_flags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_flags_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown_44: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding_301: Option<u8>,
}

pub fn parse_simple_model_binding(data: &[u8]) -> Option<ObjectModelBindingJson> {
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
            return Some(ObjectModelBindingJson {
                object_path: obj_path,
                model_name,
            });
        }
    }
    None
}

pub fn build_simple_model_binding(obj_path: &str, model_name: &str, endian: Endian) -> Vec<u8> {
    let elements = vec![
        (20, endian.write_length_prefixed_string(obj_path)),
        (21, endian.write_length_prefixed_string(model_name)),
    ];
    build_chunk_from_elements_with_endian(false, &elements, endian)
}

pub fn parse_placed_object(chunk: &[u8]) -> Option<ObjectModelBindingJson> {
    if let Ok((_, elements)) = parse_chunk_elements(chunk) {
        for (id, data) in elements {
            if id == 30 {
                return parse_simple_model_binding(&data);
            }
        }
    }
    parse_simple_model_binding(chunk)
}

pub fn build_placed_object(binding: &ObjectModelBindingJson, endian: Endian) -> Vec<u8> {
    let model_bytes = build_simple_model_binding(&binding.object_path, &binding.model_name, endian);
    let sub_elements = vec![(30, model_bytes), (35, vec![0u8, 0, 0, 0])];
    build_chunk_from_elements_with_endian(false, &sub_elements, endian)
}

pub fn parse_subcontainer_model_binding(chunk: &[u8]) -> Option<ObjectModelBindingJson> {
    if let Ok((_, elements)) = parse_chunk_elements(chunk) {
        for (_, data) in elements {
            if let Some(mb) = parse_simple_model_binding(&data) {
                return Some(mb);
            }
        }
    }
    parse_simple_model_binding(chunk)
}

pub fn build_subcontainer_model_binding(
    binding: &ObjectModelBindingJson,
    endian: Endian,
) -> Vec<u8> {
    let model_bytes = build_simple_model_binding(&binding.object_path, &binding.model_name, endian);
    let sub_elements = vec![
        (20, vec![0x01]),
        (21, vec![0x02]),
        (24, vec![0x06]),
        (25, vec![0x07]),
        (26, model_bytes),
    ];
    build_chunk_from_elements_with_endian(false, &sub_elements, endian)
}

pub fn parse_door_states(chunk: &[u8]) -> Vec<DoorStateJson> {
    let mut states = Vec::new();
    let (_, state_containers) = match parse_chunk_elements(chunk) {
        Ok(res) => res,
        Err(_) => return states,
    };

    for (s_idx, s_chunk) in state_containers {
        if let Ok((_, props)) = parse_chunk_elements(&s_chunk) {
            let mut idle_anim_clip = None;
            let mut idle_anim_name = None;
            let mut transition_anim_clip = None;
            let mut transition_anim_name = None;
            let mut model_binding = None;

            for (pid, pdata) in props {
                match pid {
                    40 => {
                        if let Ok((_, anim_props)) = parse_chunk_elements(&pdata) {
                            for (aid, adata) in anim_props {
                                if aid == 20 {
                                    idle_anim_clip = read_length_prefixed_string(&adata);
                                } else if aid == 21 {
                                    idle_anim_name = read_length_prefixed_string(&adata);
                                }
                            }
                        }
                    }
                    41 => {
                        if let Ok((_, anim_props)) = parse_chunk_elements(&pdata) {
                            for (aid, adata) in anim_props {
                                if aid == 20 {
                                    transition_anim_clip = read_length_prefixed_string(&adata);
                                } else if aid == 21 {
                                    transition_anim_name = read_length_prefixed_string(&adata);
                                }
                            }
                        }
                    }
                    42 => {
                        model_binding = parse_simple_model_binding(&pdata);
                    }
                    _ => {}
                }
            }

            if idle_anim_clip.is_some() || model_binding.is_some() {
                let state_name = match s_idx {
                    40 => "Open".to_string(),
                    41 => "Closed".to_string(),
                    _ => format!("State_{}", s_idx),
                };
                let state_id = if s_idx == 40 {
                    0
                } else if s_idx == 41 {
                    1
                } else {
                    s_idx
                };

                states.push(DoorStateJson {
                    state_id,
                    state_name,
                    idle_anim_clip,
                    idle_anim_name,
                    transition_anim_clip,
                    transition_anim_name,
                    model_binding,
                });
            }
        }
    }

    states
}

pub fn parse_interaction_actions_safe(chunk: &[u8]) -> Vec<ItemTriggerActionJson> {
    let mut acts = Vec::new();
    if let Ok((_, elements)) = parse_chunk_elements(chunk) {
        for (_, data) in elements {
            let sub_acts = parse_item_trigger_actions(&data);
            if !sub_acts.is_empty() {
                acts.extend(sub_acts);
            }
        }
    }
    if acts.is_empty() {
        acts = parse_item_trigger_actions(chunk);
    }
    acts
}
