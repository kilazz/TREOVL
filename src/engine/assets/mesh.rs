use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde_json::json;
use std::collections::HashMap;
use std::io::Cursor;

use super::animation::{ObjectBone, parse_object_bone_container};
use super::{
    build_chunk_from_elements, build_typed_container, parse_chunk_elements, parse_typed_container,
};
use crate::engine::common::{chunk_id, magic};
use crate::engine::math::{Vector2, Vector3, Vector4};
use crate::utils::gltf_builder::GltfBuilder;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VertexSemantic {
    Position,
    Normal,
    TexCoord,
    Color,
    TangentQuat,
    BlendWeights,
    BlendIndices,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub struct VertexAttribute {
    pub semantic: VertexSemantic,
    pub byte_size: usize,
}

impl VertexAttribute {
    pub fn from_descriptor(desc: u32) -> Self {
        let semantic_byte = ((desc >> 16) & 0xFF) as u8;
        let flags = ((desc >> 24) & 0xFF) as u8;

        let semantic = match semantic_byte {
            0x01 => VertexSemantic::Position,
            0x04 => VertexSemantic::Normal,
            0x05 => VertexSemantic::TexCoord,
            0x06 => VertexSemantic::Color,
            0x09 => VertexSemantic::TangentQuat,
            0x0A => VertexSemantic::BlendWeights,
            0x0B => VertexSemantic::BlendIndices,
            _ => VertexSemantic::Unknown,
        };

        let byte_size = match flags {
            1 => 8,     // 2x f32 (UVs)
            2 => 12,    // 3x f32 (Vec3 Position / Normal)
            3 => 16,    // 4x f32 (Vec4 TangentQuat)
            4 | 7 => 1, // 1x u8 (Packed bone index or normalized weight)
            15 => 4,    // 4x u8 packed
            _ => 12,
        };

        Self {
            semantic,
            byte_size,
        }
    }
}

pub struct MeshStats {
    pub vertex_count: usize,
    pub triangle_count: usize,
    pub stride: usize,
    pub is_skinned: bool,
}

struct ParsedMeshData {
    positions: Vec<Vector3>,
    normals: Vec<Vector3>,
    uvs: Vec<Vector2>,
    weights: Vec<Vector4>,
    joints: Vec<[u16; 4]>,
    indices: Vec<u16>,
    bones: Vec<ObjectBone>,
    stride: usize,
    is_skinned: bool,
}

fn extract_mesh_geometry(chunk_data: &[u8]) -> Result<ParsedMeshData> {
    let mut bones = Vec::new();

    if let Ok(parsed_bones) = parse_object_bone_container(chunk_data) {
        bones = parsed_bones;
    }

    let mesh_data_bytes = if chunk_data.starts_with(magic::MESH) {
        let (_, elements) = parse_typed_container(chunk_data)?;
        elements
            .into_iter()
            .find(|(id, _)| *id == chunk_id::SUB_CONTAINER)
            .map(|(_, data)| data)
            .context("MeshAsset container is missing element ID 1 (MeshData)")?
    } else {
        chunk_data.to_vec()
    };

    let (_, sub_elements) = parse_chunk_elements(&mesh_data_bytes)?;

    let indice_chunk = sub_elements
        .iter()
        .find(|(id, _)| *id == chunk_id::INDEX_DATA)
        .map(|(_, data)| data)
        .context("Missing ID 10 (IndiceData)")?;

    let vertex_chunk = sub_elements
        .iter()
        .find(|(id, _)| *id == chunk_id::VERTEX_DATA)
        .map(|(_, data)| data)
        .context("Missing ID 11 (VertexBuffer)")?;

    let (_, indice_elements) = parse_chunk_elements(indice_chunk)?;
    let raw_indices = indice_elements
        .iter()
        .find(|(id, _)| *id == chunk_id::DATA_BLOB)
        .map(|(_, data)| data)
        .context("Missing index buffer blob ID 22")?;

    let mut indices = Vec::new();
    let mut cur_ind = Cursor::new(raw_indices);
    while (cur_ind.position() as usize) + 2 <= raw_indices.len() {
        indices.push(cur_ind.read_u16::<LittleEndian>()?);
    }

    let (_, vbuf_elements) = parse_chunk_elements(vertex_chunk)?;
    let info_chunk = vbuf_elements
        .iter()
        .find(|(id, _)| *id == chunk_id::TAG_STRING)
        .map(|(_, data)| data)
        .context("Missing VertexBufferInfo ID 20")?;

    let raw_vertices = vbuf_elements
        .iter()
        .find(|(id, _)| *id == chunk_id::DATA_BLOB)
        .map(|(_, data)| data)
        .context("Missing vertex buffer blob ID 22")?;

    let (_, info_elements) = parse_chunk_elements(info_chunk)?;
    let stride_data = info_elements
        .iter()
        .find(|(id, _)| *id == chunk_id::NAME_STRING)
        .map(|(_, data)| data)
        .context("Missing Stride ID 21")?;
    let stride = Cursor::new(stride_data).read_u32::<LittleEndian>()? as usize;

    let decl_data = info_elements
        .iter()
        .find(|(id, _)| *id == chunk_id::FORMAT)
        .map(|(_, data)| data)
        .context("Missing Attribute Descriptor Table ID 23")?;

    let mut cur_decl = Cursor::new(decl_data);
    let mut attributes = Vec::new();
    while (cur_decl.position() as usize) + 4 <= decl_data.len() {
        let desc = cur_decl.read_u32::<LittleEndian>()?;
        attributes.push(VertexAttribute::from_descriptor(desc));
    }

    let has_weights = attributes
        .iter()
        .any(|a| a.semantic == VertexSemantic::BlendWeights);
    let has_indices = attributes
        .iter()
        .any(|a| a.semantic == VertexSemantic::BlendIndices);
    let is_skinned = has_weights && has_indices;

    let vertex_count = raw_vertices.len() / stride;
    let mut positions = Vec::with_capacity(vertex_count);
    let mut normals = Vec::with_capacity(vertex_count);
    let mut uvs = Vec::with_capacity(vertex_count);
    let mut weights = Vec::with_capacity(vertex_count);
    let mut joints = Vec::with_capacity(vertex_count);

    for i in 0..vertex_count {
        let base = i * stride;
        let mut cur = Cursor::new(&raw_vertices[base..base + stride]);

        let mut pos = Vector3::default();
        let mut norm = Vector3::default();
        let mut uv = Vector2::default();
        let mut w_val = Vector4::default();
        let mut j_val = [0u16; 4];

        let mut weight_slot = 0;
        let mut joint_slot = 0;

        for attr in &attributes {
            match attr.semantic {
                VertexSemantic::Position => {
                    pos.x = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    pos.y = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    pos.z = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                }
                VertexSemantic::Normal => {
                    norm.x = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    norm.y = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    norm.z = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                }
                VertexSemantic::TexCoord => {
                    uv.x = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    uv.y = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                }
                VertexSemantic::BlendWeights => {
                    if attr.byte_size == 16 {
                        w_val.x = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                        w_val.y = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                        w_val.z = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                        w_val.w = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    } else if attr.byte_size == 12 {
                        w_val.x = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                        w_val.y = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                        w_val.z = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                        w_val.w = (1.0 - (w_val.x + w_val.y + w_val.z)).max(0.0);
                    } else if attr.byte_size == 1 {
                        // 1-byte normalized weight (u8 / 255.0)
                        let w_norm = cur.read_u8().unwrap_or(0) as f32 / 255.0;
                        match weight_slot {
                            0 => w_val.x = w_norm,
                            1 => w_val.y = w_norm,
                            2 => w_val.z = w_norm,
                            3 => w_val.w = w_norm,
                            _ => {}
                        }
                        weight_slot += 1;
                    } else {
                        let p = cur.position();
                        cur.set_position(p + attr.byte_size as u64);
                    }
                }
                VertexSemantic::BlendIndices => {
                    if attr.byte_size == 4 {
                        j_val[0] = cur.read_u8().unwrap_or(0) as u16;
                        j_val[1] = cur.read_u8().unwrap_or(0) as u16;
                        j_val[2] = cur.read_u8().unwrap_or(0) as u16;
                        j_val[3] = cur.read_u8().unwrap_or(0) as u16;
                    } else if attr.byte_size == 1 {
                        // 1-byte bone index
                        let j_byte = cur.read_u8().unwrap_or(0) as u16;
                        if joint_slot < 4 {
                            j_val[joint_slot] = j_byte;
                            joint_slot += 1;
                        }
                    } else {
                        let p = cur.position();
                        cur.set_position(p + attr.byte_size as u64);
                    }
                }
                _ => {
                    let p = cur.position();
                    cur.set_position(p + attr.byte_size as u64);
                }
            }
        }

        // Normalize weights so their sum is exactly 1.0
        let weight_sum = w_val.x + w_val.y + w_val.z + w_val.w;
        if weight_sum > 0.001 {
            w_val.x /= weight_sum;
            w_val.y /= weight_sum;
            w_val.z /= weight_sum;
            w_val.w /= weight_sum;
        } else {
            w_val.x = 1.0;
        }

        positions.push(pos);
        normals.push(norm);
        uvs.push(uv);
        if is_skinned {
            weights.push(w_val);
            joints.push(j_val);
        }
    }

    Ok(ParsedMeshData {
        positions,
        normals,
        uvs,
        weights,
        joints,
        indices,
        bones,
        stride,
        is_skinned,
    })
}

pub fn export_mesh_to_glb(chunk_data: &[u8]) -> Result<(Vec<u8>, MeshStats)> {
    let parsed = extract_mesh_geometry(chunk_data)?;
    let vertex_count = parsed.positions.len();

    let mut min_pos = [f32::INFINITY, f32::INFINITY, f32::INFINITY];
    let mut max_pos = [f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];

    for p in &parsed.positions {
        min_pos[0] = min_pos[0].min(p.x);
        min_pos[1] = min_pos[1].min(p.y);
        min_pos[2] = min_pos[2].min(p.z);
        max_pos[0] = max_pos[0].max(p.x);
        max_pos[1] = max_pos[1].max(p.y);
        max_pos[2] = max_pos[2].max(p.z);
    }

    if min_pos[0].is_infinite() {
        min_pos = [0.0, 0.0, 0.0];
        max_pos = [0.0, 0.0, 0.0];
    }

    let mut builder = GltfBuilder::new();

    let mut ind_bytes = Vec::with_capacity(parsed.indices.len() * 2);
    for idx in &parsed.indices {
        ind_bytes.write_u16::<LittleEndian>(*idx)?;
    }
    let ind_view = builder.add_buffer_view(&ind_bytes, Some(34963));

    let mut pos_bytes = Vec::with_capacity(vertex_count * 12);
    for p in &parsed.positions {
        pos_bytes.write_f32::<LittleEndian>(p.x)?;
        pos_bytes.write_f32::<LittleEndian>(p.y)?;
        pos_bytes.write_f32::<LittleEndian>(p.z)?;
    }
    let pos_view = builder.add_buffer_view(&pos_bytes, Some(34962));

    let mut norm_bytes = Vec::with_capacity(vertex_count * 12);
    for n in &parsed.normals {
        norm_bytes.write_f32::<LittleEndian>(n.x)?;
        norm_bytes.write_f32::<LittleEndian>(n.y)?;
        norm_bytes.write_f32::<LittleEndian>(n.z)?;
    }
    let norm_view = builder.add_buffer_view(&norm_bytes, Some(34962));

    let mut uv_bytes = Vec::with_capacity(vertex_count * 8);
    for uv in &parsed.uvs {
        uv_bytes.write_f32::<LittleEndian>(uv.x)?;
        uv_bytes.write_f32::<LittleEndian>(uv.y)?;
    }
    let uv_view = builder.add_buffer_view(&uv_bytes, Some(34962));

    let ind_acc = builder.add_accessor(ind_view, parsed.indices.len(), 5123, "SCALAR", None, None);
    let pos_acc = builder.add_accessor(
        pos_view,
        vertex_count,
        5126,
        "VEC3",
        Some(min_pos.to_vec()),
        Some(max_pos.to_vec()),
    );
    let norm_acc = builder.add_accessor(norm_view, vertex_count, 5126, "VEC3", None, None);
    let uv_acc = builder.add_accessor(uv_view, vertex_count, 5126, "VEC2", None, None);

    let mut prim_attributes = json!({
        "POSITION": pos_acc,
        "NORMAL": norm_acc,
        "TEXCOORD_0": uv_acc
    });

    let has_skinning = parsed.is_skinned
        && !parsed.joints.is_empty()
        && !parsed.weights.is_empty()
        && !parsed.bones.is_empty();

    if has_skinning {
        let mut joint_bytes = Vec::with_capacity(vertex_count * 8);
        for j in &parsed.joints {
            joint_bytes.write_u16::<LittleEndian>(j[0])?;
            joint_bytes.write_u16::<LittleEndian>(j[1])?;
            joint_bytes.write_u16::<LittleEndian>(j[2])?;
            joint_bytes.write_u16::<LittleEndian>(j[3])?;
        }
        let joint_view = builder.add_buffer_view(&joint_bytes, Some(34962));
        let joint_acc = builder.add_accessor(joint_view, vertex_count, 5123, "VEC4", None, None);

        let mut weight_bytes = Vec::with_capacity(vertex_count * 16);
        for w in &parsed.weights {
            weight_bytes.write_f32::<LittleEndian>(w.x)?;
            weight_bytes.write_f32::<LittleEndian>(w.y)?;
            weight_bytes.write_f32::<LittleEndian>(w.z)?;
            weight_bytes.write_f32::<LittleEndian>(w.w)?;
        }
        let weight_view = builder.add_buffer_view(&weight_bytes, Some(34962));
        let weight_acc = builder.add_accessor(weight_view, vertex_count, 5126, "VEC4", None, None);

        prim_attributes["JOINTS_0"] = json!(joint_acc);
        prim_attributes["WEIGHTS_0"] = json!(weight_acc);
    }

    let mesh_idx = builder.add_mesh(json!({
        "name": "OverlordMesh",
        "primitives": [{
            "attributes": prim_attributes,
            "indices": ind_acc,
            "mode": 4
        }]
    }));

    if has_skinning {
        let num_bones = parsed.bones.len();
        let mut armature_children = Vec::new();
        let mut bone_nodes = Vec::new();

        for (i, bone) in parsed.bones.iter().enumerate() {
            let bone_node_idx = i + 1;

            let mut children = Vec::new();
            for (j, b2) in parsed.bones.iter().enumerate() {
                if b2.parent_index == i as i32 && j != i {
                    children.push(j + 1);
                }
            }

            if bone.parent_index < 0 || bone.parent_index >= num_bones as i32 {
                armature_children.push(bone_node_idx);
            }

            let bname = if bone.name.is_empty() {
                format!("Bone_{:03}", i)
            } else {
                bone.name.clone()
            };

            let mut bnode = json!({
                "name": bname,
                "translation": [bone.translation.x, bone.translation.y, bone.translation.z],
                "rotation": [bone.rotation.x, bone.rotation.y, bone.rotation.z, bone.rotation.w]
            });
            if !children.is_empty() {
                bnode["children"] = json!(children);
            }
            bone_nodes.push(bnode);
        }

        builder.add_node(json!({
            "name": "Armature",
            "children": armature_children
        }));

        for bnode in bone_nodes {
            builder.add_node(bnode);
        }

        let joint_indices: Vec<usize> = (1..=num_bones).collect();
        let skin_idx = builder.add_skin(json!({
            "name": "Mesh_Skin",
            "joints": joint_indices
        }));

        let mesh_node_idx = num_bones + 1;
        builder.add_node(json!({
            "name": "SkinnedMesh",
            "mesh": mesh_idx,
            "skin": skin_idx
        }));

        builder.add_scene(vec![0, mesh_node_idx]);
    } else {
        builder.add_node(json!({
            "name": "StaticMesh",
            "mesh": mesh_idx
        }));
        builder.add_scene(vec![0]);
    }

    let glb = builder.build("Overlord Modding Studio glTF Exporter")?;

    let stats = MeshStats {
        vertex_count,
        triangle_count: parsed.indices.len() / 3,
        stride: parsed.stride,
        is_skinned: has_skinning,
    };

    Ok((glb, stats))
}

pub fn import_glb_to_mesh(original_chunk: &[u8], glb_bytes: &[u8]) -> Result<Vec<u8>> {
    if glb_bytes.len() < 20 || &glb_bytes[0..4] != b"glTF" {
        bail!("Invalid .glb file format (missing 'glTF' signature)");
    }

    let original_parsed = extract_mesh_geometry(original_chunk).ok();
    let original_was_skinned = original_parsed.as_ref().is_some_and(|p| p.is_skinned);

    let mut cur = Cursor::new(&glb_bytes[12..]);
    let json_len = cur.read_u32::<LittleEndian>()? as usize;
    let mut json_type = [0u8; 4];
    std::io::Read::read_exact(&mut cur, &mut json_type)?;

    if &json_type != b"JSON" {
        bail!("Missing JSON chunk in .glb");
    }

    let json_pos = 20;
    let json_slice = &glb_bytes[json_pos..json_pos + json_len];
    let gltf: serde_json::Value = serde_json::from_slice(json_slice)?;

    let bin_header_pos = 20 + json_len;
    if bin_header_pos + 8 > glb_bytes.len() {
        bail!("Missing BIN chunk in .glb");
    }

    let mut bin_cur = Cursor::new(&glb_bytes[bin_header_pos..]);
    let bin_len = bin_cur.read_u32::<LittleEndian>()? as usize;
    let bin_pos = bin_header_pos + 8;
    let bin_data = &glb_bytes[bin_pos..(bin_pos + bin_len).min(glb_bytes.len())];

    let prim = gltf["meshes"]
        .get(0)
        .and_then(|m| m["primitives"].get(0))
        .context("No mesh primitives found in glTF")?;

    let accessors = gltf["accessors"]
        .as_array()
        .context("glTF missing accessors array")?;
    let buffer_views = gltf["bufferViews"]
        .as_array()
        .context("glTF missing bufferViews array")?;

    let read_buffer_view_slice = |accessor_idx: usize| -> Result<&[u8]> {
        let acc = accessors
            .get(accessor_idx)
            .context(format!("Missing accessor {}", accessor_idx))?;
        let bv_idx = acc["bufferView"]
            .as_u64()
            .context("Accessor missing bufferView")? as usize;
        let bv = buffer_views
            .get(bv_idx)
            .context(format!("Missing bufferView {}", bv_idx))?;

        let bv_offset = bv["byteOffset"].as_u64().unwrap_or(0) as usize;
        let acc_offset = acc["byteOffset"].as_u64().unwrap_or(0) as usize;
        let total_offset = bv_offset + acc_offset;

        if total_offset > bin_data.len() {
            bail!("Accessor offset exceeds binary buffer");
        }

        Ok(&bin_data[total_offset..])
    };

    let pos_acc_idx = prim["attributes"]["POSITION"]
        .as_u64()
        .context("Primitive missing POSITION")? as usize;
    let pos_count = accessors[pos_acc_idx]["count"].as_u64().unwrap_or(0) as usize;
    let pos_slice = read_buffer_view_slice(pos_acc_idx)?;
    let mut pos_cur = Cursor::new(pos_slice);

    let mut raw_positions = Vec::with_capacity(pos_count);
    for _ in 0..pos_count {
        raw_positions.push(Vector3 {
            x: pos_cur.read_f32::<LittleEndian>().unwrap_or(0.0),
            y: pos_cur.read_f32::<LittleEndian>().unwrap_or(0.0),
            z: pos_cur.read_f32::<LittleEndian>().unwrap_or(0.0),
        });
    }

    let mut raw_normals = vec![
        Vector3 {
            x: 0.0,
            y: 1.0,
            z: 0.0
        };
        pos_count
    ];
    if let Some(norm_acc_idx) = prim["attributes"]["NORMAL"].as_u64().map(|v| v as usize)
        && let Ok(norm_slice) = read_buffer_view_slice(norm_acc_idx)
    {
        let mut norm_cur = Cursor::new(norm_slice);
        for n in raw_normals.iter_mut().take(pos_count) {
            *n = Vector3 {
                x: norm_cur.read_f32::<LittleEndian>().unwrap_or(0.0),
                y: norm_cur.read_f32::<LittleEndian>().unwrap_or(0.0),
                z: norm_cur.read_f32::<LittleEndian>().unwrap_or(0.0),
            };
        }
    }

    let mut raw_uvs = vec![Vector2::default(); pos_count];
    if let Some(uv_acc_idx) = prim["attributes"]["TEXCOORD_0"]
        .as_u64()
        .map(|v| v as usize)
        && let Ok(uv_slice) = read_buffer_view_slice(uv_acc_idx)
    {
        let mut uv_cur = Cursor::new(uv_slice);
        for uv in raw_uvs.iter_mut().take(pos_count) {
            *uv = Vector2 {
                x: uv_cur.read_f32::<LittleEndian>().unwrap_or(0.0),
                y: uv_cur.read_f32::<LittleEndian>().unwrap_or(0.0),
            };
        }
    }

    let has_glb_skinning =
        prim["attributes"]["JOINTS_0"].is_number() && prim["attributes"]["WEIGHTS_0"].is_number();

    let mut raw_joints = vec![[0u8; 4]; pos_count];
    let mut raw_weights = vec![
        Vector4 {
            x: 1.0,
            y: 0.0,
            z: 0.0,
            w: 0.0
        };
        pos_count
    ];

    if has_glb_skinning {
        let j_acc_idx = prim["attributes"]["JOINTS_0"].as_u64().unwrap() as usize;
        let w_acc_idx = prim["attributes"]["WEIGHTS_0"].as_u64().unwrap() as usize;

        let j_comp = accessors[j_acc_idx]["componentType"]
            .as_u64()
            .unwrap_or(5123);
        let j_slice = read_buffer_view_slice(j_acc_idx)?;
        let mut j_cur = Cursor::new(j_slice);

        for j in raw_joints.iter_mut().take(pos_count) {
            if j_comp == 5121 {
                *j = [
                    j_cur.read_u8().unwrap_or(0),
                    j_cur.read_u8().unwrap_or(0),
                    j_cur.read_u8().unwrap_or(0),
                    j_cur.read_u8().unwrap_or(0),
                ];
            } else {
                *j = [
                    j_cur.read_u16::<LittleEndian>().unwrap_or(0) as u8,
                    j_cur.read_u16::<LittleEndian>().unwrap_or(0) as u8,
                    j_cur.read_u16::<LittleEndian>().unwrap_or(0) as u8,
                    j_cur.read_u16::<LittleEndian>().unwrap_or(0) as u8,
                ];
            }
        }

        let w_slice = read_buffer_view_slice(w_acc_idx)?;
        let mut w_cur = Cursor::new(w_slice);
        for w in raw_weights.iter_mut().take(pos_count) {
            *w = Vector4 {
                x: w_cur.read_f32::<LittleEndian>().unwrap_or(1.0),
                y: w_cur.read_f32::<LittleEndian>().unwrap_or(0.0),
                z: w_cur.read_f32::<LittleEndian>().unwrap_or(0.0),
                w: w_cur.read_f32::<LittleEndian>().unwrap_or(0.0),
            };
        }
    }

    let target_skinned = has_glb_skinning || original_was_skinned;

    let indices_acc_idx = prim["indices"]
        .as_u64()
        .context("Primitive missing indices")? as usize;
    let indices_count = accessors[indices_acc_idx]["count"].as_u64().unwrap_or(0) as usize;
    let indices_comp = accessors[indices_acc_idx]["componentType"]
        .as_u64()
        .unwrap_or(5123);
    let indices_slice = read_buffer_view_slice(indices_acc_idx)?;
    let mut idx_cur = Cursor::new(indices_slice);

    let mut index_buffer = Vec::with_capacity(indices_count);
    for _ in 0..indices_count {
        let idx = if indices_comp == 5125 {
            idx_cur.read_u32::<LittleEndian>().unwrap_or(0) as u16
        } else {
            idx_cur.read_u16::<LittleEndian>().unwrap_or(0)
        };
        index_buffer.push(idx);
    }

    let stride = if target_skinned { 52u32 } else { 32u32 };
    let mut vertex_buffer = Vec::with_capacity(pos_count * stride as usize);
    let mut cur_vbuf = Cursor::new(&mut vertex_buffer);

    for i in 0..pos_count {
        let p = raw_positions[i];
        let n = raw_normals[i];
        let uv = raw_uvs[i];

        cur_vbuf.write_f32::<LittleEndian>(p.x)?;
        cur_vbuf.write_f32::<LittleEndian>(p.y)?;
        cur_vbuf.write_f32::<LittleEndian>(p.z)?;

        cur_vbuf.write_f32::<LittleEndian>(n.x)?;
        cur_vbuf.write_f32::<LittleEndian>(n.y)?;
        cur_vbuf.write_f32::<LittleEndian>(n.z)?;

        cur_vbuf.write_f32::<LittleEndian>(uv.x)?;
        cur_vbuf.write_f32::<LittleEndian>(uv.y)?;

        if target_skinned {
            let w = raw_weights[i];
            let j = raw_joints[i];

            cur_vbuf.write_f32::<LittleEndian>(w.x)?;
            cur_vbuf.write_f32::<LittleEndian>(w.y)?;
            cur_vbuf.write_f32::<LittleEndian>(w.z)?;
            cur_vbuf.write_f32::<LittleEndian>(w.w)?;

            cur_vbuf.write_u8(j[0])?;
            cur_vbuf.write_u8(j[1])?;
            cur_vbuf.write_u8(j[2])?;
            cur_vbuf.write_u8(j[3])?;
        }
    }

    let mut raw_indices_bytes = Vec::with_capacity(index_buffer.len() * 2);
    let mut cur_idx = Cursor::new(&mut raw_indices_bytes);
    for idx in &index_buffer {
        cur_idx.write_u16::<LittleEndian>(*idx)?;
    }

    let indice_elements = vec![
        (20, vec![0u8]),
        (21, (index_buffer.len() as u32).to_le_bytes().to_vec()),
        (22, raw_indices_bytes),
    ];
    let new_indice_chunk = build_chunk_from_elements(false, &indice_elements);

    let mut attr_table = Vec::new();
    let mut cur_attr = Cursor::new(&mut attr_table);
    cur_attr.write_u32::<LittleEndian>(0x02010000)?; // Position
    cur_attr.write_u32::<LittleEndian>(0x02040000)?; // Normal
    cur_attr.write_u32::<LittleEndian>(0x01050000)?; // TexCoord

    let attr_count = if target_skinned {
        cur_attr.write_u32::<LittleEndian>(0x030A0000)?; // BlendWeights
        cur_attr.write_u32::<LittleEndian>(0x0F0B0000)?; // BlendIndices
        5u32
    } else {
        3u32
    };

    let info_elements = vec![
        (20, vec![0u8]),
        (21, stride.to_le_bytes().to_vec()),
        (22, attr_count.to_le_bytes().to_vec()),
        (23, attr_table),
    ];
    let new_info_chunk = build_chunk_from_elements(false, &info_elements);

    let vbuf_elements = vec![
        (20, new_info_chunk),
        (21, (pos_count as u32).to_le_bytes().to_vec()),
        (22, vertex_buffer),
    ];
    let new_vbuf_chunk = build_chunk_from_elements(false, &vbuf_elements);

    let is_mesh_asset = original_chunk.starts_with(magic::MESH);
    let mesh_data_bytes = if is_mesh_asset {
        let (_, elements) = parse_typed_container(original_chunk)?;
        elements
            .into_iter()
            .find(|(id, _)| *id == 1)
            .map(|(_, data)| data)
            .unwrap_or_default()
    } else {
        original_chunk.to_vec()
    };

    let (has_magic, mut mesh_elements) = parse_chunk_elements(&mesh_data_bytes)?;
    mesh_elements.retain(|(id, _)| *id != 10 && *id != 11);
    mesh_elements.push((10, new_indice_chunk));
    mesh_elements.push((11, new_vbuf_chunk));
    mesh_elements.sort_by_key(|&(id, _)| id);

    let final_mesh_data = build_chunk_from_elements(has_magic, &mesh_elements);

    if is_mesh_asset {
        let (type_id, mut asset_elements) = parse_typed_container(original_chunk)?;
        for (id, data) in asset_elements.iter_mut() {
            if *id == 1 {
                *data = final_mesh_data.clone();
            }
        }
        Ok(build_typed_container(type_id, &asset_elements))
    } else {
        Ok(final_mesh_data)
    }
}

pub fn export_mesh_to_obj(chunk_data: &[u8]) -> Result<(String, MeshStats)> {
    let parsed = extract_mesh_geometry(chunk_data)?;
    let vertex_count = parsed.positions.len();

    let mut obj = String::new();
    obj.push_str("# Exported from Overlord Modding Studio\n");
    obj.push_str("o OverlordMesh\n\n");

    for p in &parsed.positions {
        obj.push_str(&format!("v {:.6} {:.6} {:.6}\n", p.x, p.y, p.z));
    }

    for t in &parsed.uvs {
        obj.push_str(&format!("vt {:.6} {:.6}\n", t.x, 1.0 - t.y));
    }

    for n in &parsed.normals {
        obj.push_str(&format!("vn {:.6} {:.6} {:.6}\n", n.x, n.y, n.z));
    }

    obj.push_str("\ns 1\n");
    for tri in parsed.indices.chunks(3) {
        if tri.len() == 3 {
            let i1 = tri[0] as usize + 1;
            let i2 = tri[1] as usize + 1;
            let i3 = tri[2] as usize + 1;
            obj.push_str(&format!(
                "f {0}/{0}/{0} {1}/{1}/{1} {2}/{2}/{2}\n",
                i1, i2, i3
            ));
        }
    }

    let stats = MeshStats {
        vertex_count,
        triangle_count: parsed.indices.len() / 3,
        stride: parsed.stride,
        is_skinned: parsed.is_skinned,
    };

    Ok((obj, stats))
}

pub fn import_obj_to_mesh(original_chunk: &[u8], obj_content: &str) -> Result<Vec<u8>> {
    let original_parsed = extract_mesh_geometry(original_chunk).ok();
    let target_skinned = original_parsed.as_ref().is_some_and(|p| p.is_skinned);

    let mut raw_positions = Vec::new();
    let mut raw_normals = Vec::new();
    let mut raw_uvs = Vec::new();
    let mut faces = Vec::new();

    for line in obj_content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let prefix = parts.next().unwrap_or("");

        match prefix {
            "v" => {
                let x: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                let y: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                let z: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                raw_positions.push(Vector3 { x, y, z });
            }
            "vn" => {
                let x: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                let y: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                let z: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                raw_normals.push(Vector3 { x, y, z });
            }
            "vt" => {
                let u: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                let v: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                raw_uvs.push(Vector2 { x: u, y: 1.0 - v });
            }
            "f" => {
                let face_verts: Vec<&str> = parts.collect();
                if face_verts.len() >= 3 {
                    for i in 1..face_verts.len() - 1 {
                        faces.push((face_verts[0], face_verts[i], face_verts[i + 1]));
                    }
                }
            }
            _ => {}
        }
    }

    if raw_positions.is_empty() {
        bail!("OBJ file contains no vertex positions ('v').");
    }

    let mut unique_vertices = HashMap::new();
    let mut vertex_buffer = Vec::new();
    let mut index_buffer = Vec::new();

    let parse_face_token = |token: &str| -> (usize, usize, usize) {
        let parts: Vec<&str> = token.split('/').collect();
        let v = parts
            .first()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(1)
            .saturating_sub(1);
        let vt = parts
            .get(1)
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(1)
            .saturating_sub(1);
        let vn = parts
            .get(2)
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(1)
            .saturating_sub(1);
        (v, vt, vn)
    };

    for (t0, t1, t2) in faces {
        for token in [t0, t1, t2] {
            let key = parse_face_token(token);
            if let Some(&existing_idx) = unique_vertices.get(&key) {
                index_buffer.push(existing_idx);
            } else {
                let new_idx = unique_vertices.len() as u16;
                unique_vertices.insert(key, new_idx);
                index_buffer.push(new_idx);

                let pos = raw_positions.get(key.0).cloned().unwrap_or_default();
                let norm = raw_normals.get(key.2).cloned().unwrap_or(Vector3 {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                });
                let uv = raw_uvs.get(key.1).cloned().unwrap_or_default();

                let mut cur = Cursor::new(&mut vertex_buffer);
                cur.set_position(cur.get_ref().len() as u64);
                cur.write_f32::<LittleEndian>(pos.x)?;
                cur.write_f32::<LittleEndian>(pos.y)?;
                cur.write_f32::<LittleEndian>(pos.z)?;
                cur.write_f32::<LittleEndian>(norm.x)?;
                cur.write_f32::<LittleEndian>(norm.y)?;
                cur.write_f32::<LittleEndian>(norm.z)?;
                cur.write_f32::<LittleEndian>(uv.x)?;
                cur.write_f32::<LittleEndian>(uv.y)?;

                if target_skinned {
                    cur.write_f32::<LittleEndian>(1.0)?;
                    cur.write_f32::<LittleEndian>(0.0)?;
                    cur.write_f32::<LittleEndian>(0.0)?;
                    cur.write_f32::<LittleEndian>(0.0)?;

                    cur.write_u8(0)?;
                    cur.write_u8(0)?;
                    cur.write_u8(0)?;
                    cur.write_u8(0)?;
                }
            }
        }
    }

    let pos_count = unique_vertices.len();
    let stride = if target_skinned { 52u32 } else { 32u32 };

    let mut raw_indices_bytes = Vec::new();
    let mut cur_idx = Cursor::new(&mut raw_indices_bytes);
    for idx in &index_buffer {
        cur_idx.write_u16::<LittleEndian>(*idx)?;
    }

    let indice_elements = vec![
        (20, vec![0u8]),
        (21, (index_buffer.len() as u32).to_le_bytes().to_vec()),
        (22, raw_indices_bytes),
    ];
    let new_indice_chunk = build_chunk_from_elements(false, &indice_elements);

    let mut attr_table = Vec::new();
    let mut cur_attr = Cursor::new(&mut attr_table);
    cur_attr.write_u32::<LittleEndian>(0x02010000)?;
    cur_attr.write_u32::<LittleEndian>(0x02040000)?;
    cur_attr.write_u32::<LittleEndian>(0x01050000)?;

    let attr_count = if target_skinned {
        cur_attr.write_u32::<LittleEndian>(0x030A0000)?;
        cur_attr.write_u32::<LittleEndian>(0x0F0B0000)?;
        5u32
    } else {
        3u32
    };

    let info_elements = vec![
        (20, vec![0u8]),
        (21, stride.to_le_bytes().to_vec()),
        (22, attr_count.to_le_bytes().to_vec()),
        (23, attr_table),
    ];
    let new_info_chunk = build_chunk_from_elements(false, &info_elements);

    let vbuf_elements = vec![
        (20, new_info_chunk),
        (21, (pos_count as u32).to_le_bytes().to_vec()),
        (22, vertex_buffer),
    ];
    let new_vbuf_chunk = build_chunk_from_elements(false, &vbuf_elements);

    let is_mesh_asset = original_chunk.starts_with(magic::MESH);
    let mesh_data_bytes = if is_mesh_asset {
        let (_, elements) = parse_typed_container(original_chunk)?;
        elements
            .into_iter()
            .find(|(id, _)| *id == 1)
            .map(|(_, data)| data)
            .unwrap_or_default()
    } else {
        original_chunk.to_vec()
    };

    let (has_magic, mut mesh_elements) = parse_chunk_elements(&mesh_data_bytes)?;
    mesh_elements.retain(|(id, _)| *id != 10 && *id != 11);
    mesh_elements.push((10, new_indice_chunk));
    mesh_elements.push((11, new_vbuf_chunk));
    mesh_elements.sort_by_key(|&(id, _)| id);

    let final_mesh_data = build_chunk_from_elements(has_magic, &mesh_elements);

    if is_mesh_asset {
        let (type_id, mut asset_elements) = parse_typed_container(original_chunk)?;
        for (id, data) in asset_elements.iter_mut() {
            if *id == 1 {
                *data = final_mesh_data.clone();
            }
        }
        Ok(build_typed_container(type_id, &asset_elements))
    } else {
        Ok(final_mesh_data)
    }
}
