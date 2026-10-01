use anyhow::{Context, Result, bail};
use crc32fast::Hasher;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::engine::assets::animation::{export_skeleton_to_glb, parse_object_bone_container};
use crate::engine::assets::sniffer::{AssetKind, SniffedAsset, sniff_asset};
use crate::engine::common::magic;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AssetSyncEntry {
    pub chunk_rel_path: String,
    pub asset_kind: String,
    pub crc32: u32,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct AssetSyncCache {
    pub entries: HashMap<String, AssetSyncEntry>,
}

fn calculate_crc32(data: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

fn sanitize_filename(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '[' | ']' => '_',
            _ => c,
        })
        .collect();
    clean.trim_matches('_').to_string()
}

fn build_asset_filename(display_name: &str, stem: &str, ext: &str) -> String {
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

pub struct ProjectWorkspace<'a> {
    pub base_dir: &'a Path,
    pub chunks_dir: PathBuf,
    pub assets_dir: PathBuf,
}

impl<'a> ProjectWorkspace<'a> {
    pub fn new(base_dir: &'a Path) -> Self {
        Self {
            base_dir,
            chunks_dir: base_dir.join("chunks"),
            assets_dir: base_dir.join("assets"),
        }
    }
}

pub trait AssetProcessor: Sync + Send {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>>;
}

pub struct TextureProcessor;
impl AssetProcessor for TextureProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Texture {
            return Ok(None);
        }
        let dds_or_tga = crate::engine::assets::texture::export_to_dds(data)?;
        let ext = if data.starts_with(magic::TEX_INTERFACE) {
            "tga"
        } else {
            "dds"
        };
        let out_name = build_asset_filename(&sniffed.display_name, stem, ext);

        fs::write(
            workspace.assets_dir.join("textures").join(&out_name),
            &dds_or_tga,
        )?;
        Ok(Some((
            format!("assets/textures/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Texture".into(),
                crc32: calculate_crc32(&dds_or_tga),
            },
        )))
    }
}

pub struct AudioProcessor;
impl AssetProcessor for AudioProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Audio {
            return Ok(None);
        }
        let wav_bytes = crate::engine::assets::audio::export_wav(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "wav");

        fs::write(
            workspace.assets_dir.join("audio").join(&out_name),
            &wav_bytes,
        )?;
        Ok(Some((
            format!("assets/audio/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Audio".into(),
                crc32: calculate_crc32(&wav_bytes),
            },
        )))
    }
}

pub struct LuaProcessor;
impl AssetProcessor for LuaProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Lua {
            return Ok(None);
        }
        let bytecode = crate::engine::assets::lua::extract_lua_bytecode(data)
            .map_err(|e| anyhow::anyhow!(e))?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "luac");

        fs::write(
            workspace.assets_dir.join("scripts").join(&out_name),
            &bytecode,
        )?;

        if let Ok(disasm) = crate::engine::assets::lua::disassemble_lua_bytecode(&bytecode) {
            fs::write(
                workspace
                    .assets_dir
                    .join("scripts")
                    .join(format!("{}.lua.txt", out_name)),
                disasm,
            )?;
        }

        Ok(Some((
            format!("assets/scripts/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Lua".into(),
                crc32: calculate_crc32(&bytecode),
            },
        )))
    }
}

pub struct MaterialProcessor;
impl AssetProcessor for MaterialProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Material {
            return Ok(None);
        }
        let json_str = crate::engine::assets::material::export_material_to_json(data)
            .map_err(|e| anyhow::anyhow!(e))?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");

        fs::write(
            workspace.assets_dir.join("materials").join(&out_name),
            json_str.as_bytes(),
        )?;
        Ok(Some((
            format!("assets/materials/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Material".into(),
                crc32: calculate_crc32(json_str.as_bytes()),
            },
        )))
    }
}

pub struct MeshProcessor;
impl AssetProcessor for MeshProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Mesh {
            return Ok(None);
        }
        let (glb_bytes, _) = crate::engine::assets::mesh::export_mesh_to_glb(data)
            .map_err(|e| anyhow::anyhow!(e))?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "glb");

        fs::write(
            workspace.assets_dir.join("meshes").join(&out_name),
            &glb_bytes,
        )?;

        // Automatically exports the armature rig for 3D animators if bone data is present
        if let Ok(bones) = parse_object_bone_container(data)
            && !bones.is_empty()
            && let Ok(rig_glb) = export_skeleton_to_glb(&bones, &sniffed.display_name)
        {
            let rig_name = build_asset_filename(&sniffed.display_name, stem, "rig.glb");
            let _ = fs::write(workspace.assets_dir.join("meshes").join(&rig_name), rig_glb);
        }

        Ok(Some((
            format!("assets/meshes/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Mesh".into(),
                crc32: calculate_crc32(&glb_bytes),
            },
        )))
    }
}

