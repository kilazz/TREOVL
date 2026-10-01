use super::parse_chunk_elements;
use crate::engine::common::chunk_id;
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

pub fn export_terrain_to_glb(chunk_data: &[u8]) -> Result<(Vec<u8>, usize, usize)> {
    let (_, elements) = parse_chunk_elements(chunk_data)?;

    let width_bytes = elements
        .iter()
        .find(|(id, _)| *id == chunk_id::WIDTH)
        .map(|(_, d)| d)
        .context("Missing Width ID 30")?;
    let height_bytes = elements
        .iter()
        .find(|(id, _)| *id == chunk_id::HEIGHT)
        .map(|(_, d)| d)
        .context("Missing Height ID 31")?;
    let raw_points = elements
        .iter()
        .find(|(id, _)| *id == 33)
        .map(|(_, d)| d)
        .context("Missing TerrainPoints ID 33")?;

    let width = Cursor::new(width_bytes).read_u32::<LittleEndian>()? as usize;
    let height = Cursor::new(height_bytes).read_u32::<LittleEndian>()? as usize;

    let mut points = Vec::with_capacity(width * height);
    let mut cur = Cursor::new(raw_points);
    while (cur.position() as usize) + 4 <= raw_points.len() {
        let raw = cur.read_u32::<LittleEndian>()?;
        points.push(TerrainPoint::from_u32(raw));
    }

    if points.len() < width * height {
        bail!("Point count does not match grid dimensions.");
    }

    let vertex_count = width * height;
    let spacing = 2.0f32;

    let mut min_pos = [f32::INFINITY, f32::INFINITY, f32::INFINITY];
    let mut max_pos = [f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];

    let mut bin_data = Vec::new();

    let indices_offset = bin_data.len();
    let mut index_count = 0;
    for y in 0..(height - 1) {
        for x in 0..(width - 1) {
            let p1 = (y * width + x) as u32;
            let p2 = (y * width + (x + 1)) as u32;
            let p3 = ((y + 1) * width + x) as u32;
            let p4 = ((y + 1) * width + (x + 1)) as u32;

            bin_data.write_u32::<LittleEndian>(p1)?;
            bin_data.write_u32::<LittleEndian>(p3)?;
            bin_data.write_u32::<LittleEndian>(p2)?;

            bin_data.write_u32::<LittleEndian>(p2)?;
            bin_data.write_u32::<LittleEndian>(p3)?;
            bin_data.write_u32::<LittleEndian>(p4)?;

            index_count += 6;
        }
    }
    while !bin_data.len().is_multiple_of(4) {
        bin_data.push(0);
    }
    let indices_length = index_count * 4;

    let pos_offset = bin_data.len();
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

            bin_data.write_f32::<LittleEndian>(px)?;
            bin_data.write_f32::<LittleEndian>(py)?;
            bin_data.write_f32::<LittleEndian>(pz)?;
        }
    }
    let pos_length = vertex_count * 12;

    let col_offset = bin_data.len();
    for pt in &points {
        let r = pt.main_texture_idx as f32 / 15.0;
        let g = pt.foliage_value as f32 / 15.0;
        let b = pt.cliff_texture_idx as f32 / 15.0;

        bin_data.write_f32::<LittleEndian>(r)?;
        bin_data.write_f32::<LittleEndian>(g)?;
        bin_data.write_f32::<LittleEndian>(b)?;
    }
    let col_length = vertex_count * 12;

    while !bin_data.len().is_multiple_of(4) {
        bin_data.push(0);
    }

    let gltf_json = json!({
        "asset": {
            "version": "2.0",
            "generator": "Overlord Modding Studio Terrain Exporter"
        },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "name": "TerrainHeightmap", "mesh": 0 }],
        "meshes": [{
            "name": "TerrainMesh",
            "primitives": [{
                "attributes": {
                    "POSITION": 1,
                    "COLOR_0": 2
                },
                "indices": 0,
                "mode": 4
            }]
        }],
        "buffers": [{ "byteLength": bin_data.len() }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": indices_offset, "byteLength": indices_length, "target": 34963 },
            { "buffer": 0, "byteOffset": pos_offset, "byteLength": pos_length, "target": 34962 },
            { "buffer": 0, "byteOffset": col_offset, "byteLength": col_length, "target": 34962 }
        ],
        "accessors": [
            { "bufferView": 0, "byteOffset": 0, "componentType": 5125, "count": index_count, "type": "SCALAR" },
            { "bufferView": 1, "byteOffset": 0, "componentType": 5126, "count": vertex_count, "type": "VEC3", "min": min_pos, "max": max_pos },
            { "bufferView": 2, "byteOffset": 0, "componentType": 5126, "count": vertex_count, "type": "VEC3" }
        ]
    });

    let mut json_bytes = serde_json::to_vec(&gltf_json)?;
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }

    let total_length = 12 + 8 + json_bytes.len() + 8 + bin_data.len();
    let mut glb = Vec::with_capacity(total_length);

    glb.extend_from_slice(b"glTF");
    glb.write_u32::<LittleEndian>(2)?;
    glb.write_u32::<LittleEndian>(total_length as u32)?;

    glb.write_u32::<LittleEndian>(json_bytes.len() as u32)?;
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&json_bytes);

    glb.write_u32::<LittleEndian>(bin_data.len() as u32)?;
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&bin_data);

    Ok((glb, vertex_count, index_count / 3))
}

pub fn export_terrain_to_obj(chunk_data: &[u8]) -> Result<String> {
    let (_, elements) = parse_chunk_elements(chunk_data)?;

    let width_bytes = elements
        .iter()
        .find(|(id, _)| *id == chunk_id::WIDTH)
        .map(|(_, d)| d)
        .context("Missing Width ID 30")?;
    let height_bytes = elements
        .iter()
        .find(|(id, _)| *id == chunk_id::HEIGHT)
        .map(|(_, d)| d)
        .context("Missing Height ID 31")?;
    let raw_points = elements
        .iter()
        .find(|(id, _)| *id == 33)
        .map(|(_, d)| d)
        .context("Missing TerrainPoints ID 33")?;

    let width = Cursor::new(width_bytes).read_u32::<LittleEndian>()? as usize;
    let height = Cursor::new(height_bytes).read_u32::<LittleEndian>()? as usize;

    let mut points = Vec::with_capacity(width * height);
    let mut cur = Cursor::new(raw_points);
    while (cur.position() as usize) + 4 <= raw_points.len() {
        let raw = cur.read_u32::<LittleEndian>()?;
        points.push(TerrainPoint::from_u32(raw));
    }

    if points.len() < width * height {
        bail!("Point count does not match grid dimensions.");
    }

    let mut obj = String::new();
    obj.push_str("# Exported Overlord Terrain Heightmap\n");
    obj.push_str("o TerrainMesh\n\n");

    let spacing = 2.0;

    for y in 0..height {
        for x in 0..width {
            let pt = points[y * width + x];
            obj.push_str(&format!(
                "v {:.3} {:.3} {:.3}\n",
                x as f32 * spacing,
                pt.height,
                y as f32 * spacing
            ));
        }
    }

    obj.push_str("\ns 1\n");
    for y in 0..(height - 1) {
        for x in 0..(width - 1) {
            let p1 = (y * width + x) + 1;
            let p2 = (y * width + (x + 1)) + 1;
            let p3 = ((y + 1) * width + x) + 1;
            let p4 = ((y + 1) * width + (x + 1)) + 1;

            obj.push_str(&format!("f {} {} {}\n", p1, p3, p2));
            obj.push_str(&format!("f {} {} {}\n", p2, p3, p4));
        }
    }

    Ok(obj)
}
