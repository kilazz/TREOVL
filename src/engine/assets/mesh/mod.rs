pub mod fvf;
pub mod io_gltf;
pub mod io_obj;
pub mod packing;

use anyhow::{Context, Result, bail};
use std::io::Cursor;

use super::animation::{ObjectBone, parse_object_bone_container};
use super::{parse_chunk_elements, parse_typed_container};
use crate::engine::common::{Endian, chunk_id, magic};
use crate::engine::math::{Vector2, Vector3, Vector4};

pub use fvf::{VertexAttribute, VertexSemantic};
pub use io_gltf::{export_mesh_to_glb, import_glb_to_mesh};
pub use io_obj::{export_mesh_to_obj, import_obj_to_mesh};
pub use packing::{VertexStreams, pack_vertex_buffer_preserving_fvf, rebuild_mesh_container};

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
    pub endian: Endian,
    pub trailing_bytes: Vec<u8>,
}

pub fn extract_mesh_geometry(chunk_data: &[u8]) -> Result<ParsedMeshData> {
    extract_mesh_geometry_with_endian(chunk_data, Endian::Little)
}

pub fn extract_mesh_geometry_with_endian(
    chunk_data: &[u8],
    endian: Endian,
) -> Result<ParsedMeshData> {
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
        .and_then(|(_, d)| endian.read_u32(&mut Cursor::new(d.as_slice())).ok())
        .unwrap_or(0) as usize;

    let is_32bit = if declared_count > 0 {
        raw_indices.len() == declared_count * 4
    } else {
        flag_val != 0 || (raw_indices.len().is_multiple_of(4) && raw_indices.len() > 131_072)
    };

    let indices: Vec<u32> = if is_32bit {
        let mut cur = Cursor::new(raw_indices.as_slice());
        let mut ind = Vec::with_capacity(raw_indices.len() / 4);
        while (cur.position() as usize) + 4 <= raw_indices.len() {
            if let Ok(val) = endian.read_u32(&mut cur) {
                ind.push(val);
            }
        }
        ind
    } else {
        let mut cur = Cursor::new(raw_indices.as_slice());
        let mut ind = Vec::with_capacity(raw_indices.len() / 2);
        while (cur.position() as usize) + 2 <= raw_indices.len() {
            if let Ok(val) = endian.read_u16(&mut cur) {
                ind.push(val as u32);
            }
        }
        ind
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
    let stride = endian.read_u32(&mut Cursor::new(stride_data.as_slice()))? as usize;

    if stride == 0 {
        bail!("Invalid vertex buffer stride: 0");
    }

    let decl_data = info_elements
        .iter()
        .find(|(id, _)| *id == chunk_id::FORMAT)
        .map(|(_, data)| data)
        .context("Missing Attribute Descriptor Table ID 23")?;

    let mut cur_decl = Cursor::new(decl_data.as_slice());
    let mut attributes = Vec::new();
    let mut raw_descriptors = Vec::new();
    while (cur_decl.position() as usize) + 4 <= decl_data.len() {
        let desc = endian.read_u32(&mut cur_decl)?;
        raw_descriptors.push(desc);
        attributes.push(VertexAttribute::from_descriptor(desc));
    }

    let has_weights = attributes
        .iter()
        .any(|a| a.semantic == VertexSemantic::BlendWeights);
    let has_indices = attributes
        .iter()
        .any(|a| a.semantic == VertexSemantic::BlendIndices);
    let is_skinned = (has_weights && has_indices) || stride == 53 || stride == 54;

    let vertex_count = raw_vertices.len() / stride;
    let remainder = raw_vertices.len() % stride;
    let trailing_bytes = if remainder != 0 {
        raw_vertices[vertex_count * stride..].to_vec()
    } else {
        Vec::new()
    };

    let mut positions = Vec::with_capacity(vertex_count);
    let mut normals = Vec::with_capacity(vertex_count);
    let mut uvs = Vec::with_capacity(vertex_count);
    let mut weights = Vec::with_capacity(vertex_count);
    let mut joints = Vec::with_capacity(vertex_count);

    for i in 0..vertex_count {
        let base = i * stride;
        if base + stride > raw_vertices.len() {
            break;
        }
        let vdata = &raw_vertices[base..base + stride];

        let mut pos = Vector3::default();
        let mut norm = Vector3::default();
        let mut uv = Vector2::default();
        let mut w_val = Vector4 {
            x: 1.0,
            y: 0.0,
            z: 0.0,
            w: 0.0,
        };
        let mut j_val = [0u16; 4];

        // Direct Triumph 53/54-byte packed layout parser (Overlord 1 & 2 character/creature meshes)
        if stride == 53 || stride == 54 {
            let mut cur = Cursor::new(&vdata[0..12]);
            pos = Vector3 {
                x: endian.read_f32(&mut cur).unwrap_or(0.0),
                y: endian.read_f32(&mut cur).unwrap_or(0.0),
                z: endian.read_f32(&mut cur).unwrap_or(0.0),
            };

            let mut cur_n = Cursor::new(&vdata[12..24]);
            norm = Vector3 {
                x: endian.read_f32(&mut cur_n).unwrap_or(0.0),
                y: endian.read_f32(&mut cur_n).unwrap_or(1.0),
                z: endian.read_f32(&mut cur_n).unwrap_or(0.0),
            };

            let mut cur_uv = Cursor::new(&vdata[24..32]);
            uv = Vector2 {
                x: endian.read_f32(&mut cur_uv).unwrap_or(0.0),
                y: endian.read_f32(&mut cur_uv).unwrap_or(0.0),
            };

            j_val[0] = vdata[32] as u16;
            j_val[1] = vdata[33] as u16;
            j_val[2] = vdata[34] as u16;
            j_val[3] = 0;

            let w0 = vdata[35] as f32 / 255.0;
            let w1 = vdata[36] as f32 / 255.0;
            let w2 = (1.0 - (w0 + w1)).max(0.0);

            w_val = Vector4 {
                x: w0,
                y: w1,
                z: w2,
                w: 0.0,
            };
        } else {
            // Generic flexible FVF descriptor parser
            let mut weight_slot = 0;
            let mut joint_slot = 0;
            let mut offset = 0;

            for attr in &attributes {
                if offset + attr.byte_size > vdata.len() {
                    break;
                }
                match attr.semantic {
                    VertexSemantic::Position => {
                        let mut cur = Cursor::new(&vdata[offset..offset + 12]);
                        pos = Vector3 {
                            x: endian.read_f32(&mut cur).unwrap_or(0.0),
                            y: endian.read_f32(&mut cur).unwrap_or(0.0),
                            z: endian.read_f32(&mut cur).unwrap_or(0.0),
                        };
                    }
                    VertexSemantic::Normal => {
                        let mut cur = Cursor::new(&vdata[offset..offset + 12]);
                        norm = Vector3 {
                            x: endian.read_f32(&mut cur).unwrap_or(0.0),
                            y: endian.read_f32(&mut cur).unwrap_or(1.0),
                            z: endian.read_f32(&mut cur).unwrap_or(0.0),
                        };
                    }
                    VertexSemantic::TexCoord => {
                        let mut cur = Cursor::new(&vdata[offset..offset + 8]);
                        uv = Vector2 {
                            x: endian.read_f32(&mut cur).unwrap_or(0.0),
                            y: endian.read_f32(&mut cur).unwrap_or(0.0),
                        };
                    }
                    VertexSemantic::BlendWeights => {
                        if attr.byte_size == 16 {
                            let mut cur = Cursor::new(&vdata[offset..offset + 16]);
                            w_val = Vector4 {
                                x: endian.read_f32(&mut cur).unwrap_or(1.0),
                                y: endian.read_f32(&mut cur).unwrap_or(0.0),
                                z: endian.read_f32(&mut cur).unwrap_or(0.0),
                                w: endian.read_f32(&mut cur).unwrap_or(0.0),
                            };
                        } else if attr.byte_size == 12 {
                            let mut cur = Cursor::new(&vdata[offset..offset + 12]);
                            let x = endian.read_f32(&mut cur).unwrap_or(1.0);
                            let y = endian.read_f32(&mut cur).unwrap_or(0.0);
                            let z = endian.read_f32(&mut cur).unwrap_or(0.0);
                            w_val = Vector4 {
                                x,
                                y,
                                z,
                                w: (1.0 - (x + y + z)).max(0.0),
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
                            let val: [u8; 4] =
                                bytemuck::pod_read_unaligned(&vdata[offset..offset + 4]);
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
        }

        let weight_sum = w_val.x + w_val.y + w_val.z + w_val.w;
        if weight_sum > 0.001 {
            let inv = 1.0 / weight_sum;
            w_val.x *= inv;
            w_val.y *= inv;
            w_val.z *= inv;
            w_val.w *= inv;
        } else {
            w_val = Vector4 {
                x: 1.0,
                y: 0.0,
                z: 0.0,
                w: 0.0,
            };
        }

        j_val[0] = j_val[0].min(127);
        j_val[1] = j_val[1].min(127);
        j_val[2] = j_val[2].min(127);
        j_val[3] = j_val[3].min(127);

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
        endian,
        trailing_bytes,
    })
}
