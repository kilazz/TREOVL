pub mod cache;
pub mod processors;

pub use cache::{AssetSyncCache, AssetSyncEntry, calculate_crc32};
pub use processors::{AssetProcessor, ProjectWorkspace};

use anyhow::{Context, Result, bail};
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};

use crate::engine::assets::sniffer::sniff_asset;
use processors::{RawProcessor, get_standard_processors};

pub fn export_smart_assets(project_dir: &Path) -> Result<usize> {
    let workspace = ProjectWorkspace::new(project_dir);

    let source_chunks_dir = if workspace.vanilla_chunks_dir.exists() {
        &workspace.vanilla_chunks_dir
    } else {
        &workspace.chunks_dir
    };

    if !workspace.base_dir.exists() || !source_chunks_dir.exists() {
        bail!(
            "Project directory or chunks folder does not exist: {:?}",
            workspace.base_dir
        );
    }

    let dirs = [
        "textures",
        "audio",
        "materials",
        "meshes",
        "animations",
        "scripts",
        "raw_chunks",
        "xml",
        "parameters",
        "ui",
        "objects",
        "vfx",
        "events",
        "facefx",
        "characters",
        "attachments",
        "terrain_palettes",
        "collisions",
        "fonts",
    ];
    for dir in dirs {
        fs::create_dir_all(workspace.assets_dir.join(dir))?;
    }

    let processors = get_standard_processors();
    let raw_processor = RawProcessor;

    let entries: Vec<PathBuf> = fs::read_dir(source_chunks_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "bin"))
        .collect();

    let sync_records: Vec<(String, AssetSyncEntry)> = entries
        .par_iter()
        .filter_map(|chunk_path| {
            let data = fs::read(chunk_path).ok()?;
            let stem = chunk_path.file_stem()?.to_string_lossy();
            let sniffed = sniff_asset(&data, &stem);

            for processor in &processors {
                match processor.process(&data, &stem, &sniffed, &workspace) {
                    Ok(Some(entry)) => return Some(entry),
                    Ok(None) => continue,
                    Err(e) => {
                        eprintln!("[!] Processor failed for {}: {}", stem, e);
                        continue;
                    }
                }
            }

            match raw_processor.process(&data, &stem, &sniffed, &workspace) {
                Ok(Some(entry)) => Some(entry),
                _ => None,
            }
        })
        .collect();

    let mut cache = AssetSyncCache::default();
    for (rel_path, record) in sync_records {
        cache.entries.insert(rel_path, record);
    }

    let cache_file = project_dir.join(".asset_cache.json");
    let serialized = serde_json::to_string_pretty(&cache)?;
    fs::write(cache_file, serialized)?;

    let _ = crate::engine::analysis::graph::build_dependency_graph(&workspace.assets_dir);

    Ok(cache.entries.len())
}

