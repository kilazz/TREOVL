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

/// Сравнивает оригинальную папку проекта с модифицированной и создает компактный patch.json
pub fn create_diff(base_dir: &Path, mod_dir: &Path, patch_out: &Path) -> Result<usize, String> {
    let mut text_diffs = BTreeMap::new();
    let mut binary_diffs = BTreeMap::new();
    let mut changes_count = 0;

    for entry in WalkDir::new(mod_dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.path().is_file() {
            continue;
        }

        let rel_path = entry
            .path()
            .strip_prefix(mod_dir)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");

        let base_file = base_dir.join(&rel_path);
        let mod_bytes = fs::read(entry.path()).map_err(|e| e.to_string())?;

        let is_modified = if base_file.exists() {
            let base_bytes = fs::read(&base_file).unwrap_or_default();
            base_bytes != mod_bytes
        } else {
            true // Новый добавленный файл
        };

        if is_modified {
            changes_count += 1;
            // Текстовые файлы (project.json, xml, txt) сохраняем в читаемом виде
            if (rel_path.ends_with(".json")
                || rel_path.ends_with(".xml")
                || rel_path.ends_with(".txt"))
                && let Ok(text_content) = String::from_utf8(mod_bytes.clone())
            {
                text_diffs.insert(rel_path, text_content);
            } else {
                // Бинарники (чанки, dds, wav) сохраняем в Base64
                let b64 = simple_base64_encode(&mod_bytes);
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
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        text_diffs,
        binary_diffs,
    };

    let f = File::create(patch_out).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(f, &patch).map_err(|e| e.to_string())?;

    Ok(changes_count)
}

/// Применяет patch.json к целевой папке проекта
pub fn apply_patch(target_dir: &Path, patch_path: &Path) -> Result<usize, String> {
    let patch_str = fs::read_to_string(patch_path).map_err(|e| e.to_string())?;
    let patch: OverlordModPatch = serde_json::from_str(&patch_str).map_err(|e| e.to_string())?;

    let mut applied_count = 0;

    for (rel, content) in patch.text_diffs {
        let dest = target_dir.join(&rel);
        if let Some(p) = dest.parent() {
            fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        fs::write(&dest, content.as_bytes()).map_err(|e| e.to_string())?;
        applied_count += 1;
    }

    for (rel, b64) in patch.binary_diffs {
        let dest = target_dir.join(&rel);
        if let Some(p) = dest.parent() {
            fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        if let Some(bytes) = simple_base64_decode(&b64) {
            fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
            applied_count += 1;
        }
    }

    Ok(applied_count)
}

fn simple_base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);

        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0x0F) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn simple_base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0;

    for &b in s.as_bytes() {
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        };
        buf = (buf << 6) | (val as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}
