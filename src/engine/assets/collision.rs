use anyhow::{Context, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Cursor;

use crate::utils::gltf_builder::GltfBuilder;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CollisionJson {
    pub _engine_metadata: CollisionEngineMetadataJson,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub shapes: Vec<CollisionShapeJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CollisionEngineMetadataJson {
    pub file_type: String,
    pub size: usize,
    pub hex: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CollisionShapeJson {
    pub shape_type: String, // "box_oriented"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub half_extents: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation_matrix: Option<[[f32; 3]; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    pub hex: String,
}

#[derive(Debug, Clone)]
pub struct OrientedBoundingBox {
    pub rotation: [[f32; 3]; 3],
    pub half_extents: [f32; 3],
    pub center: [f32; 3],
}

pub fn parse_collision_boxes(data: &[u8]) -> Vec<OrientedBoundingBox> {
    let mut boxes = Vec::new();
    let mut cur = Cursor::new(data);

    while (cur.position() as usize) + 60 <= data.len() {
        let pos = cur.position() as usize;
        let mut valid = true;
        let mut buf = [0f32; 15];
        let mut temp_cur = Cursor::new(&data[pos..pos + 60]);

        for v in &mut buf {
            if let Ok(f) = temp_cur.read_f32::<LittleEndian>() {
                if !f.is_finite() || f.abs() > 100_000.0 {
                    valid = false;
                    break;
                }
                *v = f;
            } else {
                valid = false;
                break;
            }
        }

        if valid {
            boxes.push(OrientedBoundingBox {
                rotation: [
                    [buf[0], buf[1], buf[2]],
                    [buf[3], buf[4], buf[5]],
                    [buf[6], buf[7], buf[8]],
                ],
                half_extents: [buf[9], buf[10], buf[11]],
                center: [buf[12], buf[13], buf[14]],
            });
            cur.set_position((pos + 60) as u64);
        } else {
            break;
        }
    }

    boxes
}

/// Exports collision boundaries (.clb) to a glTF 2.0 Binary (.glb) file with semi-transparent boxes
pub fn export_collision_to_glb(data: &[u8]) -> Result<Vec<u8>> {
    let boxes = parse_collision_boxes(data);
    let mut builder = GltfBuilder::new();

    // Semi-transparent collision visualizer material
    let col_mat = builder.add_material(json!({
        "name": "Collision_Visualizer_Mat",
        "pbrMetallicRoughness": {
            "baseColorFactor": [0.15, 0.85, 0.45, 0.55],
            "metallicFactor": 0.0,
            "roughnessFactor": 0.5
        },
        "alphaMode": "BLEND",
        "doubleSided": true
    }));

    // Face definitions for a unit cube with distinct normals per face
    let face_normals = [
        [0.0, 0.0, 1.0],  // Front
        [0.0, 0.0, -1.0], // Back
        [1.0, 0.0, 0.0],  // Right
        [-1.0, 0.0, 0.0], // Left
        [0.0, 1.0, 0.0],  // Top
        [0.0, -1.0, 0.0], // Bottom
    ];

    let face_quads = [
        [
            [-1.0, -1.0, 1.0],
            [1.0, -1.0, 1.0],
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0],
        ],
        [
            [1.0, -1.0, -1.0],
            [-1.0, -1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [1.0, 1.0, -1.0],
        ],
        [
            [1.0, -1.0, 1.0],
            [1.0, -1.0, -1.0],
            [1.0, 1.0, -1.0],
            [1.0, 1.0, 1.0],
        ],
        [
            [-1.0, -1.0, -1.0],
            [-1.0, -1.0, 1.0],
            [-1.0, 1.0, 1.0],
            [-1.0, 1.0, -1.0],
        ],
        [
            [-1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, -1.0],
            [-1.0, 1.0, -1.0],
        ],
        [
            [-1.0, -1.0, -1.0],
            [1.0, -1.0, -1.0],
            [1.0, -1.0, 1.0],
            [-1.0, -1.0, 1.0],
        ],
    ];

    let mut scene_nodes = Vec::new();

    for (b_idx, obb) in boxes.iter().enumerate() {
        let mut positions = Vec::with_capacity(24 * 12);
        let mut normals = Vec::with_capacity(24 * 12);
        let mut indices = Vec::with_capacity(36 * 2);

        let mut min_pos = [f32::INFINITY; 3];
        let mut max_pos = [f32::NEG_INFINITY; 3];

        let r = obb.rotation;
        let c = obb.center;
        let h = obb.half_extents;

        for (f_i, quad) in face_quads.iter().enumerate() {
            let fnorm = face_normals[f_i];
            let rot_norm = [
                r[0][0] * fnorm[0] + r[0][1] * fnorm[1] + r[0][2] * fnorm[2],
                r[1][0] * fnorm[0] + r[1][1] * fnorm[1] + r[1][2] * fnorm[2],
                r[2][0] * fnorm[0] + r[2][1] * fnorm[1] + r[2][2] * fnorm[2],
            ];

            let base_idx = (f_i * 4) as u16;

            for v in quad {
                let local_x = v[0] * h[0];
                let local_y = v[1] * h[1];
                let local_z = v[2] * h[2];

                let wx = r[0][0] * local_x + r[0][1] * local_y + r[0][2] * local_z + c[0];
                let wy = r[1][0] * local_x + r[1][1] * local_y + r[1][2] * local_z + c[1];
                let wz = r[2][0] * local_x + r[2][1] * local_y + r[2][2] * local_z + c[2];

                min_pos[0] = min_pos[0].min(wx);
                min_pos[1] = min_pos[1].min(wy);
                min_pos[2] = min_pos[2].min(wz);
                max_pos[0] = max_pos[0].max(wx);
                max_pos[1] = max_pos[1].max(wy);
                max_pos[2] = max_pos[2].max(wz);

                positions.write_f32::<LittleEndian>(wx)?;
                positions.write_f32::<LittleEndian>(wy)?;
                positions.write_f32::<LittleEndian>(wz)?;

                normals.write_f32::<LittleEndian>(rot_norm[0])?;
                normals.write_f32::<LittleEndian>(rot_norm[1])?;
                normals.write_f32::<LittleEndian>(rot_norm[2])?;
            }

            indices.write_u16::<LittleEndian>(base_idx)?;
            indices.write_u16::<LittleEndian>(base_idx + 1)?;
            indices.write_u16::<LittleEndian>(base_idx + 2)?;

            indices.write_u16::<LittleEndian>(base_idx)?;
            indices.write_u16::<LittleEndian>(base_idx + 2)?;
            indices.write_u16::<LittleEndian>(base_idx + 3)?;
        }

        let idx_view = builder.add_buffer_view(&indices, Some(34963));
        let idx_acc = builder.add_accessor(idx_view, 36, 5123, "SCALAR", None, None);

        let pos_view = builder.add_buffer_view(&positions, Some(34962));
        let pos_acc = builder.add_accessor(
            pos_view,
            24,
            5126,
            "VEC3",
            Some(min_pos.to_vec()),
            Some(max_pos.to_vec()),
        );

        let norm_view = builder.add_buffer_view(&normals, Some(34962));
        let norm_acc = builder.add_accessor(norm_view, 24, 5126, "VEC3", None, None);

        let mesh_idx = builder.add_mesh(json!({
            "name": format!("ColBoxMesh_{:03}", b_idx),
            "primitives": [{
                "attributes": {
                    "POSITION": pos_acc,
                    "NORMAL": norm_acc
                },
                "indices": idx_acc,
                "material": col_mat,
                "mode": 4
            }]
        }));

        let node_idx = builder.add_node(json!({
            "name": format!("CollisionBox_{:03}", b_idx),
            "mesh": mesh_idx
        }));

        scene_nodes.push(node_idx);
    }

    if scene_nodes.is_empty() {
        let empty = builder.add_node(json!({ "name": "EmptyCollision" }));
        scene_nodes.push(empty);
    }

    builder.add_scene(scene_nodes);
    builder.build("TREOVL 3D Collision Generator")
}

/// Imports glTF 2.0 Binary (.glb) geometry back into .clb binary data
pub fn import_collision_from_glb(glb_bytes: &[u8]) -> Result<Vec<u8>> {
    if glb_bytes.len() < 20 || &glb_bytes[0..4] != b"glTF" {
        anyhow::bail!("Not a valid .glb binary file");
    }

    let mut cur = Cursor::new(&glb_bytes[12..]);
    let json_len = cur.read_u32::<LittleEndian>()? as usize;
    let mut json_type = [0u8; 4];
    std::io::Read::read_exact(&mut cur, &mut json_type)?;

    let json_slice = &glb_bytes[20..20 + json_len];
    let gltf: serde_json::Value = serde_json::from_slice(json_slice)?;

    let bin_header_pos = 20 + json_len;
    let bin_pos = bin_header_pos + 8;
    let bin_data = &glb_bytes[bin_pos..];

    let accessors = gltf["accessors"]
        .as_array()
        .context("Missing accessors array")?;
    let buffer_views = gltf["bufferViews"]
        .as_array()
        .context("Missing bufferViews array")?;
    let nodes = gltf["nodes"].as_array().context("Missing nodes array")?;

    let mut out_bytes = Vec::new();

    for node in nodes {
        if let Some(mesh_idx) = node["mesh"].as_u64()
            && let Some(mesh) = gltf["meshes"].get(mesh_idx as usize)
            && let Some(prim) = mesh["primitives"].get(0)
            && let Some(pos_acc_idx) = prim["attributes"]["POSITION"].as_u64()
        {
            let acc = &accessors[pos_acc_idx as usize];
            let count = acc["count"].as_u64().unwrap_or(0) as usize;
            let bv_idx = acc["bufferView"].as_u64().unwrap_or(0) as usize;
            let bv = &buffer_views[bv_idx];

            let bv_offset = bv["byteOffset"].as_u64().unwrap_or(0) as usize;
            let acc_offset = acc["byteOffset"].as_u64().unwrap_or(0) as usize;
            let total_offset = bv_offset + acc_offset;

            if total_offset + count * 12 <= bin_data.len() {
                let mut pos_cur = Cursor::new(&bin_data[total_offset..]);
                let mut min_p = [f32::INFINITY; 3];
                let mut max_p = [f32::NEG_INFINITY; 3];

                for _ in 0..count {
                    let x = pos_cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    let y = pos_cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    let z = pos_cur.read_f32::<LittleEndian>().unwrap_or(0.0);

                    min_p[0] = min_p[0].min(x);
                    min_p[1] = min_p[1].min(y);
                    min_p[2] = min_p[2].min(z);
                    max_p[0] = max_p[0].max(x);
                    max_p[1] = max_p[1].max(y);
                    max_p[2] = max_p[2].max(z);
                }

                let center = [
                    (min_p[0] + max_p[0]) * 0.5,
                    (min_p[1] + max_p[1]) * 0.5,
                    (min_p[2] + max_p[2]) * 0.5,
                ];
                let half_extents = [
                    (max_p[0] - min_p[0]) * 0.5,
                    (max_p[1] - min_p[1]) * 0.5,
                    (max_p[2] - min_p[2]) * 0.5,
                ];

                // Write 3x3 Identity matrix (9 floats)
                out_bytes.write_f32::<LittleEndian>(1.0)?;
                out_bytes.write_f32::<LittleEndian>(0.0)?;
                out_bytes.write_f32::<LittleEndian>(0.0)?;
                out_bytes.write_f32::<LittleEndian>(0.0)?;
                out_bytes.write_f32::<LittleEndian>(1.0)?;
                out_bytes.write_f32::<LittleEndian>(0.0)?;
                out_bytes.write_f32::<LittleEndian>(0.0)?;
                out_bytes.write_f32::<LittleEndian>(0.0)?;
                out_bytes.write_f32::<LittleEndian>(1.0)?;

                // Write half extents (3 floats)
                out_bytes.write_f32::<LittleEndian>(half_extents[0])?;
                out_bytes.write_f32::<LittleEndian>(half_extents[1])?;
                out_bytes.write_f32::<LittleEndian>(half_extents[2])?;

                // Write center (3 floats)
                out_bytes.write_f32::<LittleEndian>(center[0])?;
                out_bytes.write_f32::<LittleEndian>(center[1])?;
                out_bytes.write_f32::<LittleEndian>(center[2])?;
            }
        }
    }

    Ok(out_bytes)
}

pub fn export_collision_to_json(data: &[u8], _stem: &str) -> Result<String> {
    let boxes = parse_collision_boxes(data);
    let mut shapes = Vec::new();

    for b in boxes {
        shapes.push(CollisionShapeJson {
            shape_type: "box_oriented".into(),
            center: Some(b.center),
            half_extents: Some(b.half_extents),
            rotation_matrix: Some(b.rotation),
            radius: None,
            hex: String::new(),
        });
    }

    let metadata = CollisionEngineMetadataJson {
        file_type: "CollisionBoundary (.clb)".into(),
        size: data.len(),
        hex: hex::encode_upper(data),
    };

    let col_json = CollisionJson {
        _engine_metadata: metadata,
        shapes,
    };

    serde_json::to_string_pretty(&col_json).context("Failed to format Collision JSON")
}

pub fn import_collision_from_json(json_str: &str, baseline: &[u8]) -> Result<Vec<u8>> {
    let parsed: CollisionJson = serde_json::from_str(json_str)?;

    if !parsed.shapes.is_empty() {
        let mut out = Vec::with_capacity(parsed.shapes.len() * 60);
        for shape in parsed.shapes {
            if let (Some(center), Some(half), Some(rot)) =
                (shape.center, shape.half_extents, shape.rotation_matrix)
            {
                for row in rot {
                    for val in row {
                        out.write_f32::<LittleEndian>(val)?;
                    }
                }
                for val in half {
                    out.write_f32::<LittleEndian>(val)?;
                }
                for val in center {
                    out.write_f32::<LittleEndian>(val)?;
                }
            }
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }

    if !parsed._engine_metadata.hex.is_empty()
        && let Ok(bytes) = hex::decode(&parsed._engine_metadata.hex)
    {
        return Ok(bytes);
    }

    Ok(baseline.to_vec())
}
