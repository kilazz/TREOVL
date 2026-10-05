use anyhow::Result;
use std::fs;

use super::super::cache::{AssetSyncEntry, build_asset_filename, calculate_crc32};
use super::{AssetProcessor, ProjectWorkspace};
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

        let json_str = crate::engine::assets::character::export_character_to_json(
            data,
            Some(&workspace.assets_dir),
            stem,
        )?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");
        fs::write(char_dir.join(&out_name), json_str.as_bytes())?;

        Ok(Some((
            format!("assets/characters/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Character".into(),
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
                asset_kind: "Attachment".into(),
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

        let json_str = crate::engine::assets::object::export_object_to_json(data, Some(&obj_dir))?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");

        fs::write(obj_dir.join(&out_name), json_str.as_bytes())?;
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
