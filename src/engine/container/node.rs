use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::Path;

use crate::engine::common::magic;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PrpNode {
    pub id: u32,
    pub is_large: bool,
    pub is_container: bool,
    pub has_magic: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub children: Vec<PrpNode>,
}

pub fn parse_node(
    data: &[u8],
    node_id: u32,
    is_large: bool,
    is_root: bool,
    chunk_counter: &mut u32,
    output_dir: &Path,
) -> PrpNode {
    if data.is_empty() {
        return save_leaf(data, node_id, is_large, chunk_counter, output_dir);
    }

    let mut pos = 0;
    let mut has_magic = false;

    if !is_root {
        // ACTIVELY USES magic::CONTAINER_MAGIC
        if data.len() >= 3 && &data[0..3] == magic::CONTAINER_MAGIC {
            has_magic = true;
            pos = 3;
        } else if (data[0] & 0x80) == 0 {
            return save_leaf(data, node_id, is_large, chunk_counter, output_dir);
        }
    }

    if pos >= data.len() {
        return save_leaf(data, node_id, is_large, chunk_counter, output_dir);
    }

    let control_byte = data[pos];
    let has_large_entries = (control_byte & 0x80) != 0;
    let small_count = (control_byte & 0x7F) as usize;
    pos += 1;

    let mut large_count = 0;
    if has_large_entries {
        if pos + 4 > data.len() {
            return save_leaf(data, node_id, is_large, chunk_counter, output_dir);
        }
        let mut cur = Cursor::new(&data[pos..pos + 4]);
        large_count = cur.read_u32::<LittleEndian>().unwrap_or(0) as usize;
        pos += 4;
    }

    let total_entries = small_count + large_count;
    if total_entries == 0 || total_entries > 4096 {
        return save_leaf(data, node_id, is_large, chunk_counter, output_dir);
    }

    let table_size = (small_count * 2) + (large_count * 8);
    let data_start = pos + table_size;

    if data_start > data.len() {
        return save_leaf(data, node_id, is_large, chunk_counter, output_dir);
    }

    let first_offset = if small_count > 0 {
        data[pos + 1] as usize
    } else {
        let mut cur = Cursor::new(&data[pos + 4..pos + 8]);
        cur.read_u32::<LittleEndian>().unwrap_or(1) as usize
    };

    if first_offset != 0 {
        return save_leaf(data, node_id, is_large, chunk_counter, output_dir);
    }

    let mut entries = Vec::new();
    let mut cur = Cursor::new(&data[pos..pos + table_size]);

    for _ in 0..small_count {
        let id = cur.read_u8().unwrap_or(0) as u32;
        let offset = cur.read_u8().unwrap_or(0) as usize;
        entries.push((id, offset, false));
    }

    for _ in 0..large_count {
        let id = cur.read_u32::<LittleEndian>().unwrap_or(0);
        let offset = cur.read_u32::<LittleEndian>().unwrap_or(0) as usize;
        entries.push((id, offset, true));
    }

    entries.sort_by_key(|&(_, offset, _)| offset);

    let mut children = Vec::new();
    for i in 0..entries.len() {
        let (id, offset, child_is_large) = entries[i];
        let start = data_start + offset;
        let end = if i + 1 < entries.len() {
            data_start + entries[i + 1].1
        } else {
            data.len()
        };

        if start > data.len() || end > data.len() || end < start {
            return save_leaf(data, node_id, is_large, chunk_counter, output_dir);
        }

        let child_data = &data[start..end];
        let child_node = parse_node(
            child_data,
            id,
            child_is_large,
            false,
            chunk_counter,
            output_dir,
        );
        children.push(child_node);
    }

    PrpNode {
        id: node_id,
        is_large,
        is_container: true,
        has_magic,
        file_path: None,
        children,
    }
}

fn save_leaf(data: &[u8], node_id: u32, is_large: bool, counter: &mut u32, dir: &Path) -> PrpNode {
    *counter += 1;
    let filename = format!("chunk_{:04}_id0x{:X}.bin", counter, node_id);
    let filepath = dir.join("chunks").join(&filename);

    let _ = std::fs::write(&filepath, data);

    PrpNode {
        id: node_id,
        is_large,
        is_container: false,
        has_magic: false,
        file_path: Some(format!("chunks/{}", filename)),
        children: Vec::new(),
    }
}
