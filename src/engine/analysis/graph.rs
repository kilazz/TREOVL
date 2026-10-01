use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct DependencyGraph {
    pub materials_to_textures: HashMap<String, Vec<String>>,
    pub parameters_to_files: HashMap<String, Vec<String>>,
}

pub fn build_dependency_graph(assets_dir: &Path) -> Result<DependencyGraph> {
    let mut graph = DependencyGraph::default();

    // 1. Scan Materials -> Linked Textures
    let mat_dir = assets_dir.join("materials");
    if mat_dir.exists() {
        for entry in fs::read_dir(mat_dir)?.flatten() {
            if entry.path().extension().is_some_and(|e| e == "json") {
                let stem = entry
                    .path()
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let content = fs::read_to_string(entry.path())?;
                if let Ok(v) = serde_json::from_str::<Value>(&content) {
                    let mut textures = Vec::new();
                    if let Some(blocks) = v["blocks"].as_array() {
                        for b in blocks {
                            // Collapsed if-condition
                            if b["btype"] == "texture_link"
                                && let Some(ptr) = b["ptr"].as_str()
                            {
                                textures.push(ptr.to_string());
                            }
                        }
                    }
                    if !textures.is_empty() {
                        graph.materials_to_textures.insert(stem, textures);
                    }
                }
            }
        }
    }

    // 2. Scan Parameters -> Collision references (.clb) or string tables
    let param_dir = assets_dir.join("parameters");
    if param_dir.exists() {
        for entry in fs::read_dir(param_dir)?.flatten() {
            if entry.path().extension().is_some_and(|e| e == "json") {
                let stem = entry
                    .path()
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let content = fs::read_to_string(entry.path())?;
                // Collapsed if-condition
                if let Ok(v) = serde_json::from_str::<Value>(&content)
                    && let Some(files) = v["files"].as_array()
                {
                    let list: Vec<String> = files
                        .iter()
                        .filter_map(|s| s.as_str().map(|s| s.to_string()))
                        .collect();
                    if !list.is_empty() {
                        graph.parameters_to_files.insert(stem, list);
                    }
                }
            }
        }
    }

    let graph_path = assets_dir.join("dependency_graph.json");
    let json_bytes = serde_json::to_string_pretty(&graph)?;
    fs::write(graph_path, json_bytes)?;

    Ok(graph)
}
