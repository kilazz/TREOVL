use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use crc32fast::Hasher;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use crate::engine::assets::animation::{export_skeleton_to_glb, parse_object_bone_container};
use crate::engine::assets::sniffer::{AssetKind, SniffedAsset, sniff_asset};
use crate::engine::assets::{parse_chunk_elements, parse_typed_container};
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

// -------------------------------------------------------------
// TEXTURE PROCESSOR
// -------------------------------------------------------------
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

// -------------------------------------------------------------
// AUDIO PROCESSOR
// -------------------------------------------------------------
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

// -------------------------------------------------------------
// LUA SCRIPT PROCESSOR
// -------------------------------------------------------------
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
        let bytecode = crate::engine::assets::lua::extract_lua_bytecode(data)?;
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

// -------------------------------------------------------------
// MATERIAL PROCESSOR
// -------------------------------------------------------------
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
        let json_str = crate::engine::assets::material::export_material_to_json(data)?;
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

// -------------------------------------------------------------
// MESH PROCESSOR (WITH AUTO-RIG EXTRACTION)
// -------------------------------------------------------------
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
        let (glb_bytes, _) = crate::engine::assets::mesh::export_mesh_to_glb(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "glb");

        fs::write(
            workspace.assets_dir.join("meshes").join(&out_name),
            &glb_bytes,
        )?;

        // Automatically exports the armature rig for Blender if bone data is present
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

// -------------------------------------------------------------
// XML PROCESSOR
// -------------------------------------------------------------
pub struct XmlProcessor;
impl AssetProcessor for XmlProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if !data.windows(5).any(|w| w == b"<?xml") {
            return Ok(None);
        }

        let out_name = build_asset_filename(&sniffed.display_name, stem, "xml");
        let abs_path = workspace.assets_dir.join("xml").join(&out_name);

        let xml_payload = if data.len() > 4 {
            let mut cur = Cursor::new(&data[0..4]);
            let str_len = cur.read_u32::<LittleEndian>().unwrap_or(0) as usize;
            if str_len + 4 <= data.len() {
                &data[4..4 + str_len]
            } else {
                data
            }
        } else {
            data
        };

        fs::write(&abs_path, xml_payload)?;

        Ok(Some((
            format!("assets/xml/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Xml".into(),
                crc32: calculate_crc32(xml_payload),
            },
        )))
    }
}

// -------------------------------------------------------------
// PARAMETER / SCALAR PROCESSOR
// -------------------------------------------------------------
pub struct ParameterProcessor;
impl AssetProcessor for ParameterProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Parameter && data.len() > 64 {
            return Ok(None);
        }

        let out_name = format!("{}.json", stem);
        let abs_path = workspace.assets_dir.join("parameters").join(&out_name);

        // A. Named Asset Container Slot (e.g. "17039")
        let param_json = if data.len() == 24 && data.starts_with(&[3, 20, 0, 21]) {
            let slen = u32::from_le_bytes(data[7..11].try_into().unwrap_or_default()) as usize;
            if slen <= 13
                && let Ok(slot_id) = std::str::from_utf8(&data[11..11 + slen])
            {
                json!({
                    "type": "asset_group_slot",
                    "slot_id": slot_id.trim_matches(char::from(0)),
                })
            } else {
                json!({ "type": "raw_bytes", "hex": hex::encode_upper(data) })
            }
        // B. Sound Bank Descriptor (57 00 00 04)
        } else if data.starts_with(b"\x57\x00\x00\x04") && data.len() >= 14 {
            let mut sfx_label = String::new();
            if let Ok((_, sub_elem)) = parse_chunk_elements(&data[4..]) {
                for (sid, sval) in sub_elem {
                    if sid == 10 && sval.len() >= 4 {
                        let slen =
                            u32::from_le_bytes(sval[0..4].try_into().unwrap_or_default()) as usize;
                        if slen <= sval.len() - 4 {
                            sfx_label = String::from_utf8_lossy(&sval[4..4 + slen])
                                .trim_matches(char::from(0))
                                .to_string();
                        }
                    }
                }
            }
            json!({
                "type": "sound_bank_descriptor",
                "label": sfx_label
            })
        // C. String Parameters & 32-bit Scalars (<= 64 bytes)
        } else if data.len() == 4 {
            let val_u32 = u32::from_le_bytes(data[0..4].try_into().unwrap_or_default());
            let val_f32 = f32::from_le_bytes(data[0..4].try_into().unwrap_or_default());
            json!({
                "type": "scalar_32bit",
                "uint_value": val_u32,
                "float_value": if val_f32.is_finite() { val_f32 } else { 0.0 },
                "hex": hex::encode_upper(data)
            })
        } else {
            let string_candidate = if data.len() >= 4 {
                let slen = u32::from_le_bytes(data[0..4].try_into().unwrap_or_default()) as usize;
                if (slen == data.len() - 4 || slen == data.len() - 5)
                    && data[4..4 + slen]
                        .iter()
                        .all(|&b| (0x20..=0x7E).contains(&b) || b == 0)
                {
                    std::str::from_utf8(&data[4..4 + slen])
                        .ok()
                        .map(|s| s.trim_matches(char::from(0)).to_string())
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(text) = string_candidate {
                json!({ "type": "string", "value": text })
            } else {
                json!({ "type": "raw_bytes", "size": data.len(), "hex": hex::encode_upper(data) })
            }
        };

        let json_str = serde_json::to_string_pretty(&param_json)?;
        fs::write(&abs_path, json_str.as_bytes())?;

        Ok(Some((
            format!("assets/parameters/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Parameter".into(),
                crc32: calculate_crc32(json_str.as_bytes()),
            },
        )))
    }
}

