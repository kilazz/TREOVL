use super::parse_chunk_elements;
use crate::engine::common::chunk_id;
use crate::utils::gltf_builder::GltfBuilder;
use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde_json::json;
use std::io::Cursor;

#[derive(Debug, Clone, Copy)]
pub struct TerrainPoint {
    pub height: f32,
    pub main_texture_idx: u8,
    pub foliage_value: u8,
    pub cliff_texture_idx: u8,
}

impl TerrainPoint {
    pub fn from_u32(raw: u32) -> Self {
        let b0 = raw & 0xFF;
        let b1 = (raw >> 8) & 0xFF;

        let high = (b1 & 0x0F) as i32;
        let mid = ((b0 >> 4) & 0x0F) as i32;
        let low = (b0 & 0x0F) as i32;

        let packed_height = (high << 8) | (mid << 4) | low;
        let height = packed_height as f32 / 32.0;

        let main_texture_idx = ((raw >> 16) & 0x0F) as u8;
        let foliage_value = ((raw >> 20) & 0x0F) as u8;
        let cliff_texture_idx = ((raw >> 24) & 0x0F) as u8;

        Self {
            height,
            main_texture_idx,
            foliage_value,
            cliff_texture_idx,
        }
    }
}

pub struct TerrainGeometry {
    pub width: usize,
    pub height: usize,
    pub vertex_count: usize,
    pub triangle_count: usize,
    pub positions: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub min_pos: [f32; 3],
    pub max_pos: [f32; 3],
}

pub fn parse_terrain_geometry(chunk_data: &[u8]) -> Result<TerrainGeometry> {
    let (_, elements) = parse_chunk_elements(chunk_data)?;

    let width_bytes = elements
        .iter()
        .find(|(id, _)| *id == chunk_id::WIDTH)
        .map(|(_, d)| d)
        .context("Missing Width chunk (ID 30)")?;
    let height_bytes = elements
        .iter()
        .find(|(id, _)| *id == chunk_id::HEIGHT)
        .map(|(_, d)| d)
        .context("Missing Height chunk (ID 31)")?;
    let raw_points = elements
        .iter()
        .find(|(id, _)| *id == 33)
        .map(|(_, d)| d)
        .context("Missing TerrainPoints chunk (ID 33)")?;

    let width = Cursor::new(width_bytes).read_u32::<LittleEndian>()? as usize;
    let height = Cursor::new(height_bytes).read_u32::<LittleEndian>()? as usize;

    let mut points = Vec::with_capacity(width * height);
    let mut cur = Cursor::new(raw_points);
    while (cur.position() as usize) + 4 <= raw_points.len() {
        let raw = cur.read_u32::<LittleEndian>()?;
        points.push(TerrainPoint::from_u32(raw));
    }

    if points.len() < width * height {
        bail!("Point count does not match grid dimensions");
    }

    let vertex_count = width * height;
    let spacing = 2.0f32;

    let mut min_pos = [f32::INFINITY, f32::INFINITY, f32::INFINITY];
    let mut max_pos = [f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];

    let mut indices = Vec::with_capacity((width - 1) * (height - 1) * 6);
    for y in 0..(height - 1) {
        for x in 0..(width - 1) {
            let p1 = (y * width + x) as u32;
            let p2 = (y * width + (x + 1)) as u32;
            let p3 = ((y + 1) * width + x) as u32;
            let p4 = ((y + 1) * width + (x + 1)) as u32;

            indices.push(p1);
            indices.push(p3);
            indices.push(p2);

            indices.push(p2);
            indices.push(p3);
            indices.push(p4);
        }
    }

    let mut positions = Vec::with_capacity(vertex_count);
    for y in 0..height {
        for x in 0..width {
            let pt = points[y * width + x];
            let px = x as f32 * spacing;
            let py = pt.height;
            let pz = y as f32 * spacing;

            min_pos[0] = min_pos[0].min(px);
            min_pos[1] = min_pos[1].min(py);
            min_pos[2] = min_pos[2].min(pz);
            max_pos[0] = max_pos[0].max(px);
            max_pos[1] = max_pos[1].max(py);
            max_pos[2] = max_pos[2].max(pz);

            positions.push([px, py, pz]);
        }
    }

    if min_pos[0].is_infinite() {
        min_pos = [0.0, 0.0, 0.0];
        max_pos = [0.0, 0.0, 0.0];
    }

    let mut colors = Vec::with_capacity(vertex_count);
    for pt in &points {
        let r = pt.main_texture_idx as f32 / 15.0;
        let g = pt.foliage_value as f32 / 15.0;
        let b = pt.cliff_texture_idx as f32 / 15.0;
        colors.push([r, g, b]);
    }

    let triangle_count = indices.len() / 3;

    Ok(TerrainGeometry {
        width,
        height,
        vertex_count,
        triangle_count,
        positions,
        colors,
        indices,
        min_pos,
        max_pos,
    })
}

