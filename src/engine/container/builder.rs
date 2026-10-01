use super::node::PrpNode;
use crate::utils::zlib::{compress, decompress, is_zlib_compressed};
use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, WriteBytesExt};
use std::fs;
use std::path::Path;

pub fn build_node(node: &PrpNode, project_dir: &Path) -> Result<Vec<u8>> {
    if !node.is_container {
        let rel_path = match &node.file_path {
            Some(path) => path,
            None => bail!("Leaf node ID 0x{:X} is missing file_path", node.id),
        };

        let full_path = project_dir.join(rel_path);
        let mut raw_data = fs::read(&full_path)
            .with_context(|| format!("Failed to read chunk file {:?}", full_path))?;

        // Smart Level 9 compression if chunk payload was compressed
        if is_zlib_compressed(&raw_data)
            && let Ok(decompressed) = decompress(&raw_data)
            && let Ok(optimized) = compress(&decompressed, 9)
            && optimized.len() < raw_data.len()
        {
            raw_data = optimized;
        }

        return Ok(raw_data);
    }

    let mut child_buffers = Vec::new();
    for child in &node.children {
        let child_bin = build_node(child, project_dir)?;
        child_buffers.push((child, child_bin));
    }

    let mut small_entries = Vec::new();
    let mut large_entries = Vec::new();
    let mut data_segment = Vec::new();
    let mut current_offset = 0usize;

    for (child, bin_data) in child_buffers {
        let id = child.id;
        let c_is_large = child.is_large;

        if !c_is_large && id <= 255 && current_offset <= 255 {
            small_entries.push((id as u8, current_offset as u8));
        } else {
            large_entries.push((id, current_offset as u32));
        }

        data_segment.extend_from_slice(&bin_data);
        current_offset += bin_data.len();
    }

    let mut table = Vec::new();
    if node.has_magic {
        table.extend_from_slice(b"\x01\x01\x00");
    }

    let has_large = !large_entries.is_empty();
    let mut control_byte = (small_entries.len() & 0x7F) as u8;
    if has_large {
        control_byte |= 0x80;
    }
    table.push(control_byte);

    if has_large {
        table.write_u32::<LittleEndian>(large_entries.len() as u32)?;
    }

    for (id, offset) in small_entries {
        table.write_u8(id)?;
        table.write_u8(offset)?;
    }

    for (id, offset) in large_entries {
        table.write_u32::<LittleEndian>(id)?;
        table.write_u32::<LittleEndian>(offset)?;
    }

    table.extend(data_segment);
    Ok(table)
}
