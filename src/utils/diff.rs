use anyhow::{Context, Result};
use base64::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::Path;
use walkdir::WalkDir;

#[derive(Serialize, Deserialize)]
pub struct OverlordModPatch {
    pub patch_name: String,
    pub created_at: u64,
    pub text_diffs: BTreeMap<String, String>,
    pub binary_diffs: BTreeMap<String, String>,
}

pub fn create_diff(base_dir: &Path, mod_dir: &Path, patch_out: &Path) -> Result<usize> {
    let mut text_diffs = BTreeMap::new();
    let mut binary_diffs = BTreeMap::new();
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

        let is_modified = if base_file.exists() {
            let base_bytes = fs::read(&base_file).unwrap_or_default();
            base_bytes != mod_bytes
        } else {
            true // New file added in mod
        };

        if is_modified {
            changes_count += 1;
            if (rel_path.ends_with(".json")
                || rel_path.ends_with(".xml")
                || rel_path.ends_with(".txt")
                || rel_path.ends_with(".lua"))
                && let Ok(text_content) = String::from_utf8(mod_bytes.clone())
            {
                text_diffs.insert(rel_path, text_content);
            } else {
                // High-performance Standard Base64 Encoding
                let b64 = BASE64_STANDARD.encode(&mod_bytes);
                binary_diffs.insert(rel_path, b64);
            }
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
        binary_diffs,
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