pub fn sync_assets_to_chunks(project_dir: &Path) -> Result<usize> {
    let cache_file = project_dir.join(".asset_cache.json");
    if !cache_file.exists() {
        return Ok(0);
    }

    let cache_data = fs::read_to_string(&cache_file)?;
    let mut cache: AssetSyncCache = serde_json::from_str(&cache_data)?;
    let mut synced_count = 0;

    let vanilla_dir = project_dir.join("chunks_vanilla");
    let working_dir = project_dir.join("chunks");

    for (rel_asset_path, entry) in cache.entries.iter_mut() {
        let chunk_file_name = Path::new(&entry.chunk_rel_path)
            .file_name()
            .context("Invalid chunk relative path")?;

        let vanilla_chunk_path = if vanilla_dir.exists() {
            vanilla_dir.join(chunk_file_name)
        } else {
            working_dir.join(chunk_file_name)
        };
        let working_chunk_path = working_dir.join(chunk_file_name);
        let abs_asset_path = project_dir.join(rel_asset_path);

        if !abs_asset_path.exists() {
            if entry.is_modified && vanilla_chunk_path.exists() {
                let _ = fs::copy(&vanilla_chunk_path, &working_chunk_path);
                entry.is_modified = false;
                synced_count += 1;
            }
            continue;
        }

        let asset_bytes = match fs::read(&abs_asset_path) {
            Ok(b) => b,
            Err(_) => continue,
        };

        let current_crc = calculate_crc32(&asset_bytes);

        if current_crc != entry.vanilla_crc32 {
            let baseline_chunk = if vanilla_chunk_path.exists() {
                fs::read(&vanilla_chunk_path)?
            } else {
                fs::read(&working_chunk_path)?
            };

            let updated_chunk = match entry.asset_kind.as_str() {
                "Character" => {
                    let json_str = String::from_utf8(asset_bytes)
                        .context("Character JSON is not valid UTF-8")?;
                    crate::engine::assets::character::import_character_from_json(
                        &json_str,
                        Some(project_dir),
                    )?
                }
                "Attachment" => {
                    let json_str = String::from_utf8(asset_bytes)
                        .context("Attachment JSON is not valid UTF-8")?;
                    crate::engine::assets::attachment::import_attachment_from_json(&json_str)?
                }
                "Texture" => crate::engine::assets::texture::replace_texture_in_chunk(
                    &baseline_chunk,
                    &asset_bytes,
                )?,
                "Audio" => {
                    crate::engine::assets::audio::replace_wav(&baseline_chunk, &asset_bytes)?
                }
                "Material" => {
                    let json_str = String::from_utf8(asset_bytes)
                        .context("Material JSON is not valid UTF-8")?;
                    crate::engine::assets::material::import_material_from_json(&json_str)?
                }
                "Mesh" => {
                    if rel_asset_path.ends_with(".glb") {
                        crate::engine::assets::mesh::import_glb_to_mesh(
                            &baseline_chunk,
                            &asset_bytes,
                        )?
                    } else {
                        let obj_str = String::from_utf8(asset_bytes)
                            .context("OBJ file is not valid UTF-8")?;
                        crate::engine::assets::mesh::import_obj_to_mesh(&baseline_chunk, &obj_str)?
                    }
                }
                "Lua" => {
                    let bytecode = if rel_asset_path.ends_with(".lua") {
                        match crate::engine::assets::lua::compile_lua_script(&abs_asset_path) {
                            Ok(compiled_bin) => compiled_bin,
                            Err(e) => {
                                eprintln!("[!] {}", e);
                                let luac_path = abs_asset_path.with_extension("luac");
                                if luac_path.exists() {
                                    fs::read(luac_path)?
                                } else {
                                    bail!("Cannot sync Lua script: {}", e);
                                }
                            }
                        }
                    } else {
                        asset_bytes
                    };
                    crate::engine::assets::lua::replace_lua_bytecode(&baseline_chunk, &bytecode)?
                }
                "UI" => {
                    let json_str =
                        String::from_utf8(asset_bytes).context("UI JSON is not valid UTF-8")?;
                    crate::engine::assets::ui::import_ui_from_json(&json_str)?
                }
                "Object" => {
                    let json_str =
                        String::from_utf8(asset_bytes).context("Object JSON is not valid UTF-8")?;
                    crate::engine::assets::object::import_object_from_json(&json_str)?
                }
                "TerrainPalette" => {
                    let json_str = String::from_utf8(asset_bytes)
                        .context("Terrain Palette JSON is not valid UTF-8")?;
                    crate::engine::assets::terrain_palette::import_terrain_palette_from_json(
                        &json_str,
                    )?
                }
                "Vfx" => {
                    let json_str =
                        String::from_utf8(asset_bytes).context("VFX JSON is not valid UTF-8")?;
                    crate::engine::assets::vfx::import_vfx_from_json(&json_str)?
                }
                "Event" => {
                    let json_str =
                        String::from_utf8(asset_bytes).context("Event JSON is not valid UTF-8")?;
                    crate::engine::assets::event::import_event_from_json(&json_str)?
                }
                "Collision" => {
                    let json_str = String::from_utf8(asset_bytes)
                        .context("Collision JSON is not valid UTF-8")?;
                    crate::engine::assets::collision::import_collision_from_json(
                        &json_str,
                        &baseline_chunk,
                    )?
                }
                "Font" => {
                    let json_str =
                        String::from_utf8(asset_bytes).context("Font JSON is not valid UTF-8")?;
                    crate::engine::assets::font::import_font_from_json(&json_str, &baseline_chunk)?
                }
                "FaceFx" => {
                    let fxe_path = abs_asset_path.with_extension("fxe");
                    if fxe_path.exists() {
                        let fxe_data = fs::read(&fxe_path)?;
                        if let Some(pos) = baseline_chunk.windows(4).position(|w| w == b"FACE") {
                            let mut new_chunk = baseline_chunk[..pos].to_vec();
                            new_chunk.extend_from_slice(&fxe_data);
                            new_chunk
                        } else {
                            baseline_chunk
                        }
                    } else {
                        baseline_chunk
                    }
                }
                "Animation" => baseline_chunk,
                "Parameter" => crate::engine::assets::parameter::import_parameter_from_json(
                    &asset_bytes,
                    &baseline_chunk,
                )?,
                "Xml" => {
                    crate::engine::assets::xml::import_xml_payload(&baseline_chunk, &asset_bytes)?
                }
                "Raw" => asset_bytes,
                _ => continue,
            };

            fs::write(&working_chunk_path, updated_chunk)?;
            entry.is_modified = true;
            synced_count += 1;
        } else if entry.is_modified && vanilla_chunk_path.exists() {
            let _ = fs::copy(&vanilla_chunk_path, &working_chunk_path);
            entry.is_modified = false;
            synced_count += 1;
        }
    }

    if synced_count > 0 {
        fs::write(cache_file, serde_json::to_string_pretty(&cache)?)?;
    }
    Ok(synced_count)
}