// -------------------------------------------------------------
// REVERSE ENGINEERING DOSSIER GENERATOR FOR RAW CHUNKS
// -------------------------------------------------------------
fn build_unknown_chunk_dossier(chunk_data: &[u8], stem: &str) -> serde_json::Value {
    let magic_hex = if chunk_data.len() >= 4 {
        hex::encode_upper(&chunk_data[..4])
    } else {
        String::from("TOO_SHORT")
    };

    let mut extracted_strings = Vec::new();
    let mut i = 0;
    while i < chunk_data.len() {
        if i + 4 <= chunk_data.len() {
            let str_len =
                u32::from_le_bytes(chunk_data[i..i + 4].try_into().unwrap_or_default()) as usize;
            if (3..=128).contains(&str_len)
                && i + 4 + str_len <= chunk_data.len()
                && let slice = &chunk_data[i + 4..i + 4 + str_len]
                && slice.iter().all(|&b| (0x20..=0x7E).contains(&b) || b == 0)
                && let Ok(s) = std::str::from_utf8(slice)
            {
                let clean = s.trim_matches(char::from(0)).trim();
                if clean.len() >= 3 && !extracted_strings.contains(&clean.to_string()) {
                    extracted_strings.push(clean.to_string());
                }
            }
        }
        i += 1;
    }

    if let Ok((has_magic, elements)) = parse_chunk_elements(chunk_data) {
        let mut sub_elements_info = Vec::new();
        for (sub_id, sub_data) in elements {
            sub_elements_info.push(json!({
                "sub_id": sub_id,
                "sub_id_hex": format!("0x{:X}", sub_id),
                "size_bytes": sub_data.len(),
                "hex_preview": hex::encode_upper(&sub_data[..sub_data.len().min(32)])
            }));
        }
        if !sub_elements_info.is_empty() {
            return json!({
                "chunk": stem,
                "structure_type": "Untyped Container Sub-Table",
                "has_container_magic": has_magic,
                "total_bytes": chunk_data.len(),
                "sub_elements_count": sub_elements_info.len(),
                "sub_elements": sub_elements_info,
                "embedded_strings": extracted_strings,
                "full_hex": hex::encode_upper(chunk_data)
            });
        }
    }

    if let Ok((type_id, elements)) = parse_typed_container(chunk_data) {
        let mut sub_elements_info = Vec::new();
        for (sub_id, sub_data) in elements {
            sub_elements_info.push(json!({
                "sub_id": sub_id,
                "size_bytes": sub_data.len(),
                "hex_preview": hex::encode_upper(&sub_data[..sub_data.len().min(32)])
            }));
        }
        return json!({
            "chunk": stem,
            "structure_type": "Typed Container",
            "type_id_hex": format!("{:08X}", type_id),
            "total_bytes": chunk_data.len(),
            "sub_elements_count": sub_elements_info.len(),
            "sub_elements": sub_elements_info,
            "embedded_strings": extracted_strings,
            "full_hex": hex::encode_upper(chunk_data)
        });
    }

    let scalar_guesses = if chunk_data.len() == 4 {
        let u = u32::from_le_bytes(chunk_data[0..4].try_into().unwrap_or_default());
        let i = i32::from_le_bytes(chunk_data[0..4].try_into().unwrap_or_default());
        let f = f32::from_le_bytes(chunk_data[0..4].try_into().unwrap_or_default());
        json!({
            "as_u32": u,
            "as_i32": i,
            "as_f32": if f.is_finite() { f } else { 0.0 }
        })
    } else {
        json!(null)
    };

    json!({
        "chunk": stem,
        "structure_type": "Raw Leaf Data",
        "total_bytes": chunk_data.len(),
        "magic_header_hex": magic_hex,
        "scalar_guesses": scalar_guesses,
        "embedded_strings": extracted_strings,
        "full_hex": hex::encode_upper(chunk_data)
    })
}

