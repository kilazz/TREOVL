pub mod cache;
pub mod processors;

pub use cache::{AssetSyncCache, AssetSyncEntry, calculate_crc32};
pub use processors::{AssetProcessor, ProjectWorkspace};

use anyhow::{Context, Result, bail};
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::engine::assets::codec::reencode_asset_to_chunk;
use crate::engine::assets::sniffer::sniff_asset;
use crate::engine::common::Endian;
use processors::{RawProcessor, get_standard_processors};

pub type ProgressCallback<'a> = &'a (dyn Fn(f32, &str) + Send + Sync);

pub fn export_smart_assets(project_dir: &Path) -> Result<usize> {
    export_smart_assets_with_progress(project_dir, None)
}

pub fn export_smart_assets_with_progress(
    project_dir: &Path,
    progress: Option<ProgressCallback>,
) -> Result<usize> {
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

    fs::create_dir_all(&workspace.assets_dir)?;

    let processors = get_standard_processors();
    let raw_processor = RawProcessor;

    let entries: Vec<PathBuf> = fs::read_dir(source_chunks_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "bin"))
        .collect();

    let total_entries = entries.len();
    let processed_counter = AtomicUsize::new(0);

    let sync_records: Vec<(String, AssetSyncEntry)> = entries
        .par_iter()
        .filter_map(|chunk_path| {
            let data = fs::read(chunk_path).ok()?;
            let stem = chunk_path.file_stem()?.to_string_lossy();
            let sniffed = sniff_asset(&data, &stem);

            let res = (|| {
                for processor in &processors {
                    match processor.process(&data, &stem, &sniffed, &workspace) {
                        Ok(Some(entry)) => return Some(entry),
                        Ok(None) => continue,
                        Err(e) => {
                            eprintln!(
                                "[!] Processor error on {} ({}): {:#}",
                                stem, sniffed.kind_name, e
                            );
                            continue;
                        }
                    }
                }
                match raw_processor.process(&data, &stem, &sniffed, &workspace) {
                    Ok(Some(entry)) => Some(entry),
                    _ => None,
                }
            })();

            let current_count = processed_counter.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(cb) = progress
                && total_entries > 0
                && current_count.is_multiple_of(10)
            {
                let frac = 0.60 + 0.35 * (current_count as f32 / total_entries as f32);
                cb(
                    frac,
                    &format!("Exporting asset {} of {}...", current_count, total_entries),
                );
            }

            res
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
    sync_assets_to_chunks_with_progress(project_dir, None)
}

pub fn sync_assets_to_chunks_with_progress(
    project_dir: &Path,
    progress: Option<ProgressCallback>,
) -> Result<usize> {
    let cache_file = project_dir.join(".asset_cache.json");
    if !cache_file.exists() {
        return Ok(0);
    }

    let cache_data = fs::read_to_string(&cache_file)?;
    let mut cache: AssetSyncCache = serde_json::from_str(&cache_data)?;
    let mut synced_count = 0;

    let vanilla_dir = project_dir.join("chunks_vanilla");
    let working_dir = project_dir.join("chunks");

    let manifest_endian = project_dir
        .join("project.json")
        .exists()
        .then(|| {
            fs::read_to_string(project_dir.join("project.json"))
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| {
                    v.get("endian")
                        .and_then(|e| serde_json::from_value::<Endian>(e.clone()).ok())
                })
        })
        .flatten()
        .unwrap_or(Endian::Little);

    let total = cache.entries.len();
    let mut current: usize = 0;

    for (rel_asset_path, entry) in cache.entries.iter_mut() {
        current += 1;
        if let Some(cb) = progress
            && total > 0
            && current.is_multiple_of(15)
        {
            let frac = 0.10 + 0.35 * (current as f32 / total as f32);
            cb(
                frac,
                &format!("Checking modifications {}/{}...", current, total),
            );
        }

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

            let updated_chunk = reencode_asset_to_chunk(
                entry.asset_kind,
                &baseline_chunk,
                &asset_bytes,
                manifest_endian,
                Some(project_dir),
                Some(rel_asset_path),
            )?;

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
        match processor.process(&data, &stem, &sniffed, &workspace) {
            Ok(Some((rel_path, mut sync_entry))) => {
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
            Ok(None) => continue,
            Err(e) => {
                eprintln!(
                    "[!] Revert processor error on {} ({}): {:#}",
                    stem, sniffed.kind_name, e
                );
                continue;
            }
        }
    }

    Ok(())
}

pub fn clean_rebuild_project(project_dir: &Path) -> Result<usize> {
    clean_rebuild_project_with_progress(project_dir, None)
}

pub fn clean_rebuild_project_with_progress(
    project_dir: &Path,
    progress: Option<ProgressCallback>,
) -> Result<usize> {
    let vanilla_dir = project_dir.join("chunks_vanilla");
    let working_dir = project_dir.join("chunks");

    if !vanilla_dir.exists() {
        bail!("No chunks_vanilla/ baseline folder found in project!");
    }

    if let Some(cb) = progress {
        cb(0.1, "Restoring chunks from vanilla baseline...");
    }

    for entry in fs::read_dir(&vanilla_dir)?.flatten() {
        let dest = working_dir.join(entry.file_name());
        fs::copy(entry.path(), dest)?;
    }

    sync_assets_to_chunks_with_progress(project_dir, progress)
}
