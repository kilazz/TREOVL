use anyhow::Result;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

use super::cache::{AssetSyncEntry, build_asset_filename, calculate_crc32};
use crate::engine::assets::animation::{
    export_animation_to_glb, export_animation_to_json, export_skeleton_to_glb,
    parse_object_bone_container,
};
use crate::engine::assets::parse_chunk_elements;
use crate::engine::assets::sniffer::{AssetKind, SniffedAsset};
use crate::engine::common::magic;

pub struct ProjectWorkspace<'a> {
    pub base_dir: &'a Path,
    pub chunks_dir: PathBuf,
    pub vanilla_chunks_dir: PathBuf,
    pub assets_dir: PathBuf,
}

impl<'a> ProjectWorkspace<'a> {
    pub fn new(base_dir: &'a Path) -> Self {
        Self {
            base_dir,
            chunks_dir: base_dir.join("chunks"),
            vanilla_chunks_dir: base_dir.join("chunks_vanilla"),
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
                vanilla_crc32: calculate_crc32(&dds_or_tga),
                is_modified: false,
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
                vanilla_crc32: calculate_crc32(&wav_bytes),
                is_modified: false,
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
        let bytecode = crate::engine::assets::lua::extract_lua_bytecode(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "luac");

        fs::write(
            workspace.assets_dir.join("scripts").join(&out_name),
            &bytecode,
        )?;

        if let Ok(disasm) = crate::engine::assets::lua::disassemble_lua_bytecode(&bytecode) {
            let _ = fs::write(
                workspace
                    .assets_dir
                    .join("scripts")
                    .join(format!("{}.lua.txt", out_name)),
                disasm,
            );
        }

        if let Ok(decompiled) = crate::engine::assets::lua::decompile_lua_bytecode(&bytecode) {
            let _ = fs::write(
                workspace
                    .assets_dir
                    .join("scripts")
                    .join(format!("{}.decompiled.lua", out_name)),
                decompiled,
            );
        }

        Ok(Some((
            format!("assets/scripts/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Lua".into(),
                vanilla_crc32: calculate_crc32(&bytecode),
                is_modified: false,
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
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
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
        let (glb_bytes, stats) = crate::engine::assets::mesh::export_mesh_to_glb(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "glb");

        fs::write(
            workspace.assets_dir.join("meshes").join(&out_name),
            &glb_bytes,
        )?;

        if stats.is_skinned
            && let Ok(bones) = parse_object_bone_container(data)
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
                vanilla_crc32: calculate_crc32(&glb_bytes),
                is_modified: false,
            },
        )))
    }
}

pub struct AnimationProcessor;
impl AssetProcessor for AnimationProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Animation {
            return Ok(None);
        }

        let glb_bytes = export_animation_to_glb(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "glb");
        let glb_path = workspace.assets_dir.join("animations").join(&out_name);
        fs::write(&glb_path, &glb_bytes)?;

        if let Ok(json_str) = export_animation_to_json(data) {
            let json_name = build_asset_filename(&sniffed.display_name, stem, "json");
            let _ = fs::write(
                workspace.assets_dir.join("animations").join(json_name),
                json_str.as_bytes(),
            );
        }

        Ok(Some((
            format!("assets/animations/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Animation".into(),
                vanilla_crc32: calculate_crc32(&glb_bytes),
                is_modified: false,
            },
        )))
    }
}