pub fn add_terrain_to_builder(
    builder: &mut GltfBuilder,
    geom: &TerrainGeometry,
    node_name: &str,
) -> Result<usize> {
    let mut idx_bytes = Vec::with_capacity(geom.indices.len() * 4);
    for idx in &geom.indices {
        idx_bytes.write_u32::<LittleEndian>(*idx)?;
    }
    let idx_view = builder.add_buffer_view(&idx_bytes, Some(34963));
    let idx_acc = builder.add_accessor(idx_view, geom.indices.len(), 5125, "SCALAR", None, None);

    let mut pos_bytes = Vec::with_capacity(geom.positions.len() * 12);
    for p in &geom.positions {
        pos_bytes.write_f32::<LittleEndian>(p[0])?;
        pos_bytes.write_f32::<LittleEndian>(p[1])?;
        pos_bytes.write_f32::<LittleEndian>(p[2])?;
    }
    let pos_view = builder.add_buffer_view(&pos_bytes, Some(34962));
    let pos_acc = builder.add_accessor(
        pos_view,
        geom.vertex_count,
        5126,
        "VEC3",
        Some(geom.min_pos.to_vec()),
        Some(geom.max_pos.to_vec()),
    );

    let mut col_bytes = Vec::with_capacity(geom.colors.len() * 12);
    for c in &geom.colors {
        col_bytes.write_f32::<LittleEndian>(c[0])?;
        col_bytes.write_f32::<LittleEndian>(c[1])?;
        col_bytes.write_f32::<LittleEndian>(c[2])?;
    }
    let col_view = builder.add_buffer_view(&col_bytes, Some(34962));
    let col_acc = builder.add_accessor(col_view, geom.vertex_count, 5126, "VEC3", None, None);

    let mesh_idx = builder.add_mesh(json!({
        "name": "TerrainMesh",
        "primitives": [{
            "attributes": {
                "POSITION": pos_acc,
                "COLOR_0": col_acc
            },
            "indices": idx_acc,
            "mode": 4
        }]
    }));

    let node_idx = builder.add_node(json!({
        "name": node_name,
        "mesh": mesh_idx
    }));

    Ok(node_idx)
}

pub fn export_terrain_to_glb(chunk_data: &[u8]) -> Result<(Vec<u8>, usize, usize)> {
    let geom = parse_terrain_geometry(chunk_data)?;
    let mut builder = GltfBuilder::new();
    let node_idx = add_terrain_to_builder(&mut builder, &geom, "TerrainHeightmap")?;
    builder.add_scene(vec![node_idx]);
    let glb = builder.build("Overlord Modding Studio Terrain Exporter")?;
    Ok((glb, geom.vertex_count, geom.triangle_count))
}

pub fn export_terrain_to_obj(chunk_data: &[u8]) -> Result<(String, usize, usize)> {
    let geom = parse_terrain_geometry(chunk_data)?;
    let mut obj = String::new();
    obj.push_str("# Exported Overlord Terrain Heightmap\n");
    obj.push_str("o TerrainMesh\n\n");

    for p in &geom.positions {
        obj.push_str(&format!("v {:.3} {:.3} {:.3}\n", p[0], p[1], p[2]));
    }

    obj.push_str("\ns 1\n");
    // Clippy idiomatic chunking with as_chunks::<3>().0
    for tri in geom.indices.as_chunks::<3>().0 {
        obj.push_str(&format!("f {} {} {}\n", tri[0] + 1, tri[1] + 1, tri[2] + 1));
    }

    Ok((obj, geom.vertex_count, geom.triangle_count))
}
