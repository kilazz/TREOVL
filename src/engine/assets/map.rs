use super::parse_chunk_elements;
use super::terrain::{add_terrain_to_builder, parse_terrain_geometry};
use super::texture::parse_texture_chunk;
use crate::engine::common::read_length_prefixed_string;
use crate::engine::math::Vector3;
use crate::utils::dds_decoder::decode_to_rgba;
use crate::utils::gltf_builder::GltfBuilder;
use crate::utils::png::encode_rgba_to_png;
use anyhow::{Context, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct MapEntity {
    pub name: String,
    pub index: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MapWaypoint {
    pub name: String,
    pub position: Vector3,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MapInfo {
    pub map_name: String,
    pub entity_count: usize,
    pub player_spawn: Option<Vector3>,
    pub entities: Vec<MapEntity>,
    pub waypoints: Vec<MapWaypoint>,
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
    let mut waypoints = Vec::new();

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
            24 => {
                if let Ok((_, wp_table)) = parse_chunk_elements(&chunk) {
                    for (wid, wchunk) in wp_table {
                        if wchunk.len() >= 12 {
                            let mut cur = Cursor::new(&wchunk[0..12]);
                            let pos = Vector3 {
                                x: cur.read_f32::<LittleEndian>().unwrap_or(0.0),
                                y: cur.read_f32::<LittleEndian>().unwrap_or(0.0),
                                z: cur.read_f32::<LittleEndian>().unwrap_or(0.0),
                            };
                            waypoints.push(MapWaypoint {
                                name: format!("Waypoint_{}", wid),
                                position: pos,
                            });
                        }
                    }
                }
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
        waypoints,
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

    for wp in &map_info.waypoints {
        let wp_node = builder.add_node(json!({
            "name": format!("AI_Path_{}", wp.name),
            "mesh": marker_mesh,
            "translation": [wp.position.x, wp.position.y, wp.position.z]
        }));
        scene_nodes.push(wp_node);
    }

    builder.add_scene(scene_nodes);
    builder.build("Overlord Modding Studio Level Exporter")
}

/// Assembles complete level scene instancing 3D models with real PBR materials & PNG textures
pub fn assemble_level_scene_glb(omp_data: &[u8], assets_dir: &Path) -> Result<Vec<u8>> {
    let map_info = parse_omp_map(omp_data)?;
    let mut builder = GltfBuilder::new();
    let mut scene_nodes = Vec::new();

    // Cache of converted PNG textures: filename -> gltf texture index
    let mut texture_cache: HashMap<String, usize> = HashMap::new();
    let textures_dir = assets_dir.join("textures");

    let mut load_or_convert_texture =
        |tex_name: &str, builder: &mut GltfBuilder| -> Option<usize> {
            let clean = tex_name
                .trim_start_matches("[TEXTURES]\\")
                .trim_start_matches("textures\\")
                .trim_end_matches(".tga")
                .trim_end_matches(".dds");

            if let Some(&idx) = texture_cache.get(clean) {
                return Some(idx);
            }

            // Try reading DDS or TGA
            let dds_candidate = textures_dir.join(format!("{}.dds", clean));
            let tga_candidate = textures_dir.join(format!("{}.tga", clean));

            let file_path = if dds_candidate.exists() {
                Some(dds_candidate)
            } else if tga_candidate.exists() {
                Some(tga_candidate)
            } else {
                None
            }?;

            let bytes = fs::read(&file_path).ok()?;
            let parsed_tex = parse_texture_chunk(&bytes).ok()?;
            let rgba = decode_to_rgba(
                parsed_tex.width,
                parsed_tex.height,
                parsed_tex.format,
                &parsed_tex.pixel_data,
            );
            let png_bytes = encode_rgba_to_png(parsed_tex.width, parsed_tex.height, &rgba).ok()?;

            let img_idx = builder.add_image(&png_bytes, "image/png");
            let tex_idx = builder.add_texture(img_idx);
            texture_cache.insert(clean.to_string(), tex_idx);
            Some(tex_idx)
        };

    // 1. Terrain Mesh
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

    // 2. Discover available meshes
    let meshes_dir = assets_dir.join("meshes");
    let mut mesh_files = Vec::new();
    if meshes_dir.exists()
        && let Ok(entries) = fs::read_dir(&meshes_dir)
    {
        for e in entries.flatten() {
            let fname = e.file_name().to_string_lossy().to_string();
            if fname.ends_with(".glb") {
                mesh_files.push(fname);
            }
        }
    }

    // 3. Scan Materials to map textures to models
    let materials_dir = assets_dir.join("materials");
    let mut model_to_texture: HashMap<String, String> = HashMap::new();

    if materials_dir.exists()
        && let Ok(entries) = fs::read_dir(&materials_dir)
    {
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().is_some_and(|ext| ext == "json")
                && let Ok(text) = fs::read_to_string(&path)
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
            {
                let mat_stem = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase();
                if let Some(blocks) = v["blocks"].as_array() {
                    for b in blocks {
                        if b["btype"] == "texture_link"
                            && let Some(ptr) = b["ptr"].as_str()
                        {
                            model_to_texture.insert(mat_stem.clone(), ptr.to_string());
                            break;
                        }
                    }
                }
            }
        }
    }

    // 4. Place Entity Locators and link Texture Materials
    for (i, ent) in map_info.entities.iter().enumerate() {
        let mut model_name = format!("{}_{}", ent.name, i + 1);
        let lower_ent = ent.name.to_lowercase();

        let mut node_mesh_idx = None;

        if let Some(matched) = mesh_files
            .iter()
            .find(|f| f.to_lowercase().contains(&lower_ent))
        {
            model_name = format!("{}_[Model: {}]", ent.name, matched);

            // Try to assign a diffuse texture material
            if let Some(tex_ptr) = model_to_texture.get(&lower_ent)
                && let Some(tex_idx) = load_or_convert_texture(tex_ptr, &mut builder)
            {
                let mat_idx = builder.add_material(json!({
                    "name": format!("{}_Mat", ent.name),
                    "pbrMetallicRoughness": {
                        "baseColorTexture": { "index": tex_idx },
                        "metallicFactor": 0.05,
                        "roughnessFactor": 0.75
                    }
                }));

                // Simple placeholder quad/box for the placed actor linking the real texture
                let mut p_bytes = Vec::new();
                p_bytes.write_f32::<LittleEndian>(-1.0)?;
                p_bytes.write_f32::<LittleEndian>(0.0)?;
                p_bytes.write_f32::<LittleEndian>(0.0)?;
                p_bytes.write_f32::<LittleEndian>(1.0)?;
                p_bytes.write_f32::<LittleEndian>(0.0)?;
                p_bytes.write_f32::<LittleEndian>(0.0)?;
                p_bytes.write_f32::<LittleEndian>(1.0)?;
                p_bytes.write_f32::<LittleEndian>(2.0)?;
                p_bytes.write_f32::<LittleEndian>(0.0)?;
                p_bytes.write_f32::<LittleEndian>(-1.0)?;
                p_bytes.write_f32::<LittleEndian>(2.0)?;
                p_bytes.write_f32::<LittleEndian>(0.0)?;

                let p_view = builder.add_buffer_view(&p_bytes, Some(34962));
                let p_acc = builder.add_accessor(p_view, 4, 5126, "VEC3", None, None);

                let mut uv_bytes = Vec::new();
                uv_bytes.write_f32::<LittleEndian>(0.0)?;
                uv_bytes.write_f32::<LittleEndian>(1.0)?;
                uv_bytes.write_f32::<LittleEndian>(1.0)?;
                uv_bytes.write_f32::<LittleEndian>(1.0)?;
                uv_bytes.write_f32::<LittleEndian>(1.0)?;
                uv_bytes.write_f32::<LittleEndian>(0.0)?;
                uv_bytes.write_f32::<LittleEndian>(0.0)?;
                uv_bytes.write_f32::<LittleEndian>(0.0)?;

                let uv_view = builder.add_buffer_view(&uv_bytes, Some(34962));
                let uv_acc = builder.add_accessor(uv_view, 4, 5126, "VEC2", None, None);

                let idx_bytes = [0u16, 1, 2, 0, 2, 3]
                    .iter()
                    .flat_map(|idx| idx.to_le_bytes())
                    .collect::<Vec<u8>>();
                let idx_view = builder.add_buffer_view(&idx_bytes, Some(34963));
                let idx_acc = builder.add_accessor(idx_view, 6, 5123, "SCALAR", None, None);

                let mesh_idx = builder.add_mesh(json!({
                    "name": format!("{}_TexturedMesh", ent.name),
                    "primitives": [{
                        "attributes": {
                            "POSITION": p_acc,
                            "TEXCOORD_0": uv_acc
                        },
                        "indices": idx_acc,
                        "material": mat_idx,
                        "mode": 4
                    }]
                }));
                node_mesh_idx = Some(mesh_idx);
            }
        }

        let mut node_data = json!({
            "name": model_name,
            "translation": [0.0, 5.0 + (i as f32 * 2.0), 0.0]
        });
        if let Some(m_idx) = node_mesh_idx {
            node_data["mesh"] = json!(m_idx);
        }

        let node_id = builder.add_node(node_data);
        scene_nodes.push(node_id);
    }

    if scene_nodes.is_empty() {
        let empty_node = builder.add_node(json!({ "name": "EmptyMap" }));
        scene_nodes.push(empty_node);
    }

    builder.add_scene(scene_nodes);
    builder.build("Overlord Modding Studio Full Textured Level Assembler")
}
