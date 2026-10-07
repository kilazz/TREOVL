use anyhow::{Result, bail};
use std::collections::HashMap;

use super::fvf::VertexAttribute;
use super::packing::{VertexStreams, pack_vertex_buffer_preserving_fvf, rebuild_mesh_container};
use super::{MeshStats, extract_mesh_geometry};
use crate::engine::common::Endian;
use crate::engine::math::{Vector2, Vector3, Vector4};
use crate::utils::tangents::generate_tangents;

pub fn export_mesh_to_obj(chunk_data: &[u8]) -> Result<(String, MeshStats)> {
    let parsed = extract_mesh_geometry(chunk_data)?;
    let vertex_count = parsed.positions.len();

    let mut obj = String::new();
    obj.push_str("# Exported from Overlord Modding Studio (Blender Ready)\n");
    obj.push_str("o OverlordMesh\n\n");

    for p in &parsed.positions {
        obj.push_str(&format!("v {:.6} {:.6} {:.6}\n", p.x, -p.z, p.y));
    }

    for t in &parsed.uvs {
        obj.push_str(&format!("vt {:.6} {:.6}\n", t.x, 1.0 - t.y));
    }

    for n in &parsed.normals {
        obj.push_str(&format!("vn {:.6} {:.6} {:.6}\n", n.x, -n.z, n.y));
    }

    obj.push_str("\ns 1\n");
    for tri in parsed.indices.as_chunks::<3>().0 {
        let i1 = tri[0] as usize + 1;
        let i2 = tri[2] as usize + 1;
        let i3 = tri[1] as usize + 1;
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
    let endian = original_parsed
        .as_ref()
        .map(|p| p.endian)
        .unwrap_or(Endian::Little);

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
                let bx: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                let by: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                let bz: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                raw_positions.push(Vector3 {
                    x: bx,
                    y: bz,
                    z: -by,
                });
            }
            "vn" => {
                let bx: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                let by: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                let bz: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                raw_normals.push(Vector3 {
                    x: bx,
                    y: bz,
                    z: -by,
                });
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
        for token in [t0, t2, t1] {
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
