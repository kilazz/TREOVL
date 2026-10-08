use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AssetLink {
    pub label: String,
    pub target_name: String,
    pub category: String,
    pub icon: String,
}

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

    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub logic_markers_to_environments: HashMap<String, Vec<String>>,

    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub environments_to_logic_markers: HashMap<String, Vec<String>>,
}

fn clean_and_dedup(list: &mut Vec<String>) {
    list.retain(|s| !s.trim().is_empty());
    list.sort();
    list.dedup();
}

impl DependencyGraph {
    pub fn find_links_for_asset(&self, stem: &str) -> Vec<AssetLink> {
        let mut links = Vec::new();
        let stem_norm = stem.trim().to_lowercase();
        if stem_norm.is_empty() {
            return links;
        }

        let matches = |a: &str, b: &str| -> bool {
            let la = a.to_lowercase();
            let lb = b.to_lowercase();
            la == lb || la.contains(&lb) || lb.contains(&la)
        };

        // 1. Materials <-> Textures
        for (mat, textures) in &self.materials_to_textures {
            if matches(mat, &stem_norm) {
                for tex in textures {
                    links.push(AssetLink {
                        label: format!("Texture: {}", tex),
                        target_name: tex.clone(),
                        category: "Texture".into(),
                        icon: "🎨".into(),
                    });
                }
            } else {
                for tex in textures {
                    if matches(tex, &stem_norm) {
                        links.push(AssetLink {
                            label: format!("Material: {}", mat),
                            target_name: mat.clone(),
                            category: "Material".into(),
                            icon: "🛠️".into(),
                        });
                    }
                }
            }
        }

        // 2. Objects <-> Meshes and Materials
        for (obj, meshes) in &self.objects_to_meshes {
            if matches(obj, &stem_norm) {
                for m in meshes {
                    links.push(AssetLink {
                        label: format!("Mesh: {}", m),
                        target_name: m.clone(),
                        category: "Mesh".into(),
                        icon: "🗿".into(),
                    });
                }
            } else {
                for m in meshes {
                    if matches(m, &stem_norm) {
                        links.push(AssetLink {
                            label: format!("Object: {}", obj),
                            target_name: obj.clone(),
                            category: "Object".into(),
                            icon: "🧊".into(),
                        });
                    }
                }
            }
        }

        for (obj, mats) in &self.objects_to_materials {
            if matches(obj, &stem_norm) {
                for m in mats {
                    links.push(AssetLink {
                        label: format!("Material: {}", m),
                        target_name: m.clone(),
                        category: "Material".into(),
                        icon: "🛠️".into(),
                    });
                }
            } else {
                for m in mats {
                    if matches(m, &stem_norm) {
                        links.push(AssetLink {
                            label: format!("Used By Object: {}", obj),
                            target_name: obj.clone(),
                            category: "Object".into(),
                            icon: "🧊".into(),
                        });
                    }
                }
            }
        }

        // 3. Characters <-> Animations, Sounds, FaceFX
        for (char_name, anims) in &self.characters_to_animations {
            if matches(char_name, &stem_norm) {
                for a in anims {
                    links.push(AssetLink {
                        label: format!("Animation: {}", a),
                        target_name: a.clone(),
                        category: "Animation".into(),
                        icon: "🎬".into(),
                    });
                }
            } else {
                for a in anims {
                    if matches(a, &stem_norm) {
                        links.push(AssetLink {
                            label: format!("Actor: {}", char_name),
                            target_name: char_name.clone(),
                            category: "Character".into(),
                            icon: "🧙‍♂️".into(),
                        });
                    }
                }
            }
        }

        for (char_name, sounds) in &self.characters_to_sounds {
            if matches(char_name, &stem_norm) {
                for snd in sounds {
                    links.push(AssetLink {
                        label: format!("Audio Cue: {}", snd),
                        target_name: snd.clone(),
                        category: "Audio".into(),
                        icon: "🎵".into(),
                    });
                }
            }
        }

        for (char_name, ffx_list) in &self.characters_to_facefx {
            if matches(char_name, &stem_norm) {
                for ffx in ffx_list {
                    links.push(AssetLink {
                        label: format!("FaceFX: {}", ffx),
                        target_name: ffx.clone(),
                        category: "FaceFX".into(),
                        icon: "🗣️".into(),
                    });
                }
            }
        }

        // 4. Attachments <-> Meshes and Sounds
        for (att, meshes) in &self.attachments_to_meshes {
            if matches(att, &stem_norm) {
                for m in meshes {
                    links.push(AssetLink {
                        label: format!("Attached Mesh: {}", m),
                        target_name: m.clone(),
                        category: "Mesh".into(),
                        icon: "🗡️".into(),
                    });
                }
            }
        }

        for (att, sounds) in &self.attachments_to_sounds {
            if matches(att, &stem_norm) {
                for snd in sounds {
                    links.push(AssetLink {
                        label: format!("Sound Bank: {}", snd),
                        target_name: snd.clone(),
                        category: "Audio".into(),
                        icon: "🎵".into(),
                    });
                }
            }
        }

        // 5. Terrain Palettes <-> Textures and Meshes
        for (pal, textures) in &self.terrain_palettes_to_textures {
            if matches(pal, &stem_norm) {
                for tex in textures {
                    links.push(AssetLink {
                        label: format!("Terrain Tex: {}", tex),
                        target_name: tex.clone(),
                        category: "Texture".into(),
                        icon: "🎨".into(),
                    });
                }
            }
        }

        for (pal, meshes) in &self.terrain_palettes_to_meshes {
            if matches(pal, &stem_norm) {
                for m in meshes {
                    links.push(AssetLink {
                        label: format!("Foliage Mesh: {}", m),
                        target_name: m.clone(),
                        category: "Mesh".into(),
                        icon: "🌿".into(),
                    });
                }
            }
        }

        // 6. Logic Markers <-> Environments (Direct Handle / FlagsHex Matching)
        for (marker, envs) in &self.logic_markers_to_environments {
            if matches(marker, &stem_norm) {
                for env in envs {
                    links.push(AssetLink {
                        label: format!("Linked Environment: {}", env),
                        target_name: env.clone(),
                        category: "Environment".into(),
                        icon: "🌌".into(),
                    });
                }
            }
        }

        for (env, markers) in &self.environments_to_logic_markers {
            if matches(env, &stem_norm) {
                for m in markers {
                    links.push(AssetLink {
                        label: format!("Triggered by Marker: {}", m),
                        target_name: m.clone(),
                        category: "Object".into(),
                        icon: "⚡".into(),
                    });
                }
            }
        }

        // 7. Fonts <-> Textures
        for (font, textures) in &self.fonts_to_textures {
            if matches(font, &stem_norm) {
                for tex in textures {
                    links.push(AssetLink {
                        label: format!("Font Texture: {}", tex),
                        target_name: tex.clone(),
                        category: "Texture".into(),
                        icon: "🔤".into(),
                    });
                }
            }
        }

        // 8. Events <-> Sounds
        for (evt, sounds) in &self.events_to_sounds {
            if matches(evt, &stem_norm) {
                for snd in sounds {
                    links.push(AssetLink {
                        label: format!("Event Sound: {}", snd),
                        target_name: snd.clone(),
                        category: "Audio".into(),
                        icon: "🎵".into(),
                    });
                }
            }
        }

        links.dedup_by(|a, b| a.target_name.eq_ignore_ascii_case(&b.target_name));
        links
    }
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

