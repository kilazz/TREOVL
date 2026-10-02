use anyhow::{Context, Result, bail};
use base64::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::path::Path;
use walkdir::WalkDir;

use crate::utils::zlib::{compress, decompress};

const DELTA_MAGIC_V2: &[u8; 8] = b"ODELTA02";
const BLOCK_SIZE: usize = 16;

#[derive(Serialize, Deserialize)]
pub struct OverlordModPatch {
    pub patch_name: String,
    pub created_at: u64,
    #[serde(default)]
    pub text_diffs: BTreeMap<String, String>,
    #[serde(default)]
    pub delta_diffs: BTreeMap<String, String>,
    #[serde(default)]
    pub compressed_binary_diffs: BTreeMap<String, String>,
    #[serde(default)]
    pub binary_diffs: BTreeMap<String, String>,
}

#[inline]
fn rolling_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Creates a displacement- and insertion-resilient delta stream using block matching
pub fn create_binary_delta(old: &[u8], new: &[u8]) -> Vec<u8> {
    let mut delta = Vec::new();
    delta.extend_from_slice(DELTA_MAGIC_V2);
    delta.extend_from_slice(&(old.len() as u32).to_le_bytes());
    delta.extend_from_slice(&(new.len() as u32).to_le_bytes());

    if old.is_empty() {
        if !new.is_empty() {
            delta.push(0x01); // INSERT
            delta.extend_from_slice(&(new.len() as u32).to_le_bytes());
            delta.extend_from_slice(new);
        }
        return delta;
    }

    let mut block_map: HashMap<u64, Vec<usize>> = HashMap::new();
    if old.len() >= BLOCK_SIZE {
        for i in 0..=(old.len() - BLOCK_SIZE) {
            let h = rolling_hash(&old[i..i + BLOCK_SIZE]);
            block_map.entry(h).or_default().push(i);
        }
    }

    let mut new_pos = 0;
    let mut pending_insert = Vec::new();

    while new_pos < new.len() {
        let mut matched = false;

        if new_pos + BLOCK_SIZE <= new.len() {
            let h = rolling_hash(&new[new_pos..new_pos + BLOCK_SIZE]);
            if let Some(candidates) = block_map.get(&h) {
                let mut best_offset = 0;
                let mut best_len = 0;

                for &cand in candidates {
                    let mut match_len = 0;
                    while new_pos + match_len < new.len()
                        && cand + match_len < old.len()
                        && new[new_pos + match_len] == old[cand + match_len]
                    {
                        match_len += 1;
                    }
                    if match_len > best_len {
                        best_len = match_len;
                        best_offset = cand;
                    }
                }

                if best_len >= BLOCK_SIZE {
                    if !pending_insert.is_empty() {
                        delta.push(0x01); // INSERT
                        delta.extend_from_slice(&(pending_insert.len() as u32).to_le_bytes());
                        delta.extend_from_slice(&pending_insert);
                        pending_insert.clear();
                    }

                    delta.push(0x02); // COPY
                    delta.extend_from_slice(&(best_offset as u32).to_le_bytes());
                    delta.extend_from_slice(&(best_len as u32).to_le_bytes());

                    new_pos += best_len;
                    matched = true;
                }
            }
        }

        if !matched {
            pending_insert.push(new[new_pos]);
            new_pos += 1;
        }
    }

    if !pending_insert.is_empty() {
        delta.push(0x01); // INSERT
        delta.extend_from_slice(&(pending_insert.len() as u32).to_le_bytes());
        delta.extend_from_slice(&pending_insert);
    }

    delta
}

/// Applies a binary delta against the baseline byte slice with support for V2 and legacy XOR formats
pub fn apply_binary_delta(old: &[u8], delta_data: &[u8]) -> Result<Vec<u8>> {
    if delta_data.starts_with(DELTA_MAGIC_V2) {
        if delta_data.len() < 16 {
            bail!("Truncated delta header");
        }
        let orig_old_len = u32::from_le_bytes(delta_data[8..12].try_into()?) as usize;
        let new_len = u32::from_le_bytes(delta_data[12..16].try_into()?) as usize;

        if orig_old_len != old.len() {
            bail!(
                "Base file size mismatch during delta patching (expected {} bytes, found {})",
                orig_old_len,
                old.len()
            );
        }

        let mut restored = Vec::with_capacity(new_len);
        let mut pos = 16;

        while pos < delta_data.len() {
            let opcode = delta_data[pos];
            pos += 1;
            match opcode {
                0x01 => {
                    // INSERT
                    if pos + 4 > delta_data.len() {
                        bail!("Malformed delta stream");
                    }
                    let len = u32::from_le_bytes(delta_data[pos..pos + 4].try_into()?) as usize;
                    pos += 4;
                    if pos + len > delta_data.len() {
                        bail!("Truncated insert payload");
                    }
                    restored.extend_from_slice(&delta_data[pos..pos + len]);
                    pos += len;
                }
                0x02 => {
                    // COPY
                    if pos + 8 > delta_data.len() {
                        bail!("Malformed delta stream");
                    }
                    let old_offset =
                        u32::from_le_bytes(delta_data[pos..pos + 4].try_into()?) as usize;
                    let len = u32::from_le_bytes(delta_data[pos + 4..pos + 8].try_into()?) as usize;
                    pos += 8;
                    if old_offset + len > old.len() {
                        bail!("Copy bounds exceed baseline file");
                    }
                    restored.extend_from_slice(&old[old_offset..old_offset + len]);
                }
                _ => bail!("Unknown delta opcode: 0x{:02X}", opcode),
            }
        }

        if restored.len() != new_len {
            bail!(
                "Restored size mismatch (expected {}, got {})",
                new_len,
                restored.len()
            );
        }

        return Ok(restored);
    }

    // Fallback: Legacy byte-wise XOR delta
    if delta_data.len() < 8 {
        bail!("Invalid legacy delta header");
    }

    let orig_old_len = u32::from_le_bytes(delta_data[0..4].try_into()?) as usize;
    let new_len = u32::from_le_bytes(delta_data[4..8].try_into()?) as usize;

    if orig_old_len != old.len() {
        bail!("Base file size mismatch during legacy delta patching");
    }

    let payload = &delta_data[8..];
    let common_len = old.len().min(new_len);

    let mut restored = Vec::with_capacity(new_len);
    for i in 0..common_len {
        restored.push(old[i] ^ payload[i]);
    }

    if new_len > old.len() {
        let remaining = new_len - old.len();
        restored.extend_from_slice(&payload[common_len..common_len + remaining]);
    }

    Ok(restored)
}

