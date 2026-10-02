use super::parse_chunk_elements;
use super::terrain::{add_terrain_to_builder, parse_terrain_geometry};
use crate::engine::common::read_length_prefixed_string;
use crate::engine::math::Vector3;
use crate::utils::gltf_builder::GltfBuilder;
use anyhow::{Context, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Cursor;
use std::path::Path;

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

pub fn parse_omp_map(chunk_data: &[u8]) -> Result<MapInfo> {
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
                                        if pid == 31
                                            && let Some(ename) = read_length_prefixed_string(&pval)
                                        {
                                            entities.push(MapEntity {
                                                name: ename,
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
            22 if chunk.len() >= 12 => {
                let mut cur = Cursor::new(&chunk[0..12]);
                player_spawn = Some(Vector3 {
                    x: cur.read_f32::<LittleEndian>().unwrap_or(0.0),
                    y: cur.read_f32::<LittleEndian>().unwrap_or(0.0),
                    z: cur.read_f32::<LittleEndian>().unwrap_or(0.0),
                });
            }
            34 => {
                if let Some(s) = read_length_prefixed_string(&chunk) {
                    map_name = s;
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

pub fn export_level_to_glb(chunk_data: &[u8]) -> Result<Vec<u8>> {
    let payload = if chunk_data.starts_with(b"OMP") && chunk_data.len() > 43 {
        &chunk_data[43..]
    } else {
        chunk_data
    };

    let (_, elements) = parse_chunk_elements(payload)?;

    let terrain_chunk = elements
        .iter()
        .find(|(id, _)| *id == 20)
        .map(|(_, d)| d.as_slice())
        .context("Map does not contain Terrain chunk (ID 20)")?;

    let map_info = parse_omp_map(chunk_data)?;
    let terrain_geom = parse_terrain_geometry(terrain_chunk)?;

    let mut builder = GltfBuilder::new();
    let mut scene_nodes = Vec::new();

    let terrain_node = add_terrain_to_builder(
        &mut builder,
        &terrain_geom,
        &format!("Terrain_{}", map_info.map_name),
    )?;
    scene_nodes.push(terrain_node);

    let marker_verts: [[f32; 3]; 5] = [
        [0.0, 3.0, 0.0],
        [-1.0, 0.0, -1.0],
        [1.0, 0.0, -1.0],
        [1.0, 0.0, 1.0],
        [-1.0, 0.0, 1.0],
    ];
    let marker_indices: [u16; 18] = [0, 1, 2, 0, 2, 3, 0, 3, 4, 0, 4, 1, 1, 3, 2, 1, 4, 3];

    let mut marker_idx_bytes = Vec::with_capacity(marker_indices.len() * 2);
    for idx in marker_indices {
        marker_idx_bytes.write_u16::<LittleEndian>(idx)?;
    }
    let m_idx_view = builder.add_buffer_view(&marker_idx_bytes, Some(34963));
    let m_idx_acc =
        builder.add_accessor(m_idx_view, marker_indices.len(), 5123, "SCALAR", None, None);

    let mut marker_pos_bytes = Vec::with_capacity(marker_verts.len() * 12);
    for v in marker_verts {
        marker_pos_bytes.write_f32::<LittleEndian>(v[0])?;
        marker_pos_bytes.write_f32::<LittleEndian>(v[1])?;
        marker_pos_bytes.write_f32::<LittleEndian>(v[2])?;
    }
    let m_pos_view = builder.add_buffer_view(&marker_pos_bytes, Some(34962));
    let m_pos_acc = builder.add_accessor(
        m_pos_view,
        marker_verts.len(),
        5126,
        "VEC3",
        Some(vec![-1.0, 0.0, -1.0]),
        Some(vec![1.0, 3.0, 1.0]),
    );

    let marker_mesh = builder.add_mesh(json!({
        "name": "EntityMarker",
        "primitives": [{
            "attributes": { "POSITION": m_pos_acc },
            "indices": m_idx_acc,
            "mode": 4
        }]
    }));

    if let Some(spawn) = map_info.player_spawn {
        let spawn_node = builder.add_node(json!({
            "name": "Player_Start_Location",
            "mesh": marker_mesh,
            "translation": [spawn.x, spawn.y, spawn.z]
        }));
        scene_nodes.push(spawn_node);
    }

    for (i, ent) in map_info.entities.iter().enumerate() {
        let ent_node = builder.add_node(json!({
            "name": format!("{}_{}", ent.name, i + 1),
            "mesh": marker_mesh,
            "translation": [0.0, 5.0 + (i as f32 * 2.0), 0.0]
        }));
        scene_nodes.push(ent_node);
    }

    builder.add_scene(scene_nodes);
    builder.build("Overlord Modding Studio Level Exporter")
}

pub fn assemble_level_scene_glb(omp_data: &[u8], assets_dir: &Path) -> Result<Vec<u8>> {
    let map_info = parse_omp_map(omp_data)?;
    let mut builder = GltfBuilder::new();
    let mut scene_nodes = Vec::new();

    let payload = if omp_data.starts_with(b"OMP") && omp_data.len() > 43 {
        &omp_data[43..]
    } else {
        omp_data
    };
    let (_, elements) = parse_chunk_elements(payload)?;

    if let Some((_, terr_data)) = elements.iter().find(|(id, _)| *id == 20)
        && let Ok(terrain_geom) = parse_terrain_geometry(terr_data)
    {
        let terr_node = add_terrain_to_builder(
            &mut builder,
            &terrain_geom,
            &format!("Terrain_{}", map_info.map_name),
        )?;
        scene_nodes.push(terr_node);
    }

    let meshes_dir = assets_dir.join("meshes");
    let mut mesh_files = Vec::new();
    if meshes_dir.exists()
        && let Ok(entries) = std::fs::read_dir(&meshes_dir)
    {
        for e in entries.flatten() {
            let fname = e.file_name().to_string_lossy().to_string();
            if fname.ends_with(".glb") {
                mesh_files.push(fname);
            }
        }
    }

    for (i, ent) in map_info.entities.iter().enumerate() {
        let mut model_name = format!("{}_{}", ent.name, i + 1);
        let lower_ent = ent.name.to_lowercase();

        if let Some(matched) = mesh_files
            .iter()
            .find(|f| f.to_lowercase().contains(&lower_ent))
        {
            model_name = format!("{}_[Model: {}]", ent.name, matched);
        }

        let node_id = builder.add_node(json!({
            "name": model_name,
            "translation": [0.0, 5.0 + (i as f32 * 2.0), 0.0]
        }));
        scene_nodes.push(node_id);
    }

    if scene_nodes.is_empty() {
        let empty_node = builder.add_node(json!({ "name": "EmptyMap" }));
        scene_nodes.push(empty_node);
    }

    builder.add_scene(scene_nodes);
    builder.build("Overlord Modding Studio Full Level Assembler")
}