pub struct RawProcessor;
impl AssetProcessor for RawProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        _sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        let out_name = format!("{}.bin", stem);
        fs::write(
            workspace.assets_dir.join("raw_chunks").join(&out_name),
            data,
        )?;
        Ok(Some((
            format!("assets/raw_chunks/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Raw".into(),
                crc32: calculate_crc32(data),
            },
        )))
    }
}

pub fn export_smart_assets(project_dir: &Path) -> Result<usize> {
    let workspace = ProjectWorkspace::new(project_dir);

    // ACTIVELY USES workspace.base_dir to validate the project directory structure
    if !workspace.base_dir.exists() || !workspace.chunks_dir.exists() {
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
    ];
    for dir in dirs {
        fs::create_dir_all(workspace.assets_dir.join(dir))?;
    }

    let processors: Vec<Box<dyn AssetProcessor>> = vec![
        Box::new(TextureProcessor),
        Box::new(AudioProcessor),
        Box::new(MaterialProcessor),
        Box::new(MeshProcessor),
        Box::new(LuaProcessor),
        Box::new(RawProcessor),
    ];

    let entries: Vec<PathBuf> = fs::read_dir(&workspace.chunks_dir)?
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
                        eprintln!("[!] Failed to process chunk {}: {}", stem, e);
                        break;
                    }
                }
            }
            None
        })
        .collect();

    let mut cache = AssetSyncCache::default();
    for (rel_path, record) in sync_records {
        cache.entries.insert(rel_path, record);
    }

    let cache_file = project_dir.join(".asset_cache.json");
    let serialized = serde_json::to_string_pretty(&cache)?;
    fs::write(cache_file, serialized)?;

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

    for (rel_asset_path, entry) in cache.entries.iter_mut() {
        let abs_asset_path = project_dir.join(rel_asset_path);
        let abs_chunk_path = project_dir.join(&entry.chunk_rel_path);

        if !abs_asset_path.exists() || !abs_chunk_path.exists() {
            continue;
        }

        let asset_bytes = match fs::read(&abs_asset_path) {
            Ok(b) => b,
            Err(_) => continue,
        };

        let current_crc = calculate_crc32(&asset_bytes);

        if current_crc != entry.crc32 {
            let chunk_bytes = fs::read(&abs_chunk_path)?;
            let updated_chunk = match entry.asset_kind.as_str() {
                "Texture" => crate::engine::assets::texture::replace_texture_in_chunk(
                    &chunk_bytes,
                    &asset_bytes,
                )?,
                "Audio" => crate::engine::assets::audio::replace_wav(&chunk_bytes, &asset_bytes)?,
                "Material" => {
                    let json_str = String::from_utf8(asset_bytes)
                        .context("Material JSON is not valid UTF-8")?;
                    crate::engine::assets::material::import_material_from_json(&json_str)
                        .map_err(|e| anyhow::anyhow!(e))?
                }
                "Mesh" => {
                    if rel_asset_path.ends_with(".glb") {
                        crate::engine::assets::mesh::import_glb_to_mesh(&chunk_bytes, &asset_bytes)
                            .map_err(|e| anyhow::anyhow!(e))?
                    } else {
                        let obj_str = String::from_utf8(asset_bytes)
                            .context("OBJ file is not valid UTF-8")?;
                        crate::engine::assets::mesh::import_obj_to_mesh(&chunk_bytes, &obj_str)
                            .map_err(|e| anyhow::anyhow!(e))?
                    }
                }
                "Lua" => {
                    crate::engine::assets::lua::replace_lua_bytecode(&chunk_bytes, &asset_bytes)
                        .map_err(|e| anyhow::anyhow!(e))?
                }
                "Raw" => asset_bytes,
                _ => continue,
            };

            fs::write(&abs_chunk_path, updated_chunk)?;
            entry.crc32 = current_crc;
            synced_count += 1;
        }
    }

    if synced_count > 0 {
        fs::write(cache_file, serde_json::to_string_pretty(&cache)?)?;
    }
    Ok(synced_count)
}