pub fn revert_single_asset(project_dir: &Path, chunk_path_str: &str) -> Result<()> {
    let chunk_path = Path::new(chunk_path_str);
    let chunk_file_name = chunk_path.file_name().context("Invalid chunk filename")?;

    let vanilla_path = project_dir.join("chunks_vanilla").join(chunk_file_name);
    let working_path = project_dir.join("chunks").join(chunk_file_name);

    if !vanilla_path.exists() {
        bail!("Vanilla baseline does not exist for {:?}", chunk_file_name);
    }

    fs::copy(&vanilla_path, &working_path)?;

    let data = fs::read(&vanilla_path)?;
    let stem = Path::new(chunk_file_name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let sniffed = sniff_asset(&data, &stem);
    let workspace = ProjectWorkspace::new(project_dir);

    let mut processors = get_standard_processors();
    processors.push(Box::new(RawProcessor));

    for processor in &processors {
        if let Ok(Some((rel_path, mut sync_entry))) =
            processor.process(&data, &stem, &sniffed, &workspace)
        {
            sync_entry.is_modified = false;
            let cache_file = project_dir.join(".asset_cache.json");
            if cache_file.exists()
                && let Ok(content) = fs::read_to_string(&cache_file)
                && let Ok(mut cache) = serde_json::from_str::<AssetSyncCache>(&content)
            {
                cache.entries.insert(rel_path, sync_entry);
                let _ = fs::write(cache_file, serde_json::to_string_pretty(&cache)?);
            }
            break;
        }
    }

    Ok(())
}

pub fn clean_rebuild_project(project_dir: &Path) -> Result<usize> {
    let vanilla_dir = project_dir.join("chunks_vanilla");
    let working_dir = project_dir.join("chunks");

    if !vanilla_dir.exists() {
        bail!("No chunks_vanilla/ baseline folder found in project!");
    }

    for entry in fs::read_dir(&vanilla_dir)?.flatten() {
        let dest = working_dir.join(entry.file_name());
        fs::copy(entry.path(), dest)?;
    }

    sync_assets_to_chunks(project_dir)
}
