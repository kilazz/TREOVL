use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::physics::{BoneGroupJson, BoundingBoxJson, EntityPhysicsStateJson, FullObjectBoneJson};
use super::placement::{ObjectModelBindingJson, PlacementConfigJson};
use crate::engine::assets::attachment::ItemTriggerActionJson;
use crate::engine::assets::{
    build_chunk_from_elements_with_endian, build_typed_container_with_endian, parse_chunk_elements,
};
use crate::engine::common::{Endian, EntityHandleJson, read_length_prefixed_string};

pub trait ComponentParser {
    fn parse_chunk(&mut self, id: u32, chunk: &[u8]) -> Result<()>;
}

#[inline]
pub fn is_empty_container(chunk: &[u8]) -> bool {
    chunk.is_empty() || chunk == [0u8]
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LogicEventLinkJson {
    pub target_event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_hex_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_environment_handle: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DoorStateJson {
    pub state_id: u32,
    pub state_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_anim_clip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_anim_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition_anim_clip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition_anim_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_binding: Option<ObjectModelBindingJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct PointLightConfigJson {
    pub color: String,
    pub radius: f32,
    pub intensity: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub falloff: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_type: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ObjectEntityJson {
    pub _engine_metadata: ObjectEngineMetadataJson,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_handle: Option<EntityHandleJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounding_box: Option<BoundingBoxJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_animation: Option<DefaultAnimationLinkJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physics_state: Option<EntityPhysicsStateJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub ragdoll_bone_groups: Vec<BoneGroupJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub mesh_bindings: Vec<MeshMaterialBindingJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stand_model: Option<ObjectModelBindingJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placed_object: Option<ObjectModelBindingJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_collision_model: Option<ObjectModelBindingJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_collision_model: Option<ObjectModelBindingJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point_light: Option<PointLightConfigJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub door_states: Vec<DoorStateJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub interaction_actions: Vec<ItemTriggerActionJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logic_event_link: Option<LogicEventLinkJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement_offset: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement_config: Option<PlacementConfigJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub bones: Vec<FullObjectBoneJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub attachments: Vec<AttachmentSlotJson>,
    #[serde(default = "default_true")]
    pub has_sentinel_terminator: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ObjectEngineMetadataJson {
    pub type_id_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub raw_fallbacks: Vec<RawFallbackComponentJson>,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RawFallbackComponentJson {
    pub id: u32,
    pub reason: String,
    pub hex: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MeshMaterialBindingJson {
    pub mesh_path: String,
    pub mesh_part_name: String,
    pub material_path: String,
    pub material_name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DefaultAnimationLinkJson {
    pub anim_group: String,
    pub clip_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AttachmentSlotJson {
    pub slot_id: u32,
    pub data_hex: String,
}

// -----------------------------------------------------------------------------
// Component Parsers (Strict `Result` based)
// -----------------------------------------------------------------------------

pub fn read_scale_vector(data: &[u8]) -> Result<[f32; 3]> {
    if data.len() < 12 {
        bail!("Chunk too small to contain a scale vector (requires 12 bytes)");
    }
    let mut cur = Cursor::new(data);
    let x = cur
        .read_f32::<LittleEndian>()
        .context("Failed to read scale X")?;
    let y = cur
        .read_f32::<LittleEndian>()
        .context("Failed to read scale Y")?;
    let z = cur
        .read_f32::<LittleEndian>()
        .context("Failed to read scale Z")?;

    if x.is_finite() && y.is_finite() && z.is_finite() {
        Ok([x, y, z])
    } else {
        bail!("Scale vector contains invalid (NaN/Infinite) floating point values");
    }
}

pub fn parse_placement_offset(chunk: &[u8]) -> Result<[f32; 3]> {
    if chunk.len() >= 15 && chunk[0] == 1 && chunk[1] == 20 {
        let mut cur = Cursor::new(&chunk[3..15]);
        let x = cur
            .read_f32::<LittleEndian>()
            .context("Failed to read offset X")?;
        let y = cur
            .read_f32::<LittleEndian>()
            .context("Failed to read offset Y")?;
        let z = cur
            .read_f32::<LittleEndian>()
            .context("Failed to read offset Z")?;
        if x.is_finite() && y.is_finite() && z.is_finite() {
            return Ok([x, y, z]);
        }
    }
    bail!("Invalid or unreadable placement offset format");
}

pub fn build_placement_offset(offset: [f32; 3], endian: Endian) -> Vec<u8> {
    let mut out = vec![1u8, 20, 0];
    let _ = endian.write_f32(&mut out, offset[0]);
    let _ = endian.write_f32(&mut out, offset[1]);
    let _ = endian.write_f32(&mut out, offset[2]);
    out
}

pub fn parse_point_light_params(chunk: &[u8]) -> Result<PointLightConfigJson> {
    let (_, elements) =
        parse_chunk_elements(chunk).context("Failed to parse point light elements")?;
    let mut light = PointLightConfigJson {
        color: String::from("#FFFFFF"),
        radius: 5.0,
        intensity: 1.0,
        falloff: None,
        light_type: None,
    };

    for (id, data) in elements {
        match id {
            0x0BCD if data.len() >= 4 => {
                light.intensity = f32::from_le_bytes(data[0..4].try_into()?).clamp(0.0, 10_000.0);
            }
            0x0BD0 if data.len() >= 3 => {
                light.color = format!("#{:02X}{:02X}{:02X}", data[0], data[1], data[2]);
            }
            0x0BD1 if data.len() >= 4 => {
                light.falloff = Some(f32::from_le_bytes(data[0..4].try_into()?));
            }
            0x0BCF if data.len() >= 4 => {
                light.light_type = Some(u32::from_le_bytes(data[0..4].try_into()?));
            }
            0x0BD3 if data.len() >= 4 => {
                let r = f32::from_le_bytes(data[0..4].try_into()?);
                if !r.is_finite() {
                    bail!("Light radius contains NaN or Infinity");
                }
                light.radius = r;
            }
            _ => {}
        }
    }

    Ok(light)
}

pub fn build_point_light_params(light: &PointLightConfigJson, endian: Endian) -> Vec<u8> {
    let parse_hex_color = |hex_str: &str| -> [u8; 4] {
        let clean = hex_str.trim_start_matches('#');
        if let Ok(val) = u32::from_str_radix(clean, 16) {
            [
                ((val >> 16) & 0xFF) as u8,
                ((val >> 8) & 0xFF) as u8,
                (val & 0xFF) as u8,
                0,
            ]
        } else {
            [255, 255, 255, 0]
        }
    };

    let mut elements = vec![
        (0x0BCC, vec![0, 0, 0, 0]),
        (0x0BCD, endian.f32_to_bytes(light.intensity).to_vec()),
        (0x0BCE, vec![0, 0, 0, 0x3F]),
        (
            0x0BCF,
            endian.u32_to_bytes(light.light_type.unwrap_or(1)).to_vec(),
        ),
        (0x0BD0, parse_hex_color(&light.color).to_vec()),
    ];

    if let Some(fo) = light.falloff {
        let mut b = vec![0u8; 12];
        let fo_bytes = endian.f32_to_bytes(fo);
        b[6..10].copy_from_slice(&fo_bytes);
        elements.push((0x0BD1, b));
    } else {
        elements.push((0x0BD1, vec![0, 0, 0, 0, 0, 0, 0x20, 0x40, 0, 0, 0, 0]));
    }

    elements.push((0x0BD2, vec![0x16]));
    elements.push((0x0BD3, endian.f32_to_bytes(light.radius).to_vec()));

    crate::engine::assets::build_chunk_from_elements_with_endian(false, &elements, endian)
}

pub fn parse_mesh_material_bindings(chunk_data: &[u8]) -> Vec<MeshMaterialBindingJson> {
    let mut bindings = Vec::new();
    let elements = if let Ok((_, elems)) = parse_chunk_elements(chunk_data) {
        elems
    } else {
        return bindings;
    };

    for (_, elem_data) in elements {
        if elem_data.starts_with(b"\x67\x00\x41\x00")
            && let Ok((_, sub_parts)) = crate::engine::assets::parse_typed_container(&elem_data)
        {
            let mut mesh_path = String::new();
            let mut mesh_part_name = String::new();
            let mut material_path = String::new();
            let mut material_name = String::new();

            for (part_id, part_data) in sub_parts {
                if let Ok((_, str_elements)) = parse_chunk_elements(&part_data) {
                    for (str_id, str_data) in str_elements {
                        if str_id == 20
                            && let Some(s) = read_length_prefixed_string(&str_data)
                        {
                            if part_id == 31 {
                                mesh_path = s;
                            } else if part_id == 33 {
                                material_path = s;
                            }
                        } else if str_id == 21
                            && let Some(s) = read_length_prefixed_string(&str_data)
                        {
                            if part_id == 31 {
                                mesh_part_name = s;
                            } else if part_id == 33 {
                                material_name = s;
                            }
                        }
                    }
                }
            }

            if !mesh_path.is_empty() || !material_path.is_empty() {
                bindings.push(MeshMaterialBindingJson {
                    mesh_path,
                    mesh_part_name,
                    material_path,
                    material_name,
                });
            }
        }
    }

    bindings
}

pub fn rebuild_mesh_material_bindings(
    bindings: &[MeshMaterialBindingJson],
    endian: Endian,
) -> Result<Vec<u8>> {
    let mut binding_chunks = Vec::new();

    for (idx, b) in bindings.iter().enumerate() {
        let mesh_elements = vec![
            (20, endian.write_length_prefixed_string(&b.mesh_path)),
            (21, endian.write_length_prefixed_string(&b.mesh_part_name)),
        ];
        let mesh_container = build_chunk_from_elements_with_endian(false, &mesh_elements, endian);

        let mat_elements = vec![
            (20, endian.write_length_prefixed_string(&b.material_path)),
            (21, endian.write_length_prefixed_string(&b.material_name)),
        ];
        let mat_container = build_chunk_from_elements_with_endian(false, &mat_elements, endian);

        let binding_sub = vec![(31, mesh_container), (33, mat_container)];
        let binding_chunk = build_typed_container_with_endian(0x00410067, &binding_sub, endian);
        binding_chunks.push((idx as u32, binding_chunk));
    }

    Ok(build_chunk_from_elements_with_endian(
        true,
        &binding_chunks,
        endian,
    ))
}

pub fn parse_animation_linkage(chunk_data: &[u8]) -> Option<DefaultAnimationLinkJson> {
    if let Ok((_, elements)) = parse_chunk_elements(chunk_data) {
        let mut anim_group = String::new();
        let mut clip_name = String::new();
        let mut track_name = None;

        for (id, data) in elements {
            match id {
                20 => {
                    if let Some(s) = read_length_prefixed_string(&data) {
                        anim_group = s;
                    }
                }
                21 => {
                    if let Some(s) = read_length_prefixed_string(&data) {
                        clip_name = s;
                    }
                }
                _ => {
                    if track_name.is_none()
                        && let Some(s) = read_length_prefixed_string(&data)
                    {
                        track_name = Some(s);
                    }
                }
            }
        }

        if !clip_name.is_empty() {
            return Some(DefaultAnimationLinkJson {
                anim_group,
                clip_name,
                track_name,
            });
        }
    }
    None
}

pub fn rebuild_animation_linkage(anim: &DefaultAnimationLinkJson, endian: Endian) -> Vec<u8> {
    let mut elements = vec![
        (20, endian.write_length_prefixed_string(&anim.anim_group)),
        (21, endian.write_length_prefixed_string(&anim.clip_name)),
    ];
    if let Some(ref track) = anim.track_name {
        elements.push((22, endian.write_length_prefixed_string(track)));
    }
    build_chunk_from_elements_with_endian(false, &elements, endian)
}

pub fn parse_attachment_slots(chunk: &[u8]) -> Vec<AttachmentSlotJson> {
    let mut out = Vec::new();
    if is_empty_container(chunk) {
        return out;
    }
    if let Ok((_, elements)) = parse_chunk_elements(chunk) {
        for (id, data) in elements {
            out.push(AttachmentSlotJson {
                slot_id: id,
                data_hex: hex::encode_upper(data),
            });
        }
    }
    out
}

pub fn rebuild_attachment_slots(slots: &[AttachmentSlotJson], endian: Endian) -> Vec<u8> {
    if slots.is_empty() {
        return vec![0u8];
    }
    let mut elements = Vec::new();
    for s in slots {
        if let Ok(b) = hex::decode(&s.data_hex) {
            elements.push((s.slot_id, b));
        }
    }
    build_chunk_from_elements_with_endian(false, &elements, endian)
}

pub fn write_scale_vector(s: [f32; 3], endian: Endian) -> Vec<u8> {
    let mut out = Vec::with_capacity(12);
    let _ = endian.write_f32(&mut out, s[0]);
    let _ = endian.write_f32(&mut out, s[1]);
    let _ = endian.write_f32(&mut out, s[2]);
    out
}