// -------------------------------------------------------------
// RAW / FALLBACK PROCESSOR (CREATES .BIN + .META.JSON DOSSIER)
// -------------------------------------------------------------
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
        let bin_path = workspace.assets_dir.join("raw_chunks").join(&out_name);
        fs::write(&bin_path, data)?;

        // Generates the comprehensive reverse-engineering dossier alongside each binary chunk
        let dossier = build_unknown_chunk_dossier(data, stem);
        let meta_path = workspace
            .assets_dir
            .join("raw_chunks")
            .join(format!("{}.meta.json", stem));
        let meta_json_str = serde_json::to_string_pretty(&dossier)?;
        fs::write(meta_path, meta_json_str)?;

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

// -------------------------------------------------------------
// PIPELINE ORCHESTRATOR
// -------------------------------------------------------------
pub fn export_smart_assets(project_dir: &Path) -> Result<usize> {
    let workspace = ProjectWorkspace::new(project_dir);

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
        "xml",
        "parameters",
    ];
    for dir in dirs {
        fs::create_dir_all(workspace.assets_dir.join(dir))?;
    }

    // Strategy Pipeline
    let processors: Vec<Box<dyn AssetProcessor>> = vec![
        Box::new(TextureProcessor),
        Box::new(AudioProcessor),
        Box::new(MaterialProcessor),
        Box::new(MeshProcessor),
        Box::new(LuaProcessor),
        Box::new(XmlProcessor),
        Box::new(ParameterProcessor),
        Box::new(RawProcessor), // Fallback: writes .bin + .meta.json
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

// -------------------------------------------------------------
// TWO-WAY MOD SYNC BACK TO GAME CHUNKS
// -------------------------------------------------------------
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
                    crate::engine::assets::material::import_material_from_json(&json_str)?
                }
                "Mesh" => {
                    if rel_asset_path.ends_with(".glb") {
                        crate::engine::assets::mesh::import_glb_to_mesh(&chunk_bytes, &asset_bytes)?
                    } else {
                        let obj_str = String::from_utf8(asset_bytes)
                            .context("OBJ file is not valid UTF-8")?;
                        crate::engine::assets::mesh::import_obj_to_mesh(&chunk_bytes, &obj_str)?
                    }
                }
                "Lua" => {
                    crate::engine::assets::lua::replace_lua_bytecode(&chunk_bytes, &asset_bytes)?
                }
                "Parameter" => {
                    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&asset_bytes) {
                        if v["type"] == "asset_group_slot"
                            && let Some(slot_id) = v["slot_id"].as_str()
                        {
                            let s_bytes = slot_id.as_bytes();
                            let mut buf = Vec::with_capacity(7 + 4 + s_bytes.len() + 8);
                            buf.push(3);
                            buf.extend_from_slice(&[20, 0]);
                            buf.extend_from_slice(&[21, (4 + s_bytes.len()) as u8]);
                            buf.extend_from_slice(&[30, (4 + s_bytes.len() + 4) as u8]);
                            buf.extend_from_slice(&(s_bytes.len() as u32).to_le_bytes());
                            buf.extend_from_slice(s_bytes);
                            buf.extend_from_slice(&[1, 1, 0, 0]);
                            buf.extend_from_slice(&[1, 1, 0, 0]);
                            buf
                        } else if v["type"] == "string"
                            && let Some(s) = v["value"].as_str()
                        {
                            let mut buf = Vec::new();
                            let s_bytes = s.as_bytes();
                            buf.extend_from_slice(&(s_bytes.len() as u32).to_le_bytes());
                            buf.extend_from_slice(s_bytes);
                            buf
                        } else if v["type"] == "scalar_32bit"
                            && let Some(hex_str) = v["hex"].as_str()
                        {
                            hex::decode(hex_str).unwrap_or(chunk_bytes)
                        } else if let Some(hex_str) = v["hex"].as_str() {
                            hex::decode(hex_str).unwrap_or(chunk_bytes)
                        } else {
                            chunk_bytes
                        }
                    } else {
                        chunk_bytes
                    }
                }
                "Xml" => {
                    if chunk_bytes.len() > 4 {
                        let mut buf = Vec::new();
                        buf.extend_from_slice(&(asset_bytes.len() as u32).to_le_bytes());
                        buf.extend_from_slice(&asset_bytes);
                        buf
                    } else {
                        asset_bytes
                    }
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
