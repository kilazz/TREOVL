use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::Path;

use crate::engine::assets::terrain::{add_terrain_to_builder, parse_terrain_geometry};
use crate::engine::assets::texture::parse_texture_chunk;
use crate::engine::assets::{parse_chunk_elements, parse_typed_container};
use crate::engine::common::{
    Endian, extract_elements_from_table, parse_raw_container_table, read_length_prefixed_string,
};
use crate::engine::container::builder::build_node;
use crate::engine::container::header::PrpHeader;
use crate::engine::container::node::parse_node;
use crate::engine::container::project::{ProgressCallback, ProjectManifest};
use crate::engine::math::Vector3;
use crate::utils::dds_decoder::decode_to_rgba;
use crate::utils::gltf_builder::GltfBuilder;
use crate::utils::png::encode_rgba_to_png;

pub const OMP_MAGIC: &[u8; 4] = b"OMP\0";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OmpHeaderJson {
    pub magic: String,
    pub header_size: u32,
    pub map_width: u32,
    pub map_height: u32,
    pub root_chunk_id: u32,
    pub type_id_hex: String,
    pub raw_header_hex: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OmpDependency {
    pub index: u32,
    pub display_name: String,
    pub rpk_filename: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OmpSpawner {
    pub id: u32,
    pub name: String,
    pub category: String,
    pub position: [f32; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation_raw: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MapWaypoint {
    pub name: String,
    pub position: Vector3,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MapEntity {
    pub name: String,
    pub index: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MapInfo {
    pub map_name: String,
    pub entity_count: usize,
    pub player_spawn: Option<Vector3>,
    pub entities: Vec<MapEntity>,
    pub waypoints: Vec<MapWaypoint>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CutsceneTrackJson {
    pub track_type: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animation: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CutsceneEventJson {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub tracks: Vec<CutsceneTrackJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CutsceneSequenceJson {
    pub id: String,
    pub events: Vec<CutsceneEventJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct QuestConditionJson {
    pub context: String,
    pub lua_condition: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_tag: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OmpLevelManifest {
    pub map_name: String,
    pub player_spawn: Option<[f32; 3]>,
    pub spawner_count: usize,
    pub waypoint_count: usize,
    pub dependency_count: usize,
    pub cutscene_count: usize,
    pub extracted_script_count: usize,
}

pub struct ParsedOmpPackage {
    pub header: OmpHeaderJson,
    pub manifest: OmpLevelManifest,
    pub dependencies: Vec<OmpDependency>,
    pub spawners: Vec<OmpSpawner>,
    pub waypoints: Vec<MapWaypoint>,
    pub cutscenes: Vec<CutsceneSequenceJson>,
    pub quests: Vec<QuestConditionJson>,
    pub raw_payload: Vec<u8>,
}

/// Dynamically locates the true Master Map Container Table offset, supporting both
/// outdoor terrain grids and interior map packages (such as The Tower).
pub fn find_omp_master_table_offset(data: &[u8]) -> usize {
    if data.len() >= 8 {
        let hdr_size = u32::from_le_bytes(data[4..8].try_into().unwrap_or([36, 0, 0, 0])) as usize;
        if (24..=128).contains(&hdr_size)
            && data.len() > hdr_size
            && let Ok(table) = parse_raw_container_table(data, hdr_size, true)
            && (table.entries.iter().any(|e| e.is_large)
                || table.entries.iter().any(|e| e.id == 20 || e.id == 21))
        {
            return hdr_size;
        }
    }

    if data.len() > 56
        && let Ok(table) = parse_raw_container_table(data, 56, true)
        && table.entries.iter().any(|e| e.is_large)
        && table.entries.iter().any(|e| e.id == 20)
    {
        return 56;
    }

    for offset in 36..data.len().min(512) {
        if let Ok(table) = parse_raw_container_table(data, offset, true) {
            let has_large = table.entries.iter().any(|e| e.is_large);
            let has_nodes = table.entries.iter().any(|e| e.id == 20 || e.id == 21);
            if has_large && has_nodes {
                return offset;
            }
        }
    }
    36
}

pub fn parse_omp_package(data: &[u8]) -> Result<ParsedOmpPackage> {
    if data.len() < 36 || !data.starts_with(OMP_MAGIC) {
        bail!("Data is not a valid Overlord Map Package (.omp)");
    }

    let master_offset = find_omp_master_table_offset(data);

    let mut cur = Cursor::new(&data[..master_offset]);
    cur.set_position(4);
    let header_size = cur
        .read_u32::<LittleEndian>()
        .unwrap_or(master_offset as u32);
    let _ = cur.read_u32::<LittleEndian>().unwrap_or(0);
    let root_chunk_id = cur.read_u32::<LittleEndian>().unwrap_or(20);
    let type_id = cur.read_u32::<LittleEndian>().unwrap_or(0x00460003);

    let (map_width, map_height) = if header_size >= 56 && master_offset >= 56 {
        (
            u32::from_le_bytes(data[48..52].try_into().unwrap_or_default()),
            u32::from_le_bytes(data[52..56].try_into().unwrap_or_default()),
        )
    } else {
        (0, 0)
    };

    let header = OmpHeaderJson {
        magic: "OMP".into(),
        header_size,
        map_width,
        map_height,
        root_chunk_id,
        type_id_hex: format!("{:08X}", type_id),
        raw_header_hex: hex::encode_upper(&data[..master_offset]),
    };

    let payload = &data[master_offset..];
    let dependencies = parse_omp_dependencies(payload);
    let (_, elements) = parse_chunk_elements(payload)
        .context("Failed to parse master container table in OMP payload")?;

    let mut map_name = String::new();
    let mut player_spawn = None;
    let mut spawners = Vec::new();
    let mut waypoints = Vec::new();

    if let Some(pos) = payload.windows(4).position(|w| w == b"The ")
        && let Some(title) = read_length_prefixed_string(&payload[pos - 4..])
    {
        map_name = title;
    }

    for (id, chunk) in &elements {
        match *id {
            21 => {
                spawners = parse_omp_spawners(chunk);
            }
            22 if chunk.len() >= 12 => {
                let mut p_cur = Cursor::new(&chunk[0..12]);
                if let (Ok(x), Ok(y), Ok(z)) = (
                    p_cur.read_f32::<LittleEndian>(),
                    p_cur.read_f32::<LittleEndian>(),
                    p_cur.read_f32::<LittleEndian>(),
                ) {
                    player_spawn = Some([x, y, z]);
                }
            }
            24 => {
                if let Ok((_, wp_table)) = parse_chunk_elements(chunk) {
                    for (wid, wchunk) in wp_table {
                        if wchunk.len() >= 12 {
                            let mut w_cur = Cursor::new(&wchunk[0..12]);
                            if let (Ok(x), Ok(y), Ok(z)) = (
                                w_cur.read_f32::<LittleEndian>(),
                                w_cur.read_f32::<LittleEndian>(),
                                w_cur.read_f32::<LittleEndian>(),
                            ) && x.is_finite()
                                && y.is_finite()
                                && z.is_finite()
                            {
                                waypoints.push(MapWaypoint {
                                    name: format!("Waypoint_{}", wid),
                                    position: Vector3 { x, y, z },
                                });
                            }
                        }
                    }
                } else if chunk.len() >= 12 {
                    let mut w_cur = Cursor::new(chunk);
                    let count = chunk.len() / 12;
                    for wid in 0..count {
                        if let (Ok(x), Ok(y), Ok(z)) = (
                            w_cur.read_f32::<LittleEndian>(),
                            w_cur.read_f32::<LittleEndian>(),
                            w_cur.read_f32::<LittleEndian>(),
                        ) && x.is_finite()
                            && y.is_finite()
                            && z.is_finite()
                        {
                            waypoints.push(MapWaypoint {
                                name: format!("Waypoint_{}", wid),
                                position: Vector3 { x, y, z },
                            });
                        }
                    }
                }
            }
            34 if map_name.is_empty() => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    map_name = s;
                }
            }
            _ => {}
        }
    }

    if map_name.is_empty() {
        map_name = String::from("The Tower");
    }

    if player_spawn.is_none()
        && let Some(p1) = spawners.iter().find(|s| s.name.contains("Player"))
    {
        player_spawn = Some(p1.position);
    }

    let cutscenes = extract_all_cutscenes(payload);
    let quests = extract_quest_logic(payload);

    let manifest = OmpLevelManifest {
        map_name,
        player_spawn,
        spawner_count: spawners.len(),
        waypoint_count: waypoints.len(),
        dependency_count: dependencies.len(),
        cutscene_count: cutscenes.len(),
        extracted_script_count: quests.len(),
    };

    Ok(ParsedOmpPackage {
        header,
        manifest,
        dependencies,
        spawners,
        waypoints,
        cutscenes,
        quests,
        raw_payload: payload.to_vec(),
    })
}

pub fn parse_omp_map(chunk_data: &[u8]) -> Result<MapInfo> {
    let pkg = parse_omp_package(chunk_data)?;
    let player_spawn = pkg
        .manifest
        .player_spawn
        .map(|[x, y, z]| Vector3 { x, y, z });
    let entities = pkg
        .spawners
        .iter()
        .map(|s| MapEntity {
            name: s.name.clone(),
            index: s.id,
        })
        .collect();

    Ok(MapInfo {
        map_name: pkg.manifest.map_name,
        entity_count: pkg.manifest.spawner_count,
        player_spawn,
        entities,
        waypoints: pkg.waypoints,
    })
}

pub fn parse_omp_dependencies(data: &[u8]) -> Vec<OmpDependency> {
    let mut deps = Vec::new();
    let marker = b"\x1E\x00\x00\x04";
    let mut pos = 0;

    while let Some(rel) = data[pos..].windows(4).position(|w| w == marker) {
        let start = pos + rel;
        if let Ok((type_id, fields)) = parse_typed_container(&data[start..])
            && type_id == 0x0400001E
        {
            let mut index = 0;
            let mut display_name = String::new();
            let mut rpk_filename = String::new();

            for (fid, fdata) in fields {
                match fid {
                    30 if fdata.len() >= 4 => {
                        index = u32::from_le_bytes(fdata[0..4].try_into().unwrap_or_default());
                    }
                    31 => {
                        if let Some(s) = read_length_prefixed_string(&fdata) {
                            display_name = s;
                        }
                    }
                    32 => {
                        if let Some(s) = read_length_prefixed_string(&fdata) {
                            rpk_filename = s;
                        }
                    }
                    _ => {}
                }
            }

            if !rpk_filename.is_empty() {
                deps.push(OmpDependency {
                    index,
                    display_name,
                    rpk_filename,
                });
            }
        }
        pos = start + 4;
    }

    deps.sort_by_key(|d| d.index);
    deps.dedup_by(|a, b| a.rpk_filename == b.rpk_filename);
    deps
}

fn extract_fields_from_entry(chunk: &[u8]) -> (u32, Vec<(u32, Vec<u8>)>) {
    if let Ok((t, f)) = parse_typed_container(chunk) {
        return (t, f);
    }
    if let Ok((_, f)) = parse_chunk_elements(chunk) {
        return (0, f);
    }
    for offset in 1..chunk.len().min(8) {
        if let Ok((t, f)) = parse_typed_container(&chunk[offset..]) {
            return (t, f);
        }
    }
    (0, Vec::new())
}

fn find_largest_entry_table(slice: &[u8]) -> Vec<(u32, Vec<u8>)> {
    let base_marker = b"BASE\x01\x01\x00";
    if let Some(pos) = slice
        .windows(base_marker.len())
        .position(|w| w == base_marker)
    {
        let container_start = pos + 4;
        if let Ok(table) = parse_raw_container_table(slice, container_start, true)
            && table.entries.len() > 10
        {
            return extract_elements_from_table(slice, &table);
        }
    }

    let magic = b"\x01\x01\x00";
    let mut scan_pos = 0;
    while let Some(rel) = slice[scan_pos..].windows(3).position(|w| w == magic) {
        let abs_pos = scan_pos + rel;
        if let Ok(table) = parse_raw_container_table(slice, abs_pos, true)
            && table.entries.len() > 50
        {
            return extract_elements_from_table(slice, &table);
        }
        scan_pos = abs_pos + 3;
    }

    if let Ok((_, table)) = parse_chunk_elements(slice) {
        if table.len() > 50 {
            return table;
        }
        for (_, sub_data) in &table {
            let inner = find_largest_entry_table(sub_data);
            if inner.len() > 50 {
                return inner;
            }
        }
    }

    if let Ok((_, fields)) = parse_typed_container(slice) {
        if fields.len() > 50 {
            return fields;
        }
        for (_, field_data) in &fields {
            let inner = find_largest_entry_table(field_data);
            if inner.len() > 50 {
                return inner;
            }
        }
    }

    Vec::new()
}

pub fn parse_omp_spawners(chunk_data: &[u8]) -> Vec<OmpSpawner> {
    let mut spawners = Vec::new();
    let entity_table = find_largest_entry_table(chunk_data);

    if entity_table.is_empty() {
        return spawners;
    }

    let mut spawner_groups: HashMap<u32, String> = HashMap::new();

    for (e_idx, e_chunk) in &entity_table {
        let (type_id, fields) = extract_fields_from_entry(e_chunk);

        if type_id == 0x00460013 || type_id == 0x00460015 || type_id == 0x0046001F {
            for (fid, fdata) in &fields {
                if (*fid == 60 || *fid == 62 || *fid == 31 || *fid == 32)
                    && let Some(name) = read_length_prefixed_string(fdata)
                    && !name.is_empty()
                {
                    spawner_groups.insert(*e_idx, name);
                    break;
                }
            }
        }
    }

    for (e_idx, e_chunk) in &entity_table {
        let (type_id, fields) = extract_fields_from_entry(e_chunk);

        let mut name = String::new();
        let mut category = match type_id {
            0x0046500E => String::from("SpawnerNode"),
            0x00465012 => String::from("TowerBlockMechanism"),
            0x0046501B => String::from("TriggerZone"),
            0x00464016 => String::from("ActorNode"),
            0x00464026 => String::from("DwarfActor"),
            0x0046401A => String::from("EnemyActor"),
            0x00460013 => String::from("EnemyGroup"),
            0x00460015 => String::from("ZoneMarker"),
            0x0046001F => String::from("PlayerSpawnGroup"),
            0x0046200A | 0x0046200C | 0x00462010 | 0x0046201A | 0x00462036 => {
                String::from("PropObject")
            }
            0x00464620 | 0x00464626 | 0x00464638 | 0x0046465E => String::from("PlacementObject"),
            0x00463010 | 0x00463015 | 0x00465004 | 0x0046300A => String::from("Mechanism"),
            0x00462104 | 0x00462107 | 0x00462102 | 0x0046210A => String::from("LightMarker"),
            _ => String::from("Entity"),
        };

        let mut position = [0.0f32; 3];
        let mut rotation_raw = None;
        let mut radius = None;
        let mut group_id = None;

        for (fid, fdata) in &fields {
            match *fid {
                20 if fdata.len() >= 12 => {
                    let mut cur = Cursor::new(&fdata[0..12]);
                    let x = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    let y = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    let z = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    if x.is_finite() && y.is_finite() && z.is_finite() {
                        position = [x, y, z];
                    }
                }
                21 if fdata.len() >= 16 => {
                    let mut cur = Cursor::new(&fdata[0..16]);
                    let qx = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    let qy = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    let qz = cur.read_f32::<LittleEndian>().unwrap_or(0.0);
                    let qw = cur.read_f32::<LittleEndian>().unwrap_or(1.0);
                    if qx.is_finite() && qy.is_finite() && qz.is_finite() && qw.is_finite() {
                        let norm_sq = qx * qx + qy * qy + qz * qz + qw * qw;
                        if (norm_sq - 1.0).abs() < 0.15 {
                            rotation_raw = Some([qx, qy, qz, qw]);
                        }
                    }
                }
                12 | 31 | 32 | 62 => {
                    if let Some(s) = read_length_prefixed_string(fdata)
                        && name.is_empty()
                    {
                        name = s;
                    }
                }
                60 => {
                    if let Some(s) = read_length_prefixed_string(fdata)
                        && name.is_empty()
                    {
                        name = s;
                    } else if fdata.len() >= 4
                        && radius.is_none()
                        && let Ok(r) = Cursor::new(fdata).read_f32::<LittleEndian>()
                        && r.is_finite()
                        && !r.is_subnormal()
                        && (0.05..=50_000.0).contains(&r)
                    {
                        radius = Some(r);
                    }
                }
                42 | 61 if fdata.len() >= 4 => {
                    let gid = Cursor::new(fdata).read_u32::<LittleEndian>().ok();
                    if group_id.is_none() && gid.is_some() {
                        group_id = gid;
                        if let Some(gid_val) = gid
                            && let Some(gname) = spawner_groups.get(&gid_val)
                        {
                            category = gname.clone();
                        }
                    }
                }
                _ => {}
            }
        }

        if name.is_empty() {
            if let Some(gid) = group_id
                && let Some(gname) = spawner_groups.get(&gid)
            {
                if category == *gname {
                    name = format!("{}_{}", gname, e_idx);
                } else {
                    name = format!("{}_{}_{}", gname, category, e_idx);
                }
            } else {
                name = format!("{}_{}", category, e_idx);
            }
        }

        if position != [0.0, 0.0, 0.0] || !name.is_empty() {
            spawners.push(OmpSpawner {
                id: *e_idx,
                name,
                category,
                position,
                rotation_raw,
                radius,
                group_id,
            });
        }
    }

    spawners
}

fn extract_lua_source(data: &[u8]) -> Option<String> {
    let mut best_script = String::new();
    let mut pos = 0;
    while pos < data.len().saturating_sub(4) {
        let len =
            u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        if len > 10 && len < 500_000 && pos + 4 + len <= data.len() {
            let slice = &data[pos + 4..pos + 4 + len];
            let clean = slice.strip_suffix(&[0]).unwrap_or(slice);

            if clean
                .iter()
                .all(|&b| b.is_ascii_whitespace() || (0x20..=0x7E).contains(&b))
                && let Ok(s) = std::str::from_utf8(clean)
            {
                let is_code = s.contains("--")
                    || s.contains("function ")
                    || s.contains("SetState")
                    || s.contains("if ")
                    || s.contains("GetMeta")
                    || s.contains("Print(");
                if is_code && s.len() > best_script.len() {
                    best_script = s.trim().to_string();
                }
            }
        }
        pos += 1;
    }

    if !best_script.is_empty() {
        Some(best_script)
    } else {
        None
    }
}

pub fn extract_all_cutscenes(data: &[u8]) -> Vec<CutsceneSequenceJson> {
    let mut cutscenes = Vec::new();
    let cs_marker = b"\x53\x06\x46\x00"; // 0x00460653 (Cutscene Container)
    let mut pos = 0;

    while let Some(rel) = data[pos..].windows(4).position(|w| w == cs_marker) {
        let abs_start = pos + rel;
        let end_pos = (abs_start + 64_000).min(data.len());
        let cs_data = &data[abs_start..end_pos];

        let mut seq_id = String::new();

        for i in 0..cs_data.len().saturating_sub(10) {
            if let Some(s) = read_length_prefixed_string(&cs_data[i..])
                && (s.ends_with("_CS") || s.starts_with("CS_") || s.contains("_PREVIEW"))
            {
                seq_id = s;
                break;
            }
        }

        if seq_id.is_empty() {
            seq_id = format!("Cutscene_{:08X}", abs_start);
        }

        let ev_marker = b"\x54\x06\x46\x00";
        let mut events = Vec::new();
        let mut ev_pos = 0;

        while let Some(ev_rel) = cs_data[ev_pos..].windows(4).position(|w| w == ev_marker) {
            let ev_abs = ev_pos + ev_rel;
            let ev_end = (ev_abs + 8000).min(cs_data.len());
            let ev_chunk = &cs_data[ev_abs..ev_end];

            if let Some(ev) = parse_cutscene_event(ev_chunk) {
                events.push(ev);
            }
            ev_pos = ev_abs + 4;
        }

        cutscenes.push(CutsceneSequenceJson { id: seq_id, events });
        pos = abs_start + 4;
    }

    cutscenes
}

fn parse_cutscene_event(data: &[u8]) -> Option<CutsceneEventJson> {
    let mut name = String::new();
    let description = None;
    let mut tracks = Vec::new();

    for i in 0..data.len().saturating_sub(10) {
        if let Some(s) = read_length_prefixed_string(&data[i..])
            && (s.starts_with("CS_")
                || s.starts_with("Event_")
                || s.contains("WIZARD")
                || s.contains("Gnarl")
                || s.len() < 35)
            && !s.contains("Animation")
            && !s.contains("Script")
            && !s.contains("Camera")
        {
            name = s;
            break;
        }
    }

    let mut tr_pos = 4;
    while tr_pos < data.len().saturating_sub(4) {
        let b = &data[tr_pos..tr_pos + 4];
        if b[1] == 0x06 && b[2] == 0x46 && b[3] == 0x00 && (0x50..=0x75).contains(&b[0]) {
            let tr_end = (tr_pos + 1500).min(data.len());
            let tr_chunk = &data[tr_pos..tr_end];
            if let Some(track) = parse_cutscene_track(tr_chunk) {
                tracks.push(track);
            }
        }
        tr_pos += 1;
    }

    if name.is_empty() && tracks.is_empty() {
        None
    } else {
        if name.is_empty() {
            name = String::from("CutsceneEvent");
        }
        Some(CutsceneEventJson {
            name,
            description,
            tracks,
        })
    }
}

fn parse_cutscene_track(data: &[u8]) -> Option<CutsceneTrackJson> {
    let type_id = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let mut label = String::new();
    let mut script_code = None;
    let mut target = None;
    let mut animation = None;

    let track_type = match type_id {
        0x00460660 => "CameraPosition",
        0x00460661 => "LuaScript",
        0x00460663 => "DialogueLine",
        0x0046065C => "ActorAnimation",
        0x0046065D => "FaceExpression",
        0x0046065E => "CameraShake",
        0x0046065F => "FadeIn",
        0x00460668 => "FadeOut",
        0x0046066A => "EndPoint",
        0x0046066E => "SpawnCharacter",
        0x00460664 => "SoundEffect",
        _ => "TrackAction",
    }
    .to_string();

    let mut i = 4;
    while i < data.len().saturating_sub(4) {
        if let Some(s) = read_length_prefixed_string(&data[i..]) {
            if label.is_empty() && !s.contains("Animation") && !s.contains("Script") {
                label = s;
            } else if target.is_none() {
                target = Some(s);
            } else if animation.is_none() && (s.contains('\\') || s.contains('_')) {
                animation = Some(s);
            }
        }
        i += 1;
    }

    if (track_type == "LuaScript"
        || type_id == 0x00460661
        || data
            .windows(6)
            .any(|w| w == b"SetState" || w == b"PlaySo" || w == b"MoveCh"))
        && let Some(src) = extract_lua_source(data)
    {
        script_code = Some(src);
    }

    Some(CutsceneTrackJson {
        track_type,
        label,
        script_code,
        target,
        animation,
    })
}

pub fn extract_quest_logic(data: &[u8]) -> Vec<QuestConditionJson> {
    let mut quests = Vec::new();
    let q_marker = b"\x08\x11\x46\x00"; // 0x00461108
    let mut pos = 0;

    while let Some(rel) = data[pos..].windows(4).position(|w| w == q_marker) {
        let abs_start = pos + rel;
        let (_, fields) = extract_fields_from_entry(&data[abs_start..]);

        let mut context = String::from("Tower_Narrative");
        let mut lua_condition = String::new();
        let mut event_tag = None;

        for (fid, fdata) in fields {
            match fid {
                20 | 24 => {
                    if let Some(s) = read_length_prefixed_string(&fdata) {
                        context = s;
                    }
                }
                21 => {
                    if let Some(s) = read_length_prefixed_string(&fdata) {
                        lua_condition = s;
                    }
                }
                30 | 31 => {
                    event_tag = read_length_prefixed_string(&fdata);
                }
                _ => {}
            }
        }

        if lua_condition.is_empty() {
            for i in 0..data[abs_start..].len().min(400) {
                if let Some(s) = read_length_prefixed_string(&data[abs_start + i..])
                    && (s.starts_with("if") || s.starts_with("return"))
                {
                    lua_condition = s;
                    break;
                }
            }
        }

        if !lua_condition.is_empty() {
            quests.push(QuestConditionJson {
                context,
                lua_condition,
                event_tag,
            });
        }

        pos = abs_start + 4;
    }

    quests
}

pub fn unpack_omp_to_project_with_progress(
    omp_path: &Path,
    output_dir: &Path,
    progress: Option<ProgressCallback>,
) -> Result<(usize, String)> {
    if let Some(cb) = progress {
        cb(0.10, "Reading Overlord Map Package (.omp)...");
    }

    let data = fs::read(omp_path).with_context(|| format!("Failed to read {:?}", omp_path))?;
    let parsed = parse_omp_package(&data)?;

    let chunks_dir = output_dir.join("chunks");
    let vanilla_dir = output_dir.join("chunks_vanilla");
    let assets_dir = output_dir.join("assets");
    let scripts_dir = assets_dir.join("scripts");

    fs::create_dir_all(&chunks_dir)?;
    fs::create_dir_all(&vanilla_dir)?;
    fs::create_dir_all(&assets_dir)?;
    fs::create_dir_all(&scripts_dir)?;

    if let Some(cb) = progress {
        cb(0.35, "Extracting map chunks hierarchy...");
    }

    let mut chunk_counter = 0;
    let root_node = parse_node(
        &parsed.raw_payload,
        0,
        true,
        true,
        &mut chunk_counter,
        output_dir,
    );

    if let Ok((_, elements)) = parse_chunk_elements(&parsed.raw_payload)
        && let Some((_, terr_data)) = elements.iter().find(|(id, _)| *id == 20)
        && let Ok(geom) = parse_terrain_geometry(terr_data)
    {
        let mut builder = GltfBuilder::new();
        if let Ok(node_idx) = add_terrain_to_builder(
            &mut builder,
            &geom,
            &format!("Terrain_{}", parsed.manifest.map_name),
        ) {
            builder.add_scene(vec![node_idx]);
            if let Ok(glb) = builder.build("Overlord OMP Terrain") {
                let _ = fs::write(assets_dir.join("terrain.glb"), glb);
            }
        }
    }

    if let Some(cb) = progress {
        cb(0.60, "Exporting cutscenes and Lua scripts...");
    }

    fs::write(
        assets_dir.join("cutscenes.json"),
        serde_json::to_string_pretty(&parsed.cutscenes)?,
    )?;

    fs::write(
        assets_dir.join("quests.json"),
        serde_json::to_string_pretty(&parsed.quests)?,
    )?;

    for cs in &parsed.cutscenes {
        let mut script_content = String::new();
        script_content.push_str(&format!("-- Cutscene: {}\n\n", cs.id));

        for ev in &cs.events {
            let mut event_has_code = false;
            let mut track_texts = String::new();

            for tr in &ev.tracks {
                if let Some(ref sc) = tr.script_code {
                    track_texts.push_str(&format!("-- Track: {}\n{}\n\n", tr.label, sc));
                    event_has_code = true;
                }
            }

            if event_has_code {
                script_content.push_str(&format!("-- [Event] {}\n", ev.name));
                if let Some(ref d) = ev.description {
                    script_content.push_str(&format!("-- Description: {}\n", d));
                }
                script_content.push_str(&track_texts);
            }
        }

        if !script_content.trim().is_empty() {
            let safe_name = cs
                .id
                .replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
            let file_path = scripts_dir.join(format!("{}.lua", safe_name));
            let _ = fs::write(file_path, script_content);
        }
    }

    let final_spawners = if !parsed.spawners.is_empty() {
        parsed.spawners
    } else {
        let mut sp = Vec::new();
        if let Ok(entries) = fs::read_dir(&chunks_dir) {
            for e in entries.flatten() {
                let p = e.path();
                if let Ok(meta) = p.metadata()
                    && meta.len() > 10_000
                    && meta.len() < 200_000
                    && let Ok(bytes) = fs::read(&p)
                {
                    let extracted = parse_omp_spawners(&bytes);
                    if !extracted.is_empty() {
                        sp = extracted;
                        break;
                    }
                }
            }
        }
        sp
    };

    let mut final_manifest = parsed.manifest.clone();
    final_manifest.spawner_count = final_spawners.len();

    fs::write(
        assets_dir.join("map_manifest.json"),
        serde_json::to_string_pretty(&final_manifest)?,
    )?;

    fs::write(
        assets_dir.join("dependencies.json"),
        serde_json::to_string_pretty(&parsed.dependencies)?,
    )?;

    fs::write(
        assets_dir.join("spawners.json"),
        serde_json::to_string_pretty(&final_spawners)?,
    )?;

    let prp_header = PrpHeader {
        magic: "OMP".into(),
        major_version: 3,
        minor_version: 0,
        file_id: parsed.header.root_chunk_id,
        data_size: parsed.raw_payload.len() as u32,
        pack_name: parsed.manifest.map_name.clone(),
        endian: Endian::Little,
    };

    let project_manifest = ProjectManifest {
        header: prp_header,
        has_footer: false,
        footer_hash2: 0,
        endian: Endian::Little,
        root: root_node,
        omp_header: Some(parsed.header.clone()),
    };

    fs::write(
        output_dir.join("project.json"),
        serde_json::to_string_pretty(&project_manifest)?,
    )?;

    let log = format!(
        "OMP Package: '{}' | Header Size: {} B\n\
         • Placed Entities & Spawners: {}\n\
         • Cinematic Cutscenes: {}\n\
         • Quest & Trigger Logic: {}\n\
         • Dependencies: {} archives\n\
         • Standalone Lua scripts saved to 'assets/scripts/*.lua'\n",
        final_manifest.map_name,
        parsed.header.header_size,
        final_manifest.spawner_count,
        final_manifest.cutscene_count,
        final_manifest.extracted_script_count,
        final_manifest.dependency_count
    );

    if let Some(cb) = progress {
        cb(1.0, "Level unpack complete.");
    }

    Ok((chunk_counter as usize, log))
}

pub fn pack_omp_from_project_with_progress(
    project_dir: &Path,
    output_archive: &Path,
    compression_level: u32,
    progress: Option<ProgressCallback>,
) -> Result<usize> {
    let manifest_path = project_dir.join("project.json");
    let manifest_str = fs::read_to_string(&manifest_path)?;
    let manifest: ProjectManifest = serde_json::from_str(&manifest_str)?;

    let omp_hdr = manifest
        .omp_header
        .as_ref()
        .context("Missing omp_header metadata in project.json")?;

    if let Some(cb) = progress {
        cb(0.30, "Rebuilding map container chunks...");
    }

    let payload = build_node(
        &manifest.root,
        project_dir,
        compression_level,
        manifest.endian,
    )?;

    let header_bytes = if let Ok(h_raw) = hex::decode(&omp_hdr.raw_header_hex) {
        if h_raw.len() == omp_hdr.header_size as usize {
            h_raw
        } else {
            rebuild_omp_header_bytes(omp_hdr)?
        }
    } else {
        rebuild_omp_header_bytes(omp_hdr)?
    };

    if let Some(cb) = progress {
        cb(0.80, "Writing binary map package (.omp)...");
    }

    let mut final_file = Vec::with_capacity(header_bytes.len() + payload.len());
    final_file.extend_from_slice(&header_bytes);
    final_file.extend_from_slice(&payload);

    fs::write(output_archive, &final_file)?;

    if let Some(cb) = progress {
        cb(1.0, "Map package built successfully.");
    }

    Ok(final_file.len())
}

fn rebuild_omp_header_bytes(hdr: &OmpHeaderJson) -> Result<Vec<u8>> {
    let size = hdr.header_size as usize;
    let mut out = vec![0u8; size];
    out[0..4].copy_from_slice(OMP_MAGIC);
    if size >= 8 {
        out[4..8].copy_from_slice(&hdr.header_size.to_le_bytes());
    }
    if size >= 16 {
        out[12..16].copy_from_slice(&hdr.root_chunk_id.to_le_bytes());
    }
    if size >= 20 {
        let type_id =
            u32::from_str_radix(hdr.type_id_hex.trim_start_matches("0x"), 16).unwrap_or(0x00460003);
        out[16..20].copy_from_slice(&type_id.to_le_bytes());
    }
    if size >= 52 {
        out[48..52].copy_from_slice(&hdr.map_width.to_le_bytes());
    }
    if size >= 56 {
        out[52..56].copy_from_slice(&hdr.map_height.to_le_bytes());
    }
    Ok(out)
}

pub fn export_level_to_glb(chunk_data: &[u8]) -> Result<Vec<u8>> {
    let parsed = parse_omp_package(chunk_data)?;
    let mut builder = GltfBuilder::new();
    let mut scene_nodes = Vec::new();

    if let Ok((_, elements)) = parse_chunk_elements(&parsed.raw_payload)
        && let Some((_, terr_data)) = elements.iter().find(|(id, _)| *id == 20)
        && let Ok(geom) = parse_terrain_geometry(terr_data)
    {
        let t_node = add_terrain_to_builder(
            &mut builder,
            &geom,
            &format!("Terrain_{}", parsed.manifest.map_name),
        )?;
        scene_nodes.push(t_node);
    }

    if let Some(spawn) = parsed.manifest.player_spawn {
        let spawn_node = builder.add_node(json!({
            "name": "Player_Start_Location",
            "translation": spawn
        }));
        scene_nodes.push(spawn_node);
    }

    for spawner in parsed.spawners {
        let s_node = builder.add_node(json!({
            "name": format!("{}_[{}]", spawner.name, spawner.category),
            "translation": spawner.position
        }));
        scene_nodes.push(s_node);
    }

    if scene_nodes.is_empty() {
        let empty = builder.add_node(json!({ "name": "EmptyMap" }));
        scene_nodes.push(empty);
    }

    builder.add_scene(scene_nodes);
    builder.build("Overlord Level Exporter")
}

pub fn assemble_level_scene_glb(omp_data: &[u8], assets_dir: &Path) -> Result<Vec<u8>> {
    let map_info = parse_omp_map(omp_data)?;
    let mut builder = GltfBuilder::new();
    let mut scene_nodes = Vec::new();

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

    let offset = if omp_data.starts_with(b"OMP") {
        find_omp_master_table_offset(omp_data)
    } else {
        0
    };
    let payload = &omp_data[offset..];
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
        && let Ok(entries) = fs::read_dir(&meshes_dir)
    {
        for e in entries.flatten() {
            let fname = e.file_name().to_string_lossy().to_string();
            if fname.ends_with(".glb") {
                mesh_files.push(fname);
            }
        }
    }

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

    for (i, ent) in map_info.entities.iter().enumerate() {
        let mut model_name = format!("{}_{}", ent.name, i + 1);
        let lower_ent = ent.name.to_lowercase();
        let mut node_mesh_idx = None;

        if let Some(matched) = mesh_files
            .iter()
            .find(|f| f.to_lowercase().contains(&lower_ent))
        {
            model_name = format!("{}_[Model: {}]", ent.name, matched);

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
