use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct DependencyGraph {
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub materials_to_textures: HashMap<String, Vec<String>>,

    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub objects_to_meshes: HashMap<String, Vec<String>>,

    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub objects_to_materials: HashMap<String, Vec<String>>,

    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub characters_to_animations: HashMap<String, Vec<String>>,

    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub characters_to_sounds: HashMap<String, Vec<String>>,

    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub characters_to_facefx: HashMap<String, Vec<String>>,

    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub attachments_to_meshes: HashMap<String, Vec<String>>,

    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub attachments_to_sounds: HashMap<String, Vec<String>>,

    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub terrain_palettes_to_textures: HashMap<String, Vec<String>>,

    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub terrain_palettes_to_meshes: HashMap<String, Vec<String>>,

    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub events_to_sounds: HashMap<String, Vec<String>>,

    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub fonts_to_textures: HashMap<String, Vec<String>>,

    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub voice_packages_to_files: HashMap<String, Vec<String>>,

    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub collisions_to_files: HashMap<String, Vec<String>>,
}

fn clean_and_dedup(list: &mut Vec<String>) {
    list.retain(|s| !s.trim().is_empty());
    list.sort();
    list.dedup();
}

pub fn build_dependency_graph(assets_dir: &Path) -> Result<DependencyGraph> {
    let mut graph = DependencyGraph::default();

    // -------------------------------------------------------------------------
    // 1. Scan Materials -> Linked Textures
    // -------------------------------------------------------------------------
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
                            if b["btype"] == "texture_link"
                                && let Some(ptr) = b["ptr"].as_str()
                            {
                                textures.push(ptr.to_string());
                            }
                        }
                    }
                    clean_and_dedup(&mut textures);
                    if !textures.is_empty() {
                        graph.materials_to_textures.insert(stem, textures);
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // 2. Scan Objects (TREModelResource & TREPlacementObject)
    // -------------------------------------------------------------------------
    let obj_dir = assets_dir.join("objects");
    if obj_dir.exists() {
        for entry in fs::read_dir(obj_dir)?.flatten() {
            if entry.path().extension().is_some_and(|e| e == "json") {
                let stem = entry
                    .path()
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let content = fs::read_to_string(entry.path())?;
                if let Ok(v) = serde_json::from_str::<Value>(&content) {
                    let mut meshes = Vec::new();
                    let mut materials = Vec::new();

                    if let Some(bindings) = v["mesh_bindings"].as_array() {
                        for b in bindings {
                            if let Some(m) = b["mesh_path"].as_str() {
                                meshes.push(m.to_string());
                            }
                            if let Some(mat) = b["material_path"].as_str() {
                                materials.push(mat.to_string());
                            }
                        }
                    }

                    if let Some(stand) = v["stand_model"]["object_path"].as_str() {
                        meshes.push(stand.to_string());
                    }
                    if let Some(placed) = v["placed_object"]["object_path"].as_str() {
                        meshes.push(placed.to_string());
                    }

                    clean_and_dedup(&mut meshes);
                    clean_and_dedup(&mut materials);

                    if !meshes.is_empty() {
                        graph.objects_to_meshes.insert(stem.clone(), meshes);
                    }
                    if !materials.is_empty() {
                        graph.objects_to_materials.insert(stem, materials);
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // 3. Scan Characters, Actors & Destructibles (TREActorController / TREBreakable)
    // -------------------------------------------------------------------------
    let char_dir = assets_dir.join("characters");
    if char_dir.exists() {
        for entry in fs::read_dir(char_dir)?.flatten() {
            if entry.path().extension().is_some_and(|e| e == "json") {
                let stem = entry
                    .path()
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let content = fs::read_to_string(entry.path())?;
                if let Ok(v) = serde_json::from_str::<Value>(&content) {
                    let mut meshes = Vec::new();
                    let mut animations = Vec::new();
                    let mut sounds = Vec::new();
                    let mut facefx = Vec::new();

                    if let Some(mb) = v["model_binding"]["object_path"].as_str() {
                        meshes.push(mb.to_string());
                    }
                    if let Some(ct) = v["collapse_target_model"].as_str() {
                        meshes.push(ct.to_string());
                    }
                    if let Some(ct) = v["breakable_config"]["collapse_target_model"].as_str() {
                        meshes.push(ct.to_string());
                    }

                    if let Some(eq) = v["equipment"]["item_name"].as_str() {
                        meshes.push(eq.to_string());
                    }

                    if let Some(anim_states) = v["animation_states"].as_array() {
                        for a in anim_states {
                            if let Some(clip) = a["anim_clip"].as_str() {
                                animations.push(clip.to_string());
                            }
                            if let Some(cues) = a["sound_cues"].as_array() {
                                for c in cues {
                                    if let Some(s) = c.as_str() {
                                        sounds.push(s.to_string());
                                    }
                                }
                            }
                        }
                    }

                    if let Some(fx) = v["facefx_actor"].as_str() {
                        facefx.push(fx.to_string());
                    }
                    if let Some(fx_file) = v["embedded_facefx_file"].as_str() {
                        facefx.push(fx_file.to_string());
                    }

                    clean_and_dedup(&mut meshes);
                    clean_and_dedup(&mut animations);
                    clean_and_dedup(&mut sounds);
                    clean_and_dedup(&mut facefx);

                    if !meshes.is_empty() {
                        graph.objects_to_meshes.insert(stem.clone(), meshes);
                    }
                    if !animations.is_empty() {
                        graph
                            .characters_to_animations
                            .insert(stem.clone(), animations);
                    }
                    if !sounds.is_empty() {
                        graph.characters_to_sounds.insert(stem.clone(), sounds);
                    }
                    if !facefx.is_empty() {
                        graph.characters_to_facefx.insert(stem, facefx);
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // 4. Scan Attachments & Handheld Props (TREItemResource)
    // -------------------------------------------------------------------------
    let attach_dir = assets_dir.join("attachments");
    if attach_dir.exists() {
        for entry in fs::read_dir(attach_dir)?.flatten() {
            if entry.path().extension().is_some_and(|e| e == "json") {
                let stem = entry
                    .path()
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let content = fs::read_to_string(entry.path())?;
                if let Ok(v) = serde_json::from_str::<Value>(&content) {
                    let mut meshes = Vec::new();
                    let mut sounds = Vec::new();

                    if let Some(pkg) = v["mesh_package"].as_str() {
                        meshes.push(pkg.to_string());
                    }
                    if let Some(slot) = v["internal_model_slot"].as_str() {
                        meshes.push(slot.to_string());
                    }
                    if let Some(sb) = v["sound_bank"].as_str() {
                        sounds.push(sb.to_string());
                    }

                    clean_and_dedup(&mut meshes);
                    clean_and_dedup(&mut sounds);

                    if !meshes.is_empty() {
                        graph
                            .attachments_to_meshes
                            .insert(stem.clone(), meshes.clone());
                        // Also associate with objects_to_meshes for backward compatibility
                        graph.objects_to_meshes.insert(stem.clone(), meshes);
                    }
                    if !sounds.is_empty() {
                        graph.attachments_to_sounds.insert(stem, sounds);
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // 5. Scan Terrain Texture Palettes & Biomes
    // -------------------------------------------------------------------------
    let tp_dir = assets_dir.join("terrain_palettes");
    if tp_dir.exists() {
        for entry in fs::read_dir(tp_dir)?.flatten() {
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
                    let mut meshes = Vec::new();

                    if let Some(tex_list) = v["textures"].as_array() {
                        for t in tex_list {
                            if let Some(ptr) = t["pointer_tag"].as_str() {
                                textures.push(ptr.to_string());
                            }
                            if let Some(file) = t["filename"].as_str() {
                                textures.push(file.to_string());
                            }
                        }
                    }

                    if let Some(env_sky) = v["environment"]["sky_textures"].as_array() {
                        for t in env_sky {
                            if let Some(file) = t["filename"].as_str() {
                                textures.push(file.to_string());
                            }
                        }
                    }

                    if let Some(env_water) = v["environment"]["water_textures"].as_array() {
                        for t in env_water {
                            if let Some(file) = t["filename"].as_str() {
                                textures.push(file.to_string());
                            }
                        }
                    }

                    if let Some(env_mesh) = v["environment"]["sky_meshes"].as_array() {
                        for t in env_mesh {
                            if let Some(file) = t["filename"].as_str() {
                                meshes.push(file.to_string());
                            }
                        }
                    }

                    if let Some(foliage_groups) = v["foliage_scatter_groups"].as_object() {
                        for (_group, group_meshes) in foliage_groups {
                            if let Some(mesh_arr) = group_meshes.as_array() {
                                for m in mesh_arr {
                                    if let Some(file) = m["mesh_file"].as_str() {
                                        meshes.push(file.to_string());
                                    }
                                }
                            }
                        }
                    }

                    clean_and_dedup(&mut textures);
                    clean_and_dedup(&mut meshes);

                    if !textures.is_empty() {
                        graph
                            .terrain_palettes_to_textures
                            .insert(stem.clone(), textures);
                    }
                    if !meshes.is_empty() {
                        graph.terrain_palettes_to_meshes.insert(stem, meshes);
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // 6. Scan Animation SFX Events
    // -------------------------------------------------------------------------
    let evt_dir = assets_dir.join("events");
    if evt_dir.exists() {
        for entry in fs::read_dir(evt_dir)?.flatten() {
            if entry.path().extension().is_some_and(|e| e == "json") {
                let stem = entry
                    .path()
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let content = fs::read_to_string(entry.path())?;
                if let Ok(v) = serde_json::from_str::<Value>(&content) {
                    let mut sounds = Vec::new();
                    if let Some(vars) = v["variations"].as_array() {
                        for var in vars {
                            if let Some(res) = var["sound_resource"].as_str() {
                                sounds.push(res.to_string());
                            }
                        }
                    }
                    clean_and_dedup(&mut sounds);
                    if !sounds.is_empty() {
                        graph.events_to_sounds.insert(stem, sounds);
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // 7. Scan Fonts (TREFont)
    // -------------------------------------------------------------------------
    let font_dir = assets_dir.join("fonts");
    if font_dir.exists() {
        for entry in fs::read_dir(font_dir)?.flatten() {
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
                    if let Some(tlink) = v["_engine_metadata"]["texture_link"].as_str() {
                        textures.push(tlink.to_string());
                    }
                    clean_and_dedup(&mut textures);
                    if !textures.is_empty() {
                        graph.fonts_to_textures.insert(stem, textures);
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // 8. Scan Voice Packages (.debug-vpk)
    // -------------------------------------------------------------------------
    let vpk_dir = assets_dir.join("voice_packages");
    if vpk_dir.exists() {
        for entry in fs::read_dir(vpk_dir)?.flatten() {
            if entry.path().extension().is_some_and(|e| e == "json") {
                let stem = entry
                    .path()
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let content = fs::read_to_string(entry.path())?;
                if let Ok(v) = serde_json::from_str::<Value>(&content) {
                    let mut files = Vec::new();
                    if let Some(clb) = v["base_voice_clb"].as_str() {
                        files.push(clb.to_string());
                    }
                    if let Some(cues) = v["voice_cues"].as_array() {
                        for c in cues {
                            if let Some(s) = c["sample_id"].as_str() {
                                files.push(s.to_string());
                            }
                        }
                    }
                    clean_and_dedup(&mut files);
                    if !files.is_empty() {
                        graph.voice_packages_to_files.insert(stem, files);
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // 9. Scan Parameters -> Collision Boundary (.clb) references
    // -------------------------------------------------------------------------
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
                if let Ok(v) = serde_json::from_str::<Value>(&content) {
                    if let Some(files) = v["files"].as_array() {
                        let mut list: Vec<String> = files
                            .iter()
                            .filter_map(|s| s.as_str().map(|s| s.to_string()))
                            .collect();
                        clean_and_dedup(&mut list);
                        if !list.is_empty() {
                            graph.collisions_to_files.insert(stem, list);
                        }
                    } else if let Some(text) = v["value"].as_str()
                        && text.contains(".clb")
                    {
                        graph
                            .collisions_to_files
                            .insert(stem, vec![text.to_string()]);
                    }
                }
            }
        }
    }

    // Write formatted graph JSON file
    let graph_path = assets_dir.join("dependency_graph.json");
    let json_bytes = serde_json::to_string_pretty(&graph)?;
    fs::write(graph_path, json_bytes)?;

    Ok(graph)
}
