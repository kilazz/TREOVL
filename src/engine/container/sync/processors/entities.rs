use anyhow::Result;
use std::fs;

use super::super::cache::{AssetSyncEntry, build_asset_filename, calculate_crc32};
use super::{AssetProcessor, ProjectWorkspace};
use crate::engine::assets::character::export_character;
use crate::engine::assets::object::export_object;
use crate::engine::assets::projectile::export_projectile_to_json;
use crate::engine::assets::sniffer::{AssetKind, SniffedAsset};

pub struct CharacterProcessor;
impl AssetProcessor for CharacterProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Character {
            return Ok(None);
        }

        let char_dir = workspace.assets_dir.join("characters");
        fs::create_dir_all(&char_dir)?;

        let mut extracted = export_character(data, stem)?;

        if let Some(ref lua_source) = extracted.embedded_lua_source {
            let scripts_dir = workspace.assets_dir.join("scripts");
            fs::create_dir_all(&scripts_dir)?;
            let file_base = format!("{}_logic", stem);
            let lua_path = scripts_dir.join(format!("{}.lua", file_base));
            let _ = fs::write(&lua_path, lua_source.as_bytes());

            if !extracted.embedded_lua_bytecode.is_empty() {
                let luac_path = scripts_dir.join(format!("{}.luac", file_base));
                let _ = fs::write(luac_path, &extracted.embedded_lua_bytecode);
            }
            extracted.character.lua_script_file = Some(format!("assets/scripts/{}.lua", file_base));
        }

        if let Some(ref fxe_bytes) = extracted.embedded_facefx {
            let face_dir = workspace.assets_dir.join("facefx");
            fs::create_dir_all(&face_dir)?;
            let fxe_name = format!("{}_face.fxe", stem);
            let fxe_path = face_dir.join(&fxe_name);
            let _ = fs::write(fxe_path, fxe_bytes);
            extracted.character.embedded_facefx_file = Some(format!("assets/facefx/{}", fxe_name));
        }

        let json_str = serde_json::to_string_pretty(&extracted.character)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");
        fs::write(char_dir.join(&out_name), json_str.as_bytes())?;

        Ok(Some((
            format!("assets/characters/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::Character,
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct AttachmentProcessor;
impl AssetProcessor for AttachmentProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Attachment {
            return Ok(None);
        }

        let attach_dir = workspace.assets_dir.join("attachments");
        fs::create_dir_all(&attach_dir)?;

        let json_str = crate::engine::assets::attachment::export_attachment_to_json(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");
        fs::write(attach_dir.join(&out_name), json_str.as_bytes())?;

        Ok(Some((
            format!("assets/attachments/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::Attachment,
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct ProjectileProcessor;
impl AssetProcessor for ProjectileProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Projectile {
            return Ok(None);
        }

        let proj_dir = workspace.assets_dir.join("projectiles");
        fs::create_dir_all(&proj_dir)?;

        let json_str = export_projectile_to_json(data, stem)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");
        fs::write(proj_dir.join(&out_name), json_str.as_bytes())?;

        Ok(Some((
            format!("assets/projectiles/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::Projectile,
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

        let obj_dir = workspace.assets_dir.join("objects");
        fs::create_dir_all(&obj_dir)?;

        let extracted = export_object(data)?;

        if !extracted.bones.is_empty() {
            let rig_name = extracted
                .entity
                .entity_name
                .clone()
                .unwrap_or_else(|| "Unknown_Rig".to_string());
            if let Ok(glb_bytes) = crate::engine::assets::object::export_skeleton_from_json(
                &extracted.bones,
                &rig_name,
            ) {
                let glb_path = obj_dir.join(format!("{}_MASTER_RIG.glb", rig_name));
                let _ = fs::write(glb_path, glb_bytes);
            }
        }

        let json_str = serde_json::to_string_pretty(&extracted.entity)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");

        fs::write(obj_dir.join(&out_name), json_str.as_bytes())?;
        Ok(Some((
            format!("assets/objects/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::Object,
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}
