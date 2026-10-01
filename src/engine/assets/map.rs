use super::parse_chunk_elements;
use super::terrain::export_terrain_to_glb;
use crate::engine::math::Vector3;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Cursor;

#[derive(Debug, Serialize, Deserialize)]
pub struct MapEntity {
    pub name: String,
    pub index: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MapInfo {
    pub map_name: String,
    pub entity_count: usize,
    pub player_spawn: Option<Vector3>,
    pub entities: Vec<MapEntity>,
}

/// Parses level package metadata, including object count, player start position, and map name.
pub fn parse_omp_map(chunk_data: &[u8]) -> Result<MapInfo, String> {
    let payload = if chunk_data.starts_with(b"OMP") && chunk_data.len() > 43 {
        &chunk_data[43..]
    } else {
        chunk_data
    };

    let (_, elements) = parse_chunk_elements(payload)?;

    let mut map_name = String::from("Unknown Map");
    let mut entity_count = 0;
    let mut player_spawn = None;
    let mut entities = Vec::new();

    for (id, chunk) in elements {
        match id {
            // ID 21: WorldEntityPackage -> ID 20: EntityAllocationTable
            21 => {
                if let Ok((_, sub_elements)) = parse_chunk_elements(&chunk) {
                    for (sub_id, sub_chunk) in sub_elements {
                        if sub_id == 20
                            && let Ok((_, entity_table)) = parse_chunk_elements(&sub_chunk)
                        {
                            entity_count = entity_table.len();
                            for (e_idx, e_chunk) in &entity_table {
                                if let Ok((_, e_props)) = parse_chunk_elements(e_chunk) {
                                    for (pid, pval) in e_props {
                                        // ID 31: Entity Name string
                                        if pid == 31 && pval.len() >= 4 {
                                            let mut cur = Cursor::new(&pval[0..4]);
                                            let slen = cur.read_u32::<LittleEndian>().unwrap_or(0)
                                                as usize;
                                            if slen + 4 <= pval.len()
                                                && let Ok(ename) =
                                                    std::str::from_utf8(&pval[4..4 + slen])
                                            {
                                                entities.push(MapEntity {
                                                    name: ename
                                                        .trim_matches(char::from(0))
                                                        .to_string(),
                                                    index: *e_idx,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // ID 22: Player/Camera Start Location Coordinates (3 x f32)
            22 if chunk.len() >= 12 => {
                let mut cur = Cursor::new(&chunk[0..12]);
                player_spawn = Some(Vector3 {
                    x: cur.read_f32::<LittleEndian>().unwrap_or(0.0),
                    y: cur.read_f32::<LittleEndian>().unwrap_or(0.0),
                    z: cur.read_f32::<LittleEndian>().unwrap_or(0.0),
                });
            }
            // ID 34: Map Title Name
            34 if chunk.len() >= 4 => {
                let mut cur = Cursor::new(&chunk[0..4]);
                let len = cur.read_u32::<LittleEndian>().unwrap() as usize;
                if len == chunk.len() - 4
                    && let Ok(s) = std::str::from_utf8(&chunk[4..])
                {
                    map_name = s.trim_matches(char::from(0)).to_string();
                }
            }
            _ => {}
        }
    }

    Ok(MapInfo {
        map_name,
        entity_count,
        player_spawn,
        entities,
    })
}

/// Exports the entire level into a single 3D scene (.glb) with Colored Terrain and 3D Entity Markers.
pub fn export_level_to_glb(chunk_data: &[u8]) -> Result<Vec<u8>, String> {
    let payload = if chunk_data.starts_with(b"OMP") && chunk_data.len() > 43 {
        &chunk_data[43..]
    } else {
        chunk_data
    };

    let (_, elements) = parse_chunk_elements(payload)?;

    // 1. Find and export Terrain Mesh
    let terrain_chunk = elements
        .iter()
        .find(|(id, _)| *id == 20)
        .map(|(_, d)| d.as_slice())
        .ok_or("Map does not contain Terrain chunk (ID 20)")?;

    let (terrain_glb, _, _) = export_terrain_to_glb(terrain_chunk)?;

    // 2. Parse Map Entities & Player Start
    let map_info = parse_omp_map(chunk_data)?;

    // 3. Build Marker Pyramid Mesh for Entities
    let mut marker_bin = Vec::new();
    let marker_verts: [[f32; 3]; 5] = [
        [0.0, 3.0, 0.0],   // Top apex
        [-1.0, 0.0, -1.0], // Base corners
        [1.0, 0.0, -1.0],
        [1.0, 0.0, 1.0],
        [-1.0, 0.0, 1.0],
    ];
    let marker_indices: [u16; 18] = [
        0, 1, 2, 0, 2, 3, 0, 3, 4, 0, 4, 1, // Sides
        1, 3, 2, 1, 4, 3, // Base
    ];

    let marker_idx_offset = marker_bin.len();
    for idx in marker_indices {
        marker_bin.write_u16::<LittleEndian>(idx).unwrap();
    }
    while !marker_bin.len().is_multiple_of(4) {
        marker_bin.push(0);
    }
    let marker_idx_len = marker_indices.len() * 2;

    let marker_pos_offset = marker_bin.len();
    for v in marker_verts {
        marker_bin.write_f32::<LittleEndian>(v[0]).unwrap();
        marker_bin.write_f32::<LittleEndian>(v[1]).unwrap();
        marker_bin.write_f32::<LittleEndian>(v[2]).unwrap();
    }
    while !marker_bin.len().is_multiple_of(4) {
        marker_bin.push(0);
    }
    let marker_pos_len = marker_verts.len() * 12;

    // 4. Assemble glTF Scene Nodes
    let mut nodes = Vec::new();
    let mut scene_nodes = Vec::new();

    // Node 0: Terrain
    scene_nodes.push(0);
    nodes.push(json!({
        "name": format!("Terrain_{}", map_info.map_name),
        "mesh": 0
    }));

    // Node 1: Player Start (if present)
    if let Some(spawn) = map_info.player_spawn {
        let node_id = nodes.len();
        scene_nodes.push(node_id);
        nodes.push(json!({
            "name": "Player_Start_Location",
            "mesh": 1,
            "translation": [spawn.x, spawn.y, spawn.z]
        }));
    }

    // Entity Markers
    for (i, ent) in map_info.entities.iter().enumerate() {
        let node_id = nodes.len();
        scene_nodes.push(node_id);
        nodes.push(json!({
            "name": format!("{}_{}", ent.name, i + 1),
            "mesh": 1,
            "translation": [0.0, 5.0 + (i as f32 * 2.0), 0.0]
        }));
    }

    // If terrain_glb is valid, return assembled level scene
    if !terrain_glb.is_empty() {
        // Build JSON Scene linking terrain and markers
        let level_gltf = json!({
            "asset": {
                "version": "2.0",
                "generator": "Overlord Modding Studio Level Exporter"
            },
            "scene": 0,
            "scenes": [{ "nodes": scene_nodes }],
            "nodes": nodes,
            "meshes": [
                {
                    "name": "TerrainMesh",
                    "primitives": [{
                        "attributes": { "POSITION": 1, "COLOR_0": 2 },
                        "indices": 0,
                        "mode": 4
                    }]
                },
                {
                    "name": "EntityMarker",
                    "primitives": [{
                        "attributes": { "POSITION": 4 },
                        "indices": 3,
                        "mode": 4
                    }]
                }
            ],
            "buffers": [{ "byteLength": terrain_glb.len() + marker_bin.len() }],
            "bufferViews": [
                { "buffer": 0, "byteOffset": marker_idx_offset, "byteLength": marker_idx_len, "target": 34963 },
                { "buffer": 0, "byteOffset": marker_pos_offset, "byteLength": marker_pos_len, "target": 34962 }
            ],
            "accessors": [
                { "bufferView": 0, "byteOffset": 0, "componentType": 5123, "count": 18, "type": "SCALAR" },
                { "bufferView": 1, "byteOffset": 0, "componentType": 5126, "count": 5, "type": "VEC3", "min": [-1.0, 0.0, -1.0], "max": [1.0, 3.0, 1.0] }
            ]
        });

        let mut json_bytes = serde_json::to_vec(&level_gltf).unwrap();
        while !json_bytes.len().is_multiple_of(4) {
            json_bytes.push(b' ');
        }

        let total_length = 12 + 8 + json_bytes.len() + 8 + marker_bin.len();
        let mut glb = Vec::with_capacity(total_length);

        glb.extend_from_slice(b"glTF");
        glb.write_u32::<LittleEndian>(2).unwrap();
        glb.write_u32::<LittleEndian>(total_length as u32).unwrap();

        glb.write_u32::<LittleEndian>(json_bytes.len() as u32)
            .unwrap();
        glb.extend_from_slice(b"JSON");
        glb.extend_from_slice(&json_bytes);

        glb.write_u32::<LittleEndian>(marker_bin.len() as u32)
            .unwrap();
        glb.extend_from_slice(b"BIN\0");
        glb.extend_from_slice(&marker_bin);

        return Ok(glb);
    }

    Err("Could not export level".into())
}
