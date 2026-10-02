use super::node::PrpNode;
use crate::engine::common::{magic, serialize_container_payload};
use crate::utils::zlib::{compress, decompress, is_zlib_compressed};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::Path;

pub fn build_node(node: &PrpNode, project_dir: &Path, compression_level: u32) -> Result<Vec<u8>> {
    if !node.is_container {
        let rel_path = match &node.file_path {
            Some(path) => path,
            None => bail!("Leaf node ID 0x{:X} is missing file_path", node.id),
        };

        let full_path = project_dir.join(rel_path);
        let raw_data = fs::read(&full_path)
            .with_context(|| format!("Failed to read chunk file {:?}", full_path))?;

        if compression_level == 0 {
            return Ok(raw_data);
        }

        if is_zlib_compressed(&raw_data)
            && let Ok(decompressed) = decompress(&raw_data)
            && let Ok(recompressed) = compress(&decompressed, compression_level)
        {
            return Ok(recompressed);
        }

        let is_audio = raw_data.starts_with(b"RIFF") || raw_data.starts_with(magic::AUDIO_WAV);
        if !is_audio
            && raw_data.len() > 128
            && let Ok(compressed) = compress(&raw_data, compression_level)
            && compressed.len() < raw_data.len()
        {
            return Ok(compressed);
        }

        return Ok(raw_data);
    }

    let mut child_buffers = Vec::new();
    for child in &node.children {
        let child_bin = build_node(child, project_dir, compression_level)?;
        child_buffers.push((child, child_bin));
    }

    let mut table = Vec::new();
    if node.has_magic {
        table.extend_from_slice(b"\x01\x01\x00");
    }

    let entries = child_buffers
        .iter()
        .map(|(child, bin)| (child.id, child.is_large, bin.as_slice()));
    table.extend(serialize_container_payload(entries));

    Ok(table)
}
