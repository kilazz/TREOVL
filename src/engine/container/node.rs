use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::engine::common::{extract_slices_from_table, magic, parse_raw_container_table};

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

    let slices = extract_slices_from_table(data, &table);
    if slices.len() != table.entries.len() {
        return save_leaf(data, node_id, is_large, chunk_counter, output_dir);
    }

    let mut children = Vec::with_capacity(slices.len());
    for (child_id, child_is_large, child_slice) in slices {
        let child_node = parse_node(
            child_slice,
            child_id,
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
    let _ = std::fs::write(&vanilla_path, data);

    PrpNode {
        id: node_id,
        is_large,
        is_container: false,
        has_magic: false,
        file_path: Some(format!("chunks/{}", filename)),
        children: Vec::new(),
    }
}
