use super::{build_chunk_from_elements, parse_chunk_elements};
use crate::engine::common::{Endian, chunk_id};
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

    /// Packs point values into Triumph's 32-bit bitfield format
    pub fn to_u32(&self) -> u32 {
        let packed_height = ((self.height * 32.0).round() as i32).clamp(0, 4095) as u32;
        let low = packed_height & 0x0F;
        let mid = (packed_height >> 4) & 0x0F;
        let high = (packed_height >> 8) & 0x0F;

        let b0 = (mid << 4) | low;
        let b1 = high & 0x0F;

        let main_tex = (self.main_texture_idx as u32 & 0x0F) << 16;
        let foliage = (self.foliage_value as u32 & 0x0F) << 20;
        let cliff_tex = (self.cliff_texture_idx as u32 & 0x0F) << 24;

        b0 | (b1 << 8) | main_tex | foliage | cliff_tex
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
    parse_terrain_geometry_with_endian(chunk_data, Endian::Little)
}

pub fn parse_terrain_geometry_with_endian(
    chunk_data: &[u8],
    endian: Endian,
) -> Result<TerrainGeometry> {
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

    let width = endian.read_u32(&mut Cursor::new(width_bytes.as_slice()))? as usize;
    let height = endian.read_u32(&mut Cursor::new(height_bytes.as_slice()))? as usize;

    let mut points = Vec::with_capacity(width * height);
    let mut cur = Cursor::new(raw_points.as_slice());
    while (cur.position() as usize) + 4 <= raw_points.len() {
        let raw = endian.read_u32(&mut cur)?;
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
    for tri in geom.indices.as_chunks::<3>().0 {
        obj.push_str(&format!("f {} {} {}\n", tri[0] + 1, tri[1] + 1, tri[2] + 1));
    }

    Ok((obj, geom.vertex_count, geom.triangle_count))
}

/// Injects a new 2D array of height values into an existing terrain chunk.
pub fn import_terrain_heightmap(
    original_chunk: &[u8],
    height_grid: &[f32],
    new_width: usize,
    new_height: usize,
    endian: Endian,
) -> Result<Vec<u8>> {
    if height_grid.len() < new_width * new_height {
        bail!("Provided height array is smaller than the specified dimensions (width x height)");
    }

    let (has_magic, mut elements) = parse_chunk_elements(original_chunk)?;

    let existing_points: Vec<TerrainPoint> =
        if let Some((_, raw_points)) = elements.iter().find(|(id, _)| *id == 33) {
            let mut pts = Vec::new();
            let mut cur = Cursor::new(raw_points.as_slice());
            while (cur.position() as usize) + 4 <= raw_points.len() {
                if let Ok(raw) = endian.read_u32(&mut cur) {
                    pts.push(TerrainPoint::from_u32(raw));
                }
            }
            pts
        } else {
            Vec::new()
        };

    let total_points = new_width * new_height;
    let mut packed_points_data = Vec::with_capacity(total_points * 4);

    for (i, &h) in height_grid.iter().take(total_points).enumerate() {
        let (main_tex, foliage, cliff_tex) = if let Some(orig) = existing_points.get(i) {
            (
                orig.main_texture_idx,
                orig.foliage_value,
                orig.cliff_texture_idx,
            )
        } else {
            (0, 0, 1)
        };

        let pt = TerrainPoint {
            height: h,
            main_texture_idx: main_tex,
            foliage_value: foliage,
            cliff_texture_idx: cliff_tex,
        };

        let raw = pt.to_u32();
        endian.write_u32(&mut packed_points_data, raw)?;
    }

    let mut w_bytes = Vec::new();
    endian.write_u32(&mut w_bytes, new_width as u32)?;

    let mut h_bytes = Vec::new();
    endian.write_u32(&mut h_bytes, new_height as u32)?;

    elements.retain(|(id, _)| *id != chunk_id::WIDTH && *id != chunk_id::HEIGHT && *id != 33);
    elements.push((chunk_id::WIDTH, w_bytes));
    elements.push((chunk_id::HEIGHT, h_bytes));
    elements.push((33, packed_points_data));
    elements.sort_by_key(|&(id, _)| id);

    Ok(build_chunk_from_elements(has_magic, &elements))
}

/// Imports height values from a glTF 2.0 Binary (.glb) file back into a terrain chunk
pub fn import_terrain_from_glb(
    original_chunk: &[u8],
    glb_bytes: &[u8],
    endian: Endian,
) -> Result<Vec<u8>> {
    let geom = parse_terrain_geometry_with_endian(original_chunk, endian)?;
    let width = geom.width;
    let height = geom.height;

    if glb_bytes.len() < 20 || &glb_bytes[0..4] != b"glTF" {
        bail!("Invalid .glb binary file (missing 'glTF' header)");
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

    let prim = gltf["meshes"]
        .get(0)
        .and_then(|m| m["primitives"].get(0))
        .context("No primitives found in terrain glTF mesh")?;

    let accessors = gltf["accessors"]
        .as_array()
        .context("Missing accessors array")?;
    let buffer_views = gltf["bufferViews"]
        .as_array()
        .context("Missing bufferViews array")?;

    let pos_acc_idx = prim["attributes"]["POSITION"]
        .as_u64()
        .context("Primitive missing POSITION")? as usize;
    let acc = &accessors[pos_acc_idx];
    let count = acc["count"].as_u64().unwrap_or(0) as usize;
    let bv_idx = acc["bufferView"].as_u64().unwrap_or(0) as usize;
    let bv = &buffer_views[bv_idx];

    let total_offset = bv["byteOffset"].as_u64().unwrap_or(0) as usize
        + acc["byteOffset"].as_u64().unwrap_or(0) as usize;

    if total_offset + count * 12 > bin_data.len() {
        bail!("Accessor points beyond the binary payload");
    }

    let mut pos_cur = Cursor::new(&bin_data[total_offset..]);
    let mut heights = vec![0.0f32; width * height];

    for h in heights.iter_mut().take(count) {
        let _px = pos_cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let py = pos_cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        let _pz = pos_cur.read_f32::<LittleEndian>().unwrap_or(0.0);
        *h = py;
    }

    import_terrain_heightmap(original_chunk, &heights, width, height, endian)
}
