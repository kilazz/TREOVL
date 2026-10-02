use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde_json::json;
use std::collections::HashMap;
use std::io::{Cursor, Write};

use super::animation::{ObjectBone, parse_object_bone_container};
use super::{
    build_chunk_from_elements, build_typed_container, parse_chunk_elements, parse_typed_container,
};
use crate::engine::common::{chunk_id, magic};
use crate::engine::math::{Vector2, Vector3, Vector4};
use crate::utils::gltf_builder::GltfBuilder;
use crate::utils::tangents::generate_tangents;

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
    pub raw_descriptor: u32,
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
            raw_descriptor: desc,
        }
    }
}

pub struct MeshStats {
    pub vertex_count: usize,
    pub triangle_count: usize,
    pub stride: usize,
    pub is_skinned: bool,
}

pub struct ParsedMeshData {
    pub positions: Vec<Vector3>,
    pub normals: Vec<Vector3>,
    pub uvs: Vec<Vector2>,
    pub weights: Vec<Vector4>,
    pub joints: Vec<[u16; 4]>,
    pub indices: Vec<u32>,
    pub bones: Vec<ObjectBone>,
    pub stride: usize,
    pub is_skinned: bool,
    pub attributes: Vec<VertexAttribute>,
    pub raw_descriptors: Vec<u32>,
}

pub struct VertexStreams<'a> {
    pub positions: &'a [Vector3],
    pub normals: &'a [Vector3],
    pub uvs: &'a [Vector2],
    pub weights: &'a [Vector4],
    pub joints: &'a [[u8; 4]],
    pub tangents: &'a [Vector4],
}

