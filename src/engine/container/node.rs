use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::engine::common::{magic, parse_raw_container_table};

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

    if !is_root && !data.starts_with(magic::CONTAINER_MAGIC) && (data[0] & 0x80) == 0 {
        return save_leaf(data, node_id, is_large, chunk_counter, output_dir);
    }

    let table = match parse_raw_container_table(data, 0, true) {
        Ok(t) => t,
        Err(_) => return save_leaf(data, node_id, is_large, chunk_counter, output_dir),
    };

    let mut children = Vec::with_capacity(table.entries.len());
    for i in 0..table.entries.len() {
        let entry = table.entries[i];
        let start = table.data_start + entry.offset;
        let end = if i + 1 < table.entries.len() {
            table.data_start + table.entries[i + 1].offset
        } else {
            data.len()
        };

        if start > data.len() || end > data.len() || end < start {
            return save_leaf(data, node_id, is_large, chunk_counter, output_dir);
        }

        let child_node = parse_node(
            &data[start..end],
            entry.id,
            entry.is_large,
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
        has_magic: table.has_magic,
        file_path: None,
        children,
    }
}

fn save_leaf(data: &[u8], node_id: u32, is_large: bool, counter: &mut u32, dir: &Path) -> PrpNode {
    *counter += 1;
    let filename = format!("chunk_{:04}_id0x{:X}.bin", counter, node_id);
    let working_path = dir.join("chunks").join(&filename);
    let vanilla_path = dir.join("chunks_vanilla").join(&filename);

    let _ = std::fs::write(&working_path, data);
    let _ = std::fs::write(&vanilla_path, data); // Pristine baseline copy

    PrpNode {
        id: node_id,
        is_large,
        is_container: false,
        has_magic: false,
        file_path: Some(format!("chunks/{}", filename)),
        children: Vec::new(),
    }
}
