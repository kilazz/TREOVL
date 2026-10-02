use crc32fast::Hasher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AssetSyncEntry {
    pub chunk_rel_path: String,
    pub asset_kind: String,
    pub vanilla_crc32: u32,
    #[serde(default)]
    pub is_modified: bool,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct AssetSyncCache {
    pub entries: HashMap<String, AssetSyncEntry>,
}

pub fn calculate_crc32(data: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

pub fn sanitize_filename(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '[' | ']' => '_',
            _ => c,
        })
        .collect();
    clean.trim_matches('_').to_string()
}

pub fn build_asset_filename(display_name: &str, stem: &str, ext: &str) -> String {
    let clean_title = sanitize_filename(display_name);
    let clean_title = clean_title
        .split('(')
        .next()
        .unwrap_or(&clean_title)
        .trim()
        .trim_matches('_');
    if clean_title.is_empty() || clean_title == stem {
        format!("{}.{}", stem, ext)
    } else {
        format!("{}_{}.{}", clean_title, stem, ext)
    }
}
