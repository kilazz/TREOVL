use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::Path;

use super::animation::parse_object_bone_container;
use super::{
    build_chunk_from_elements, build_typed_container, parse_chunk_elements, parse_typed_container,
};
use crate::engine::common::{read_length_prefixed_string, write_length_prefixed_string};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ObjectEntityJson {
    pub type_id_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub mesh_bindings: Vec<MeshMaterialBindingJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_animation: Option<DefaultAnimationLinkJson>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub bones: Vec<ObjectBoneSummaryJson>,
    pub components: Vec<ObjectComponentBlockJson>,
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
pub struct ObjectBoneSummaryJson {
    pub id: usize,
    pub name: String,
    pub parent_index: i32,
    pub translation: [f32; 3],
    pub rotation_quat: [f32; 4],
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ObjectComponentBlockJson {
    pub id: u32,
    pub role: String,
    pub hex: String,
}

// =========================================================================
// PUBLIC API: EXPORT & IMPORT
// =========================================================================

/// Exports Object Entity to JSON and optionally extracts the Master Rig into a GLB file.
pub fn export_object_to_json(data: &[u8], output_dir: Option<&Path>) -> Result<String> {
    if data.len() < 5 {
        bail!("Chunk data too short to be an Object Entity container.");
    }

    let (type_id, elements) = parse_typed_container(data)
        .context("Failed to parse Object Entity root typed container")?;

    let mut group_tag = None;
    let mut entity_name = None;
    let mut scale = None;
    let mut mesh_bindings = Vec::new();
    let mut default_animation = None;
    let mut bones = Vec::new();
    let mut raw_bones_struct = Vec::new(); // Store raw parsed bones for GLB export
    let mut components = Vec::new();

    for (id, chunk) in &elements {
        match *id {
            20 => {
                group_tag = read_length_prefixed_string(chunk);
            }
            21 => {
                entity_name = read_length_prefixed_string(chunk);
            }
            30 => {
                mesh_bindings = parse_mesh_material_bindings(chunk);
            }
            32 => {
                scale = read_scale_vector(chunk);
            }
            33 => {
                if let Ok(parsed_bones) = parse_object_bone_container(chunk) {
                    raw_bones_struct = parsed_bones.clone();
                    bones = parsed_bones
                        .into_iter()
                        .enumerate()
                        .map(|(idx, b)| ObjectBoneSummaryJson {
                            id: idx,
                            name: b.name,
                            parent_index: b.parent_index,
                            translation: [b.translation.x, b.translation.y, b.translation.z],
                            rotation_quat: [b.rotation.x, b.rotation.y, b.rotation.z, b.rotation.w],
                        })
                        .collect();
                }
            }
            37 => {
                default_animation = parse_animation_linkage(chunk);
            }
            _ => {}
        }

        components.push(ObjectComponentBlockJson {
            id: *id,
            role: get_component_role(*id).to_string(),
            hex: hex::encode_upper(chunk),
        });
    }

    let entity_json = ObjectEntityJson {
        type_id_hex: format!("{:08X}", type_id),
        group_tag,
        entity_name: entity_name.clone(),
        scale,
        mesh_bindings,
        default_animation,
        bones,
        components,
    };

    // Automatically export Master Skeleton to .glb if an output directory is provided
    if let Some(dir) = output_dir
        && !raw_bones_struct.is_empty()
    {
        let rig_name = entity_name.unwrap_or_else(|| "Unknown_Rig".to_string());
        if let Ok(glb_bytes) =
            super::animation::export_skeleton_to_glb(&raw_bones_struct, &rig_name)
        {
            let glb_path = dir.join(format!("{}_MASTER_RIG.glb", rig_name));
            let _ = std::fs::write(glb_path, glb_bytes);
        }
    }

    serde_json::to_string_pretty(&entity_json).map_err(|e| anyhow::anyhow!(e))
}

pub fn import_object_from_json(json_str: &str) -> Result<Vec<u8>> {
    let parsed: ObjectEntityJson = serde_json::from_str(json_str)?;
    let type_id = u32::from_str_radix(&parsed.type_id_hex, 16)
        .context("Invalid TypeID hex in Object JSON")?;

    let mut elements = Vec::new();

    for comp in parsed.components {
        let mut raw_bytes = hex::decode(&comp.hex)
            .with_context(|| format!("Invalid hex payload in component ID {}", comp.id))?;

        // Re-inject edited high-level properties into the component payload
        match comp.id {
            20 => {
                if let Some(ref tag) = parsed.group_tag {
                    raw_bytes = write_length_prefixed_string(tag);
                }
            }
            21 => {
                if let Some(ref name) = parsed.entity_name {
                    raw_bytes = write_length_prefixed_string(name);
                }
            }
            32 => {
                if let Some(s) = parsed.scale {
                    raw_bytes = write_scale_vector(s);
                }
            }
            30 if !parsed.mesh_bindings.is_empty() => {
                if let Ok(rebuilt) = rebuild_mesh_material_bindings(&parsed.mesh_bindings) {
                    raw_bytes = rebuilt;
                }
            }
            _ => {}
        }

        elements.push((comp.id, raw_bytes));
    }

    Ok(build_typed_container(type_id, &elements))
}

// =========================================================================
// INTERNAL PARSING HELPERS
// =========================================================================

fn get_component_role(id: u32) -> &'static str {
    match id {
        20 => "Object Group Tag (OBJ Path)",
        21 => "Entity / Prefab Name",
        30 => "Mesh & Material Bindings Table",
        32 => "Entity Scale Vector (X, Y, Z)",
        33 => "Skeletal Rig (Object Bones Array)",
        34 => "Bounding Box & Collision Bounds",
        35 => "Hitbox & Ragdoll Bone Groups",
        36 => "Attachment Sockets Configuration",
        37 => "Default Animation Linkage",
        19 => "Sentinel Terminator",
        1 => "Child Entity / Attachment Link",
        _ => "Engine Parameter / Data Block",
    }
}

fn read_scale_vector(data: &[u8]) -> Option<[f32; 3]> {
    if data.len() >= 12 {
        let mut cur = Cursor::new(data);
        let x = cur.read_f32::<LittleEndian>().ok()?;
        let y = cur.read_f32::<LittleEndian>().ok()?;
        let z = cur.read_f32::<LittleEndian>().ok()?;
        if x.is_finite() && y.is_finite() && z.is_finite() {
            return Some([x, y, z]);
        }
    }
    None
}

fn write_scale_vector(s: [f32; 3]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12);
    let mut cur = Cursor::new(&mut out);
    let _ = cur.write_f32::<LittleEndian>(s[0]);
    let _ = cur.write_f32::<LittleEndian>(s[1]);
    let _ = cur.write_f32::<LittleEndian>(s[2]);
    out
}

