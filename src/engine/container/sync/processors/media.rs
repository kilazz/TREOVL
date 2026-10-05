use anyhow::Result;
use std::fs;

use super::super::cache::{AssetSyncEntry, build_asset_filename, calculate_crc32};
use super::{AssetProcessor, ProjectWorkspace};
use crate::engine::assets::animation::{
    export_animation_to_glb, export_animation_to_json, export_skeleton_to_glb,
    parse_object_bone_container,
};
use crate::engine::assets::sniffer::{AssetKind, SniffedAsset};
use crate::engine::common::magic;

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

        let tex_dir = workspace.assets_dir.join("textures");
        fs::create_dir_all(&tex_dir)?;

        let dds_or_tga = crate::engine::assets::texture::export_to_dds(data)?;
        let ext = if data.starts_with(magic::TEX_INTERFACE) {
            "tga"
        } else {
            "dds"
        };
        let out_name = build_asset_filename(&sniffed.display_name, stem, ext);

        fs::write(tex_dir.join(&out_name), &dds_or_tga)?;
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

        let aud_dir = workspace.assets_dir.join("audio");
        fs::create_dir_all(&aud_dir)?;

        let wav_bytes = crate::engine::assets::audio::export_wav(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "wav");

        fs::write(aud_dir.join(&out_name), &wav_bytes)?;
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

        let mat_dir = workspace.assets_dir.join("materials");
        fs::create_dir_all(&mat_dir)?;

        let json_str = crate::engine::assets::material::export_material_to_json(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");

        fs::write(mat_dir.join(&out_name), json_str.as_bytes())?;
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

        let mesh_dir = workspace.assets_dir.join("meshes");
        fs::create_dir_all(&mesh_dir)?;

        let (glb_bytes, stats) = crate::engine::assets::mesh::export_mesh_to_glb(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "glb");

        fs::write(mesh_dir.join(&out_name), &glb_bytes)?;

        if stats.is_skinned
            && let Ok(bones) = parse_object_bone_container(data)
            && !bones.is_empty()
            && let Ok(rig_glb) = export_skeleton_to_glb(&bones, &sniffed.display_name)
        {
            let rig_name = build_asset_filename(&sniffed.display_name, stem, "rig.glb");
            let _ = fs::write(mesh_dir.join(&rig_name), rig_glb);
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

        let anim_dir = workspace.assets_dir.join("animations");
        fs::create_dir_all(&anim_dir)?;

        let glb_bytes = export_animation_to_glb(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "glb");
        let glb_path = anim_dir.join(&out_name);
        fs::write(&glb_path, &glb_bytes)?;

        if let Ok(json_str) = export_animation_to_json(data) {
            let json_name = build_asset_filename(&sniffed.display_name, stem, "json");
            let _ = fs::write(anim_dir.join(json_name), json_str.as_bytes());
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

pub struct FontProcessor;
impl AssetProcessor for FontProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Font && !data.starts_with(b"\x72\x00\x41\x00") {
            return Ok(None);
        }

        let font_dir = workspace.assets_dir.join("fonts");
        fs::create_dir_all(&font_dir)?;

        let json_str = crate::engine::assets::font::export_font_to_json(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");

        fs::write(font_dir.join(&out_name), json_str.as_bytes())?;

        Ok(Some((
            format!("assets/fonts/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: "Font".into(),
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}
