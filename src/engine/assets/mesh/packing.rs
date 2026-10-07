use anyhow::Result;
use std::io::{Cursor, Write};

use super::fvf::{VertexAttribute, VertexSemantic};
use crate::engine::assets::{
    build_chunk_from_elements, build_typed_container, parse_chunk_elements, parse_typed_container,
};
use crate::engine::common::{Endian, magic};
use crate::engine::math::{Vector2, Vector3, Vector4};

pub struct VertexStreams<'a> {
    pub positions: &'a [Vector3],
    pub normals: &'a [Vector3],
    pub uvs: &'a [Vector2],
    pub weights: &'a [Vector4],
    pub joints: &'a [[u8; 4]],
    pub tangents: &'a [Vector4],
}

pub fn rebuild_mesh_container(
    original_chunk: &[u8],
    vertex_buffer: Vec<u8>,
    index_buffer: &[u32],
    pos_count: usize,
    stride: u32,
    raw_descriptors: &[u32],
    endian: Endian,
) -> Result<Vec<u8>> {
    let max_idx = index_buffer.iter().copied().max().unwrap_or(0);
    let use_32bit = max_idx > 0xFFFF || pos_count > 0xFFFF;

    let (raw_indices_bytes, flag_val) = if use_32bit {
        let mut raw = Vec::with_capacity(index_buffer.len() * 4);
        for &idx in index_buffer {
            endian.write_u32(&mut raw, idx)?;
        }
        (raw, 1u8)
    } else {
        let mut raw = Vec::with_capacity(index_buffer.len() * 2);
        for &idx in index_buffer {
            endian.write_u16(&mut raw, idx as u16)?;
        }
        (raw, 0u8)
    };

    let mut count_bytes = Vec::new();
    endian.write_u32(&mut count_bytes, index_buffer.len() as u32)?;

    let indice_elements = vec![
        (20, vec![flag_val]),
        (21, count_bytes),
        (22, raw_indices_bytes),
    ];
    let new_indice_chunk = build_chunk_from_elements(false, &indice_elements);

    let mut attr_table = Vec::with_capacity(raw_descriptors.len() * 4);
    for &desc in raw_descriptors {
        endian.write_u32(&mut attr_table, desc)?;
    }

    let mut stride_bytes = Vec::new();
    endian.write_u32(&mut stride_bytes, stride)?;

    let mut desc_count_bytes = Vec::new();
    endian.write_u32(&mut desc_count_bytes, raw_descriptors.len() as u32)?;

    let info_elements = vec![
        (20, vec![0u8]),
        (21, stride_bytes),
        (22, desc_count_bytes),
        (23, attr_table),
    ];
    let new_info_chunk = build_chunk_from_elements(false, &info_elements);

    let mut v_count_bytes = Vec::new();
    endian.write_u32(&mut v_count_bytes, pos_count as u32)?;

    let vbuf_elements = vec![
        (20, new_info_chunk),
        (21, v_count_bytes),
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

pub fn pack_vertex_buffer_preserving_fvf(
    pos_count: usize,
    attributes: &[VertexAttribute],
    stride: usize,
    streams: &VertexStreams,
    endian: Endian,
    trailing_bytes: &[u8],
) -> Result<Vec<u8>> {
    let mut vertex_buffer = Vec::with_capacity(pos_count * stride + trailing_bytes.len());
    let mut cur_vbuf = Cursor::new(&mut vertex_buffer);

    for i in 0..pos_count {
        if stride == 53 || stride == 54 {
            let p = streams.positions.get(i).copied().unwrap_or_default();
            endian.write_f32(&mut cur_vbuf, p.x)?;
            endian.write_f32(&mut cur_vbuf, p.y)?;
            endian.write_f32(&mut cur_vbuf, p.z)?;

            let n = streams.normals.get(i).copied().unwrap_or(Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            });
            endian.write_f32(&mut cur_vbuf, n.x)?;
            endian.write_f32(&mut cur_vbuf, n.y)?;
            endian.write_f32(&mut cur_vbuf, n.z)?;

            let uv = streams.uvs.get(i).copied().unwrap_or_default();
            endian.write_f32(&mut cur_vbuf, uv.x)?;
            endian.write_f32(&mut cur_vbuf, uv.y)?;

            let j = streams.joints.get(i).copied().unwrap_or([0, 0, 0, 0]);
            cur_vbuf.write_all(&[j[0], j[1], j[2]])?;

            let w = streams.weights.get(i).copied().unwrap_or(Vector4 {
                x: 1.0,
                y: 0.0,
                z: 0.0,
                w: 0.0,
            });
            let w0 = (w.x * 255.0).clamp(0.0, 255.0) as u8;
            let w1 = (w.y * 255.0).clamp(0.0, 255.0) as u8;
            cur_vbuf.write_all(&[w0, w1])?;

            let t = streams.tangents.get(i).copied().unwrap_or(Vector4 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            });
            endian.write_f32(&mut cur_vbuf, t.x)?;
            endian.write_f32(&mut cur_vbuf, t.y)?;
            endian.write_f32(&mut cur_vbuf, t.z)?;
            endian.write_f32(&mut cur_vbuf, t.w)?;

            if stride == 54 {
                cur_vbuf.write_all(&[0u8])?;
            }
            continue;
        }

        for attr in attributes {
            match attr.semantic {
                VertexSemantic::Position => {
                    let p = streams.positions.get(i).copied().unwrap_or_default();
                    endian.write_f32(&mut cur_vbuf, p.x)?;
                    endian.write_f32(&mut cur_vbuf, p.y)?;
                    endian.write_f32(&mut cur_vbuf, p.z)?;
                }
                VertexSemantic::Normal => {
                    let n = streams.normals.get(i).copied().unwrap_or(Vector3 {
                        x: 0.0,
                        y: 1.0,
                        z: 0.0,
                    });
                    endian.write_f32(&mut cur_vbuf, n.x)?;
                    endian.write_f32(&mut cur_vbuf, n.y)?;
                    endian.write_f32(&mut cur_vbuf, n.z)?;
                }
                VertexSemantic::TexCoord => {
                    let uv = streams.uvs.get(i).copied().unwrap_or_default();
                    endian.write_f32(&mut cur_vbuf, uv.x)?;
                    endian.write_f32(&mut cur_vbuf, uv.y)?;
                }
                VertexSemantic::TangentQuat => {
                    let t = streams.tangents.get(i).copied().unwrap_or(Vector4 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                        w: 1.0,
                    });
                    endian.write_f32(&mut cur_vbuf, t.x)?;
                    endian.write_f32(&mut cur_vbuf, t.y)?;
                    endian.write_f32(&mut cur_vbuf, t.z)?;
                    endian.write_f32(&mut cur_vbuf, t.w)?;
                }
                VertexSemantic::Color => {
                    endian.write_u32(&mut cur_vbuf, 0xFFFFFFFF)?;
                }
                VertexSemantic::BlendWeights => {
                    let w = streams.weights.get(i).copied().unwrap_or(Vector4 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                        w: 0.0,
                    });
                    if attr.byte_size == 16 {
                        endian.write_f32(&mut cur_vbuf, w.x)?;
                        endian.write_f32(&mut cur_vbuf, w.y)?;
                        endian.write_f32(&mut cur_vbuf, w.z)?;
                        endian.write_f32(&mut cur_vbuf, w.w)?;
                    } else if attr.byte_size == 12 {
                        endian.write_f32(&mut cur_vbuf, w.x)?;
                        endian.write_f32(&mut cur_vbuf, w.y)?;
                        endian.write_f32(&mut cur_vbuf, w.z)?;
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

    if !trailing_bytes.is_empty() {
        vertex_buffer.extend_from_slice(trailing_bytes);
    }

    Ok(vertex_buffer)
}