    // 2. Scan Environments (for Handle & Flags mapping)
    let env_dir = assets_dir.join("environments");
    let mut env_flags_to_stem: HashMap<String, String> = HashMap::new();
    if env_dir.exists() {
        for entry in fs::read_dir(&env_dir)?.flatten() {
            if entry.path().extension().is_some_and(|e| e == "json") {
                let stem = entry
                    .path()
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if let Ok(content) = fs::read_to_string(entry.path())
                    && let Ok(v) = serde_json::from_str::<Value>(&content)
                    && let Some(flags_hex) = v["flags_hex"].as_str()
                {
                    let clean = flags_hex.trim().to_lowercase();
                    env_flags_to_stem.insert(clean, stem);
                }
            }
        }
    }

    // 3. Scan Objects (TREModelResource, TREPlacementObject, TRELogicMarker)
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

                    // Cross-link LogicMarker to Environment profiles via raw_hex_id
                    if let Some(raw_id) = v["logic_event_link"]["raw_hex_id"].as_str() {
                        let clean_id = raw_id.trim().to_lowercase();
                        if let Some(env_stem) = env_flags_to_stem.get(&clean_id) {
                            graph
                                .logic_markers_to_environments
                                .entry(stem.clone())
                                .or_default()
                                .push(env_stem.clone());
                            graph
                                .environments_to_logic_markers
                                .entry(env_stem.clone())
                                .or_default()
                                .push(stem.clone());
                        }
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

    // 4. Scan Characters, Actors & Destructibles
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

    // 5. Scan Attachments & Handheld Props
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
                        graph.objects_to_meshes.insert(stem.clone(), meshes);
                    }
                    if !sounds.is_empty() {
                        graph.attachments_to_sounds.insert(stem, sounds);
                    }
                }
            }
        }
    }

    // 6. Scan Terrain Texture Palettes & Biomes
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

                    if let Some(env_mesh) = v["environment"]["sky_meshes"].as_array() {
                        for t in env_mesh {
                            if let Some(file) = t["filename"].as_str() {
                                meshes.push(file.to_string());
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

    // 7. Scan Animation SFX Events
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

    // 8. Scan Fonts
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

    // 9. Scan Voice Packages
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

    // 10. Scan Parameters -> Collision references
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

    let graph_path = assets_dir.join("dependency_graph.json");
    let json_bytes = serde_json::to_string_pretty(&graph)?;
    fs::write(graph_path, json_bytes)?;

    Ok(graph)
}