pub fn extract_mesh_geometry(chunk_data: &[u8]) -> Result<ParsedMeshData> {
    let bones = parse_object_bone_container(chunk_data).unwrap_or_default();

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

    let flag_val = indice_elements
        .iter()
        .find(|(id, _)| *id == chunk_id::TAG_STRING)
        .and_then(|(_, d)| d.first().copied())
        .unwrap_or(0);

    let declared_count = indice_elements
        .iter()
        .find(|(id, _)| *id == chunk_id::NAME_STRING)
        .and_then(|(_, d)| Cursor::new(d).read_u32::<LittleEndian>().ok())
        .unwrap_or(0) as usize;

    let is_32bit = if declared_count > 0 {
        raw_indices.len() == declared_count * 4
    } else {
        flag_val != 0 || (raw_indices.len().is_multiple_of(4) && raw_indices.len() > 131_072)
    };

    let indices: Vec<u32> = if is_32bit {
        if (raw_indices.as_ptr() as usize).is_multiple_of(4) {
            bytemuck::cast_slice::<u8, u32>(raw_indices).to_vec()
        } else {
            raw_indices
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&c| u32::from_le_bytes(c))
                .collect()
        }
    } else if (raw_indices.as_ptr() as usize).is_multiple_of(2) {
        bytemuck::cast_slice::<u8, u16>(raw_indices)
            .iter()
            .map(|&idx| idx as u32)
            .collect()
    } else {
        raw_indices
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c) as u32)
            .collect()
    };

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
    let mut raw_descriptors = Vec::new();
    while (cur_decl.position() as usize) + 4 <= decl_data.len() {
        let desc = cur_decl.read_u32::<LittleEndian>()?;
        raw_descriptors.push(desc);
        attributes.push(VertexAttribute::from_descriptor(desc));
    }

    let has_weights = attributes
        .iter()
        .any(|a| a.semantic == VertexSemantic::BlendWeights);
    let has_indices = attributes
        .iter()
        .any(|a| a.semantic == VertexSemantic::BlendIndices);
    let is_skinned = has_weights && has_indices;

    let num_bones = bones.len();
    let vertex_count = raw_vertices.len() / stride;
    let mut positions = Vec::with_capacity(vertex_count);
    let mut normals = Vec::with_capacity(vertex_count);
    let mut uvs = Vec::with_capacity(vertex_count);
    let mut weights = Vec::with_capacity(vertex_count);
    let mut joints = Vec::with_capacity(vertex_count);

    for i in 0..vertex_count {
        let base = i * stride;
        let vdata = &raw_vertices[base..base + stride];

        let mut pos = Vector3::default();
        let mut norm = Vector3::default();
        let mut uv = Vector2::default();
        let mut w_val = Vector4::default();
        let mut j_val = [0u16; 4];

        let mut weight_slot = 0;
        let mut joint_slot = 0;
        let mut offset = 0;

        for attr in &attributes {
            match attr.semantic {
                VertexSemantic::Position => {
                    let val: [f32; 3] = bytemuck::pod_read_unaligned(&vdata[offset..offset + 12]);
                    pos = Vector3 {
                        x: val[0],
                        y: val[1],
                        z: val[2],
                    };
                }
                VertexSemantic::Normal => {
                    let val: [f32; 3] = bytemuck::pod_read_unaligned(&vdata[offset..offset + 12]);
                    norm = Vector3 {
                        x: val[0],
                        y: val[1],
                        z: val[2],
                    };
                }
                VertexSemantic::TexCoord => {
                    let val: [f32; 2] = bytemuck::pod_read_unaligned(&vdata[offset..offset + 8]);
                    uv = Vector2 {
                        x: val[0],
                        y: val[1],
                    };
                }
                VertexSemantic::BlendWeights => {
                    if attr.byte_size == 16 {
                        let val: [f32; 4] =
                            bytemuck::pod_read_unaligned(&vdata[offset..offset + 16]);
                        w_val = Vector4 {
                            x: val[0],
                            y: val[1],
                            z: val[2],
                            w: val[3],
                        };
                    } else if attr.byte_size == 12 {
                        let val: [f32; 3] =
                            bytemuck::pod_read_unaligned(&vdata[offset..offset + 12]);
                        w_val = Vector4 {
                            x: val[0],
                            y: val[1],
                            z: val[2],
                            w: (1.0 - (val[0] + val[1] + val[2])).max(0.0),
                        };
                    } else if attr.byte_size == 1 {
                        let w_norm = vdata[offset] as f32 / 255.0;
                        match weight_slot {
                            0 => w_val.x = w_norm,
                            1 => w_val.y = w_norm,
                            2 => w_val.z = w_norm,
                            3 => w_val.w = w_norm,
                            _ => {}
                        }
                        weight_slot += 1;
                    }
                }
                VertexSemantic::BlendIndices => {
                    if attr.byte_size == 4 {
                        let val: [u8; 4] = bytemuck::pod_read_unaligned(&vdata[offset..offset + 4]);
                        j_val = [val[0] as u16, val[1] as u16, val[2] as u16, val[3] as u16];
                    } else if attr.byte_size == 1 && joint_slot < 4 {
                        j_val[joint_slot] = vdata[offset] as u16;
                        joint_slot += 1;
                    }
                }
                _ => {}
            }
            offset += attr.byte_size;
        }

        if joint_slot == 3 && weight_slot == 2 {
            w_val.z = (1.0 - (w_val.x + w_val.y)).max(0.0);
        } else if joint_slot == 2 && weight_slot == 1 {
            w_val.y = (1.0 - w_val.x).max(0.0);
        }

        let weight_sum = w_val.x + w_val.y + w_val.z + w_val.w;
        if weight_sum > 0.001 {
            w_val.x /= weight_sum;
            w_val.y /= weight_sum;
            w_val.z /= weight_sum;
            w_val.w /= weight_sum;
        } else {
            w_val.x = 1.0;
        }

        if num_bones > 0 {
            let max_bone_idx = (num_bones - 1) as u16;
            j_val[0] = j_val[0].min(max_bone_idx);
            j_val[1] = j_val[1].min(max_bone_idx);
            j_val[2] = j_val[2].min(max_bone_idx);
            j_val[3] = j_val[3].min(max_bone_idx);
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
        attributes,
        raw_descriptors,
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

    let max_idx = parsed.indices.iter().copied().max().unwrap_or(0);
    let (ind_view, comp_type) = if max_idx <= 0xFFFF {
        let mut ind_bytes = Vec::with_capacity(parsed.indices.len() * 2);
        for &idx in &parsed.indices {
            ind_bytes.write_u16::<LittleEndian>(idx as u16)?;
        }
        (builder.add_buffer_view(&ind_bytes, Some(34963)), 5123)
    } else {
        let mut ind_bytes = Vec::with_capacity(parsed.indices.len() * 4);
        for &idx in &parsed.indices {
            ind_bytes.write_u32::<LittleEndian>(idx)?;
        }
        (builder.add_buffer_view(&ind_bytes, Some(34963)), 5125)
    };

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

    let ind_acc = builder.add_accessor(
        ind_view,
        parsed.indices.len(),
        comp_type,
        "SCALAR",
        None,
        None,
    );
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

        if armature_children.is_empty() {
            armature_children.push(1);
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

fn rebuild_mesh_container(
    original_chunk: &[u8],
    vertex_buffer: Vec<u8>,
    index_buffer: &[u32],
    pos_count: usize,
    stride: u32,
    raw_descriptors: &[u32],
) -> Result<Vec<u8>> {
    let max_idx = index_buffer.iter().copied().max().unwrap_or(0);
    let use_32bit = max_idx > 0xFFFF || pos_count > 0xFFFF;

    let (raw_indices_bytes, flag_val) = if use_32bit {
        let mut raw = Vec::with_capacity(index_buffer.len() * 4);
        let mut cur = Cursor::new(&mut raw);
        for &idx in index_buffer {
            cur.write_u32::<LittleEndian>(idx)?;
        }
        (raw, 1u8)
    } else {
        let mut raw = Vec::with_capacity(index_buffer.len() * 2);
        let mut cur = Cursor::new(&mut raw);
        for &idx in index_buffer {
            cur.write_u16::<LittleEndian>(idx as u16)?;
        }
        (raw, 0u8)
    };

    let indice_elements = vec![
        (20, vec![flag_val]),
        (21, (index_buffer.len() as u32).to_le_bytes().to_vec()),
        (22, raw_indices_bytes),
    ];
    let new_indice_chunk = build_chunk_from_elements(false, &indice_elements);

    let mut attr_table = Vec::with_capacity(raw_descriptors.len() * 4);
    let mut cur_attr = Cursor::new(&mut attr_table);
    for &desc in raw_descriptors {
        cur_attr.write_u32::<LittleEndian>(desc)?;
    }

    let info_elements = vec![
        (20, vec![0u8]),
        (21, stride.to_le_bytes().to_vec()),
        (22, (raw_descriptors.len() as u32).to_le_bytes().to_vec()),
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

fn pack_vertex_buffer_preserving_fvf(
    pos_count: usize,
    attributes: &[VertexAttribute],
    stride: usize,
    streams: &VertexStreams,
) -> Result<Vec<u8>> {
    let mut vertex_buffer = Vec::with_capacity(pos_count * stride);
    let mut cur_vbuf = Cursor::new(&mut vertex_buffer);

    for i in 0..pos_count {
        for attr in attributes {
            match attr.semantic {
                VertexSemantic::Position => {
                    let p = streams.positions.get(i).copied().unwrap_or_default();
                    cur_vbuf.write_f32::<LittleEndian>(p.x)?;
                    cur_vbuf.write_f32::<LittleEndian>(p.y)?;
                    cur_vbuf.write_f32::<LittleEndian>(p.z)?;
                }
                VertexSemantic::Normal => {
                    let n = streams.normals.get(i).copied().unwrap_or(Vector3 {
                        x: 0.0,
                        y: 1.0,
                        z: 0.0,
                    });
                    cur_vbuf.write_f32::<LittleEndian>(n.x)?;
                    cur_vbuf.write_f32::<LittleEndian>(n.y)?;
                    cur_vbuf.write_f32::<LittleEndian>(n.z)?;
                }
                VertexSemantic::TexCoord => {
                    let uv = streams.uvs.get(i).copied().unwrap_or_default();
                    cur_vbuf.write_f32::<LittleEndian>(uv.x)?;
                    cur_vbuf.write_f32::<LittleEndian>(uv.y)?;
                }
                VertexSemantic::TangentQuat => {
                    let t = streams.tangents.get(i).copied().unwrap_or(Vector4 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                        w: 1.0,
                    });
                    cur_vbuf.write_f32::<LittleEndian>(t.x)?;
                    cur_vbuf.write_f32::<LittleEndian>(t.y)?;
                    cur_vbuf.write_f32::<LittleEndian>(t.z)?;
                    cur_vbuf.write_f32::<LittleEndian>(t.w)?;
                }
                VertexSemantic::Color => {
                    cur_vbuf.write_u32::<LittleEndian>(0xFFFFFFFF)?;
                }
                VertexSemantic::BlendWeights => {
                    let w = streams.weights.get(i).copied().unwrap_or(Vector4 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                        w: 0.0,
                    });
                    if attr.byte_size == 16 {
                        cur_vbuf.write_f32::<LittleEndian>(w.x)?;
                        cur_vbuf.write_f32::<LittleEndian>(w.y)?;
                        cur_vbuf.write_f32::<LittleEndian>(w.z)?;
                        cur_vbuf.write_f32::<LittleEndian>(w.w)?;
                    } else if attr.byte_size == 12 {
                        cur_vbuf.write_f32::<LittleEndian>(w.x)?;
                        cur_vbuf.write_f32::<LittleEndian>(w.y)?;
                        cur_vbuf.write_f32::<LittleEndian>(w.z)?;
                    } else {
                        cur_vbuf.write_all(&[255, 0, 0, 0][..attr.byte_size])?;
                    }
                }
                VertexSemantic::BlendIndices => {
                    let j = streams.joints.get(i).copied().unwrap_or([0, 0, 0, 0]);
                    cur_vbuf.write_all(&j[..attr.byte_size.min(4)])?;
                }
                VertexSemantic::Unknown => {
                    let zeroes = vec![0u8; attr.byte_size];
                    cur_vbuf.write_all(&zeroes)?;
                }
            }
        }
    }

    Ok(vertex_buffer)
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
            z: 0.0,
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
            w: 0.0,
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
            idx_cur.read_u32::<LittleEndian>().unwrap_or(0)
        } else if indices_comp == 5121 {
            idx_cur.read_u8().unwrap_or(0) as u32
        } else {
            idx_cur.read_u16::<LittleEndian>().unwrap_or(0) as u32
        };
        index_buffer.push(idx);
    }

    let (target_stride, target_attributes, target_descriptors) =
        if let Some(ref orig) = original_parsed {
            (
                orig.stride,
                orig.attributes.clone(),
                orig.raw_descriptors.clone(),
            )
        } else if target_skinned {
            (
                52usize,
                vec![
                    VertexAttribute::from_descriptor(0x02010000),
                    VertexAttribute::from_descriptor(0x02040000),
                    VertexAttribute::from_descriptor(0x01050000),
                    VertexAttribute::from_descriptor(0x030A0000),
                    VertexAttribute::from_descriptor(0x0F0B0000),
                ],
                vec![0x02010000, 0x02040000, 0x01050000, 0x030A0000, 0x0F0B0000],
            )
        } else {
            (
                32usize,
                vec![
                    VertexAttribute::from_descriptor(0x02010000),
                    VertexAttribute::from_descriptor(0x02040000),
                    VertexAttribute::from_descriptor(0x01050000),
                ],
                vec![0x02010000, 0x02040000, 0x01050000],
            )
        };

    let generated_tangents =
        generate_tangents(&raw_positions, &raw_normals, &raw_uvs, &index_buffer);

    let streams = VertexStreams {
        positions: &raw_positions,
        normals: &raw_normals,
        uvs: &raw_uvs,
        weights: &raw_weights,
        joints: &raw_joints,
        tangents: &generated_tangents,
    };

    let vertex_buffer =
        pack_vertex_buffer_preserving_fvf(pos_count, &target_attributes, target_stride, &streams)?;

    rebuild_mesh_container(
        original_chunk,
        vertex_buffer,
        &index_buffer,
        pos_count,
        target_stride as u32,
        &target_descriptors,
    )
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
    for tri in parsed.indices.as_chunks::<3>().0 {
        let i1 = tri[0] as usize + 1;
        let i2 = tri[1] as usize + 1;
        let i3 = tri[2] as usize + 1;
        obj.push_str(&format!(
            "f {0}/{0}/{0} {1}/{1}/{1} {2}/{2}/{2}\n",
            i1, i2, i3
        ));
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

    let mut unique_vertices: HashMap<(usize, usize, usize), u32> = HashMap::new();
    let mut ordered_positions = Vec::new();
    let mut ordered_normals = Vec::new();
    let mut ordered_uvs = Vec::new();
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
                let new_idx = unique_vertices.len() as u32;
                unique_vertices.insert(key, new_idx);
                index_buffer.push(new_idx);

                ordered_positions.push(raw_positions.get(key.0).cloned().unwrap_or_default());
                ordered_normals.push(raw_normals.get(key.2).cloned().unwrap_or(Vector3 {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                }));
                ordered_uvs.push(raw_uvs.get(key.1).cloned().unwrap_or_default());
            }
        }
    }

    let pos_count = unique_vertices.len();
    let raw_weights = vec![
        Vector4 {
            x: 1.0,
            y: 0.0,
            z: 0.0,
            w: 0.0,
        };
        pos_count
    ];
    let raw_joints = vec![[0u8; 4]; pos_count];

    let (target_stride, target_attributes, target_descriptors) =
        if let Some(ref orig) = original_parsed {
            (
                orig.stride,
                orig.attributes.clone(),
                orig.raw_descriptors.clone(),
            )
        } else if target_skinned {
            (
                52usize,
                vec![
                    VertexAttribute::from_descriptor(0x02010000),
                    VertexAttribute::from_descriptor(0x02040000),
                    VertexAttribute::from_descriptor(0x01050000),
                    VertexAttribute::from_descriptor(0x030A0000),
                    VertexAttribute::from_descriptor(0x0F0B0000),
                ],
                vec![0x02010000, 0x02040000, 0x01050000, 0x030A0000, 0x0F0B0000],
            )
        } else {
            (
                32usize,
                vec![
                    VertexAttribute::from_descriptor(0x02010000),
                    VertexAttribute::from_descriptor(0x02040000),
                    VertexAttribute::from_descriptor(0x01050000),
                ],
                vec![0x02010000, 0x02040000, 0x01050000],
            )
        };

    let generated_tangents = generate_tangents(
        &ordered_positions,
        &ordered_normals,
        &ordered_uvs,
        &index_buffer,
    );

    let streams = VertexStreams {
        positions: &ordered_positions,
        normals: &ordered_normals,
        uvs: &ordered_uvs,
        weights: &raw_weights,
        joints: &raw_joints,
        tangents: &generated_tangents,
    };

    let vertex_buffer =
        pack_vertex_buffer_preserving_fvf(pos_count, &target_attributes, target_stride, &streams)?;

    rebuild_mesh_container(
        original_chunk,
        vertex_buffer,
        &index_buffer,
        pos_count,
        target_stride as u32,
        &target_descriptors,
    )
}
