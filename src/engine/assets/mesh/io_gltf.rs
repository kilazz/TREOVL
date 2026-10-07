use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde_json::json;
use std::io::{Cursor, Read};

use super::fvf::VertexAttribute;
use super::packing::{VertexStreams, pack_vertex_buffer_preserving_fvf, rebuild_mesh_container};
use super::{MeshStats, extract_mesh_geometry};
use crate::engine::common::Endian;
use crate::engine::math::{Vector2, Vector3, Vector4};
use crate::utils::gltf_builder::GltfBuilder;
use crate::utils::tangents::generate_tangents;

pub fn export_mesh_to_glb(chunk_data: &[u8]) -> Result<(Vec<u8>, MeshStats)> {
    let parsed = extract_mesh_geometry(chunk_data)?;
    let vertex_count = parsed.positions.len();

    let mut min_pos = [f32::INFINITY, f32::INFINITY, f32::INFINITY];
    let mut max_pos = [f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];

    for p in &parsed.positions {
        let px = p.x;
        let py = -p.z;
        let pz = p.y;

        min_pos[0] = min_pos[0].min(px);
        min_pos[1] = min_pos[1].min(py);
        min_pos[2] = min_pos[2].min(pz);
        max_pos[0] = max_pos[0].max(px);
        max_pos[1] = max_pos[1].max(py);
        max_pos[2] = max_pos[2].max(pz);
    }

    if min_pos[0].is_infinite() {
        min_pos = [0.0, 0.0, 0.0];
        max_pos = [0.0, 0.0, 0.0];
    }

    let mut builder = GltfBuilder::new();

    let max_idx = parsed.indices.iter().copied().max().unwrap_or(0);
    let (ind_view, comp_type) = if max_idx <= 0xFFFF {
        let mut ind_bytes = Vec::with_capacity(parsed.indices.len() * 2);
        for tri in parsed.indices.as_chunks::<3>().0 {
            ind_bytes.write_u16::<LittleEndian>(tri[0] as u16)?;
            ind_bytes.write_u16::<LittleEndian>(tri[2] as u16)?;
            ind_bytes.write_u16::<LittleEndian>(tri[1] as u16)?;
        }
        (builder.add_buffer_view(&ind_bytes, Some(34963)), 5123)
    } else {
        let mut ind_bytes = Vec::with_capacity(parsed.indices.len() * 4);
        for tri in parsed.indices.as_chunks::<3>().0 {
            ind_bytes.write_u32::<LittleEndian>(tri[0])?;
            ind_bytes.write_u32::<LittleEndian>(tri[2])?;
            ind_bytes.write_u32::<LittleEndian>(tri[1])?;
        }
        (builder.add_buffer_view(&ind_bytes, Some(34963)), 5125)
    };

    let mut pos_bytes = Vec::with_capacity(vertex_count * 12);
    for p in &parsed.positions {
        pos_bytes.write_f32::<LittleEndian>(p.x)?;
        pos_bytes.write_f32::<LittleEndian>(-p.z)?;
        pos_bytes.write_f32::<LittleEndian>(p.y)?;
    }
    let pos_view = builder.add_buffer_view(&pos_bytes, Some(34962));

    let mut norm_bytes = Vec::with_capacity(vertex_count * 12);
    for n in &parsed.normals {
        norm_bytes.write_f32::<LittleEndian>(n.x)?;
        norm_bytes.write_f32::<LittleEndian>(-n.z)?;
        norm_bytes.write_f32::<LittleEndian>(n.y)?;
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

            let tx = bone.translation.x;
            let ty = -bone.translation.z;
            let tz = bone.translation.y;

            let mut bnode = json!({
                "name": bname,
                "translation": [tx, ty, tz],
                "rotation": [bone.rotation.x, -bone.rotation.z, bone.rotation.y, bone.rotation.w]
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

    let glb = builder.build("Overlord Modding Studio glTF Exporter (Blender Ready)")?;

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
    let endian = original_parsed
        .as_ref()
        .map(|p| p.endian)
        .unwrap_or(Endian::Little);

    let num_bones = original_parsed.as_ref().map(|p| p.bones.len()).unwrap_or(0);

    let mut cur = Cursor::new(&glb_bytes[12..]);
    let json_len = cur.read_u32::<LittleEndian>()? as usize;
    let mut json_type = [0u8; 4];
    cur.read_exact(&mut json_type)?;

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
        let bx = pos_cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let by = pos_cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let bz = pos_cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        raw_positions.push(Vector3 {
            x: bx,
            y: bz,
            z: -by,
        });
    }

    let mut raw_normals = vec![
        Vector3 {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        };
        pos_count
    ];
    if let Some(norm_acc_idx) = prim["attributes"]["NORMAL"].as_u64().map(|v| v as usize)
        && let Ok(norm_slice) = read_buffer_view_slice(norm_acc_idx)
    {
        let mut norm_cur = Cursor::new(norm_slice);
        for n in raw_normals.iter_mut().take(pos_count) {
            let bx = norm_cur.read_f32::<LittleEndian>().unwrap_or(0.0);
            let by = norm_cur.read_f32::<LittleEndian>().unwrap_or(1.0);
            let bz = norm_cur.read_f32::<LittleEndian>().unwrap_or(0.0);
            *n = Vector3 {
                x: bx,
                y: bz,
                z: -by,
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
        let j_acc_idx = prim["attributes"]["JOINTS_0"]
            .as_u64()
            .context("Missing JOINTS_0 accessor index in glTF primitive")?
            as usize;
        let w_acc_idx = prim["attributes"]["WEIGHTS_0"]
            .as_u64()
            .context("Missing WEIGHTS_0 accessor index in glTF primitive")?
            as usize;

        let j_comp = accessors[j_acc_idx]["componentType"]
            .as_u64()
            .unwrap_or(5123);
        let j_slice = read_buffer_view_slice(j_acc_idx)?;
        let mut j_cur = Cursor::new(j_slice);

        let max_bone_idx = if num_bones > 0 {
            (num_bones - 1).min(127) as u8
        } else {
            127
        };

        for j in raw_joints.iter_mut().take(pos_count) {
            if j_comp == 5121 {
                *j = [
                    j_cur.read_u8().unwrap_or(0).min(max_bone_idx),
                    j_cur.read_u8().unwrap_or(0).min(max_bone_idx),
                    j_cur.read_u8().unwrap_or(0).min(max_bone_idx),
                    j_cur.read_u8().unwrap_or(0).min(max_bone_idx),
                ];
            } else {
                *j = [
                    (j_cur.read_u16::<LittleEndian>().unwrap_or(0) as u8).min(max_bone_idx),
                    (j_cur.read_u16::<LittleEndian>().unwrap_or(0) as u8).min(max_bone_idx),
                    (j_cur.read_u16::<LittleEndian>().unwrap_or(0) as u8).min(max_bone_idx),
                    (j_cur.read_u16::<LittleEndian>().unwrap_or(0) as u8).min(max_bone_idx),
                ];
            }
        }

        let w_slice = read_buffer_view_slice(w_acc_idx)?;
        let mut w_cur = Cursor::new(w_slice);
        for w in raw_weights.iter_mut().take(pos_count) {
            let wx = w_cur.read_f32::<LittleEndian>().unwrap_or(1.0);
            let wy = w_cur.read_f32::<LittleEndian>().unwrap_or(0.0);
            let wz = w_cur.read_f32::<LittleEndian>().unwrap_or(0.0);
            let ww = w_cur.read_f32::<LittleEndian>().unwrap_or(0.0);

            let sum = wx + wy + wz + ww;
            if sum.is_finite() && sum > 1e-4 {
                let inv = 1.0 / sum;
                *w = Vector4 {
                    x: (wx * inv).clamp(0.0, 1.0),
                    y: (wy * inv).clamp(0.0, 1.0),
                    z: (wz * inv).clamp(0.0, 1.0),
                    w: (ww * inv).clamp(0.0, 1.0),
                };
            } else {
                *w = Vector4 {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                    w: 0.0,
                };
            }
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

    for tri in index_buffer.as_chunks_mut::<3>().0 {
        tri.swap(1, 2);
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
                53usize,
                vec![
                    VertexAttribute::from_descriptor(0x02010000),
                    VertexAttribute::from_descriptor(0x02040000),
                    VertexAttribute::from_descriptor(0x01050000),
                    VertexAttribute::from_descriptor(0x040B0001),
                    VertexAttribute::from_descriptor(0x040B0002),
                    VertexAttribute::from_descriptor(0x070A0100),
                    VertexAttribute::from_descriptor(0x070A0101),
                    VertexAttribute::from_descriptor(0x03090000),
                ],
                vec![
                    0x02010000, 0x02040000, 0x01050000, 0x040B0001, 0x040B0002, 0x070A0100,
                    0x070A0101, 0x03090000,
                ],
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

    let trailing_bytes = original_parsed
        .as_ref()
        .map(|p| p.trailing_bytes.as_slice())
        .unwrap_or(&[]);

    let vertex_buffer = pack_vertex_buffer_preserving_fvf(
        pos_count,
        &target_attributes,
        target_stride,
        &streams,
        endian,
        trailing_bytes,
    )?;

    rebuild_mesh_container(
        original_chunk,
        vertex_buffer,
        &index_buffer,
        pos_count,
        target_stride as u32,
        &target_descriptors,
        endian,
    )
}
