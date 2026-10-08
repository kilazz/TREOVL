use anyhow::Result;
use std::fs;

use super::super::cache::{AssetSyncEntry, build_asset_filename, calculate_crc32};
use super::{AssetProcessor, ProjectWorkspace};
use crate::engine::assets::sniffer::{AssetKind, SniffedAsset};

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

        let tp_dir = workspace.assets_dir.join("terrain_palettes");
        fs::create_dir_all(&tp_dir)?;

        let json_str =
            crate::engine::assets::terrain_palette::export_terrain_palette_to_json(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");

        fs::write(tp_dir.join(&out_name), json_str.as_bytes())?;

        Ok(Some((
            format!("assets/terrain_palettes/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::TerrainPalette,
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct CollisionProcessor;
impl AssetProcessor for CollisionProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Collision {
            return Ok(None);
        }

        let col_dir = workspace.assets_dir.join("collisions");
        fs::create_dir_all(&col_dir)?;

        let json_str = crate::engine::assets::collision::export_collision_to_json(data, stem)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");

        fs::write(col_dir.join(&out_name), json_str.as_bytes())?;

        Ok(Some((
            format!("assets/collisions/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::Collision,
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct DtaProcessor;
impl AssetProcessor for DtaProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Dta {
            return Ok(None);
        }

        let dta_dir = workspace.assets_dir.join("lightsets");
        fs::create_dir_all(&dta_dir)?;

        let json_str = crate::engine::assets::dta::export_dta_to_json(data, stem)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");

        fs::write(dta_dir.join(&out_name), json_str.as_bytes())?;

        Ok(Some((
            format!("assets/lightsets/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::Dta,
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct VoicePackageProcessor;
impl AssetProcessor for VoicePackageProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::VoicePackage {
            return Ok(None);
        }

        let vpk_dir = workspace.assets_dir.join("voice_packages");
        fs::create_dir_all(&vpk_dir)?;

        let json_str = crate::engine::assets::vpk::export_vpk_to_json(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");

        fs::write(vpk_dir.join(&out_name), json_str.as_bytes())?;

        Ok(Some((
            format!("assets/voice_packages/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::VoicePackage,
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct EnvironmentProcessor;
impl AssetProcessor for EnvironmentProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Environment && !data.starts_with(b"\x83\x00\x00\x04") {
            return Ok(None);
        }

        let env_dir = workspace.assets_dir.join("environments");
        fs::create_dir_all(&env_dir)?;

        let json_str = crate::engine::assets::environment::export_environment_to_json(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");

        fs::write(env_dir.join(&out_name), json_str.as_bytes())?;

        Ok(Some((
            format!("assets/environments/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::Environment,
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}