pub fn create_diff(base_dir: &Path, mod_dir: &Path, patch_out: &Path) -> Result<usize> {
    let mut text_diffs = BTreeMap::new();
    let mut delta_diffs = BTreeMap::new();
    let mut compressed_binary_diffs = BTreeMap::new();
    let mut changes_count = 0;

    for entry in WalkDir::new(mod_dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.path().is_file() {
            continue;
        }

        let rel_path = entry
            .path()
            .strip_prefix(mod_dir)?
            .to_string_lossy()
            .replace('\\', "/");

        let base_file = base_dir.join(&rel_path);
        let mod_bytes = fs::read(entry.path())
            .with_context(|| format!("Failed to read modified file: {:?}", entry.path()))?;

        if base_file.exists() {
            let base_bytes = fs::read(&base_file).unwrap_or_default();
            if base_bytes != mod_bytes {
                changes_count += 1;
                if (rel_path.ends_with(".json")
                    || rel_path.ends_with(".xml")
                    || rel_path.ends_with(".txt")
                    || rel_path.ends_with(".lua"))
                    && let Ok(text_content) = String::from_utf8(mod_bytes.clone())
                {
                    text_diffs.insert(rel_path, text_content);
                } else {
                    let delta_bytes = create_binary_delta(&base_bytes, &mod_bytes);
                    let compressed_delta = compress(&delta_bytes, 9)?;
                    delta_diffs.insert(rel_path, BASE64_STANDARD.encode(&compressed_delta));
                }
            }
        } else {
            changes_count += 1;
            let compressed = compress(&mod_bytes, 6)?;
            compressed_binary_diffs.insert(rel_path, BASE64_STANDARD.encode(&compressed));
        }
    }

    let patch = OverlordModPatch {
        patch_name: patch_out
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
        text_diffs,
        delta_diffs,
        compressed_binary_diffs,
        binary_diffs: BTreeMap::new(),
    };

    let f = File::create(patch_out)?;
    serde_json::to_writer_pretty(f, &patch)?;

    Ok(changes_count)
}

pub fn apply_patch(target_dir: &Path, patch_path: &Path) -> Result<usize> {
    let patch_str = fs::read_to_string(patch_path)
        .with_context(|| format!("Failed to read patch file: {:?}", patch_path))?;
    let patch: OverlordModPatch = serde_json::from_str(&patch_str)?;

    let mut applied_count = 0;

    for (rel, content) in patch.text_diffs {
        let dest = target_dir.join(&rel);
        if let Some(p) = dest.parent() {
            fs::create_dir_all(p)?;
        }
        fs::write(&dest, content.as_bytes())?;
        applied_count += 1;
    }

    for (rel, b64) in patch.delta_diffs {
        let dest = target_dir.join(&rel);
        if !dest.exists() {
            continue;
        }
        let base_bytes = fs::read(&dest)?;
        if let Ok(comp_delta) = BASE64_STANDARD.decode(&b64)
            && let Ok(raw_delta) = decompress(&comp_delta)
            && let Ok(patched_bytes) = apply_binary_delta(&base_bytes, &raw_delta)
        {
            fs::write(&dest, &patched_bytes)?;
            applied_count += 1;
        }
    }

    for (rel, b64) in patch.compressed_binary_diffs {
        let dest = target_dir.join(&rel);
        if let Some(p) = dest.parent() {
            fs::create_dir_all(p)?;
        }
        if let Ok(comp_bytes) = BASE64_STANDARD.decode(&b64)
            && let Ok(raw_bytes) = decompress(&comp_bytes)
        {
            fs::write(&dest, &raw_bytes)?;
            applied_count += 1;
        }
    }

    for (rel, b64) in patch.binary_diffs {
        let dest = target_dir.join(&rel);
        if let Some(p) = dest.parent() {
            fs::create_dir_all(p)?;
        }
        if let Ok(bytes) = BASE64_STANDARD.decode(&b64) {
            fs::write(&dest, &bytes)?;
            applied_count += 1;
        }
    }

    Ok(applied_count)
}