pub struct TerrainPaletteProcessor;
impl AssetProcessor for TerrainPaletteProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::TerrainPalette && !data.starts_with(b"\x7E\x00\x00\x04") {
            return Ok(None);
        }

        let json_str =
            crate::engine::assets::terrain_palette::export_terrain_palette_to_json(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");

        fs::write(
            workspace
                .assets_dir
                .join("terrain_palettes")
                .join(&out_name),
            json_str.as_bytes(),
        )?;

        Ok(Some((
            format!("assets/terrain_palettes/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "TerrainPalette".into(),
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct VfxProcessor;
impl AssetProcessor for VfxProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Vfx {
            return Ok(None);
        }

        let json_str = crate::engine::assets::vfx::export_vfx_to_json(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");

        fs::write(
            workspace.assets_dir.join("vfx").join(&out_name),
            json_str.as_bytes(),
        )?;
        Ok(Some((
            format!("assets/vfx/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Vfx".into(),
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct EventProcessor;
impl AssetProcessor for EventProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Event
            && !data.starts_with(b"\xB0\x00\x00\x04")
            && !data.starts_with(b"\x83\x00\x00\x04")
        {
            return Ok(None);
        }

        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");
        let abs_path = workspace.assets_dir.join("events").join(&out_name);

        let json_val =
            if let Ok((type_id, elements)) = crate::engine::assets::parse_typed_container(data) {
                let mut props = Vec::new();
                let mut group_path = None;
                let mut event_name = None;

                for (id, chunk) in elements {
                    if id == 20 && chunk.len() >= 4 {
                        let slen =
                            u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default()) as usize;
                        if slen + 4 <= chunk.len() {
                            group_path = std::str::from_utf8(&chunk[4..4 + slen])
                                .ok()
                                .map(|s| s.trim_matches(char::from(0)).to_string());
                        }
                    } else if id == 21 && chunk.len() >= 4 {
                        let slen =
                            u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default()) as usize;
                        if slen + 4 <= chunk.len() {
                            event_name = std::str::from_utf8(&chunk[4..4 + slen])
                                .ok()
                                .map(|s| s.trim_matches(char::from(0)).to_string());
                        }
                    }

                    props.push(json!({
                        "id": id,
                        "size": chunk.len(),
                        "hex": hex::encode_upper(&chunk)
                    }));
                }

                json!({
                    "type": "event_table",
                    "type_id_hex": format!("{:08X}", type_id),
                    "group_path": group_path,
                    "event_name": event_name,
                    "properties": props
                })
            } else {
                json!({
                    "type": "raw_event",
                    "hex": hex::encode_upper(data)
                })
            };

        let json_str = serde_json::to_string_pretty(&json_val)?;
        fs::write(&abs_path, json_str.as_bytes())?;

        Ok(Some((
            format!("assets/events/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Event".into(),
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct XmlProcessor;
impl AssetProcessor for XmlProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Xml {
            return Ok(None);
        }

        let out_name = build_asset_filename(&sniffed.display_name, stem, "xml");
        let abs_path = workspace.assets_dir.join("xml").join(&out_name);

        let xml_payload = if let Some(pos) = data.windows(5).position(|w| w == b"<?xml") {
            &data[pos..]
        } else {
            data
        };

        fs::write(&abs_path, xml_payload)?;

        Ok(Some((
            format!("assets/xml/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Xml".into(),
                vanilla_crc32: calculate_crc32(xml_payload),
                is_modified: false,
            },
        )))
    }
}

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
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct UiProcessor;
impl AssetProcessor for UiProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::UI {
            return Ok(None);
        }

        let json_str = crate::engine::assets::ui::export_ui_to_json(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");

        fs::write(
            workspace.assets_dir.join("ui").join(&out_name),
            json_str.as_bytes(),
        )?;
        Ok(Some((
            format!("assets/ui/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "UI".into(),
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct ObjectProcessor;
impl AssetProcessor for ObjectProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Object {
            return Ok(None);
        }

        let json_str = crate::engine::assets::object::export_object_to_json(
            data,
            Some(&workspace.assets_dir.join("objects")),
        )?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");

        fs::write(
            workspace.assets_dir.join("objects").join(&out_name),
            json_str.as_bytes(),
        )?;
        Ok(Some((
            format!("assets/objects/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Object".into(),
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
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
        let bin_path = workspace.assets_dir.join("raw_chunks").join(&out_name);
        fs::write(&bin_path, data)?;

        Ok(Some((
            format!("assets/raw_chunks/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Raw".into(),
                vanilla_crc32: calculate_crc32(data),
                is_modified: false,
            },
        )))
    }
}

pub fn get_standard_processors() -> Vec<Box<dyn AssetProcessor>> {
    vec![
        Box::new(TextureProcessor),
        Box::new(AudioProcessor),
        Box::new(MaterialProcessor),
        Box::new(MeshProcessor),
        Box::new(AnimationProcessor),
        Box::new(TerrainPaletteProcessor),
        Box::new(VfxProcessor),
        Box::new(EventProcessor),
        Box::new(LuaProcessor),
        Box::new(XmlProcessor),
        Box::new(ParameterProcessor),
        Box::new(UiProcessor),
        Box::new(ObjectProcessor),
    ]
}