fn parse_mesh_material_bindings(chunk_data: &[u8]) -> Vec<MeshMaterialBindingJson> {
    let mut bindings = Vec::new();

    let elements = if let Ok((_, elems)) = parse_chunk_elements(chunk_data) {
        elems
    } else {
        return bindings;
    };

    for (_, elem_data) in elements {
        if elem_data.starts_with(b"\x67\x00\x41\x00")
            && let Ok((_, sub_parts)) = parse_typed_container(&elem_data)
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

fn rebuild_mesh_material_bindings(bindings: &[MeshMaterialBindingJson]) -> Result<Vec<u8>> {
    let mut binding_chunks = Vec::new();

    for (idx, b) in bindings.iter().enumerate() {
        // Element 31: Mesh identifiers
        let mesh_elements = vec![
            (20, write_length_prefixed_string(&b.mesh_path)),
            (21, write_length_prefixed_string(&b.mesh_part_name)),
        ];
        let mesh_container = build_chunk_from_elements(false, &mesh_elements);

        // Element 33: Material identifiers
        let mat_elements = vec![
            (20, write_length_prefixed_string(&b.material_path)),
            (21, write_length_prefixed_string(&b.material_name)),
        ];
        let mat_container = build_chunk_from_elements(false, &mat_elements);

        let binding_sub = vec![(31, mesh_container), (33, mat_container)];

        let binding_chunk = build_typed_container(0x00410067, &binding_sub);
        binding_chunks.push((idx as u32, binding_chunk));
    }

    Ok(build_chunk_from_elements(true, &binding_chunks))
}

fn parse_animation_linkage(chunk_data: &[u8]) -> Option<DefaultAnimationLinkJson> {
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
