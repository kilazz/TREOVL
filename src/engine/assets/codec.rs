use anyhow::{Context, Result, bail};
use std::fs;
use std::path::Path;

use crate::engine::assets::sniffer::AssetKind;
use crate::engine::common::Endian;

pub fn reencode_asset_to_chunk(
    kind: AssetKind,
    baseline_chunk: &[u8],
    asset_bytes: &[u8],
    endian: Endian,
    project_dir: Option<&Path>,
    rel_asset_path: Option<&str>,
) -> Result<Vec<u8>> {
    match kind {
        AssetKind::Projectile => {
            let json_str =
                std::str::from_utf8(asset_bytes).context("Projectile JSON is not valid UTF-8")?;
            crate::engine::assets::projectile::import_projectile_from_json(json_str, endian)
        }
        AssetKind::Character => {
            let json_str =
                std::str::from_utf8(asset_bytes).context("Character JSON is not valid UTF-8")?;
            crate::engine::assets::character::import_character_from_json_files(
                json_str,
                project_dir,
                endian,
            )
        }
        AssetKind::Attachment => {
            let json_str =
                std::str::from_utf8(asset_bytes).context("Attachment JSON is not valid UTF-8")?;
            crate::engine::assets::attachment::import_attachment_from_json_with_endian(
                json_str, endian,
            )
        }
        AssetKind::Texture => {
            crate::engine::assets::texture::replace_texture_in_chunk(baseline_chunk, asset_bytes)
        }
        AssetKind::Audio => crate::engine::assets::audio::replace_wav(baseline_chunk, asset_bytes),
        AssetKind::Material => {
            let json_str =
                std::str::from_utf8(asset_bytes).context("Material JSON is not valid UTF-8")?;
            crate::engine::assets::material::import_material_from_json_with_endian(json_str, endian)
        }
        AssetKind::Mesh => {
            let is_glb = rel_asset_path.is_some_and(|p| p.ends_with(".glb"))
                || asset_bytes.starts_with(b"glTF");
            if is_glb {
                crate::engine::assets::mesh::import_glb_to_mesh(baseline_chunk, asset_bytes)
            } else {
                let obj_str =
                    std::str::from_utf8(asset_bytes).context("OBJ file is not valid UTF-8")?;
                crate::engine::assets::mesh::import_obj_to_mesh(baseline_chunk, obj_str)
            }
        }
        AssetKind::Lua => {
            let bytecode = if rel_asset_path.is_some_and(|p| p.ends_with(".lua")) {
                if let Some(proj) = project_dir
                    && let Some(rel) = rel_asset_path
                {
                    let abs_path = proj.join(rel);
                    match crate::engine::assets::lua::compile_lua_script(&abs_path) {
                        Ok(compiled_bin) => compiled_bin,
                        Err(e) => {
                            eprintln!("[!] {}", e);
                            let luac_path = abs_path.with_extension("luac");
                            if luac_path.exists() {
                                fs::read(luac_path)?
                            } else {
                                bail!("Cannot sync Lua script: {}", e);
                            }
                        }
                    }
                } else {
                    asset_bytes.to_vec()
                }
            } else {
                asset_bytes.to_vec()
            };
            crate::engine::assets::lua::replace_lua_bytecode(baseline_chunk, &bytecode)
        }
        AssetKind::UI => {
            let json_str =
                std::str::from_utf8(asset_bytes).context("UI JSON is not valid UTF-8")?;
            crate::engine::assets::ui::import_ui_from_json(json_str)
        }
        AssetKind::Object => {
            let json_str =
                std::str::from_utf8(asset_bytes).context("Object JSON is not valid UTF-8")?;
            crate::engine::assets::object::import_object_from_json_with_endian(json_str, endian)
        }
        AssetKind::TerrainPalette => {
            let json_str = std::str::from_utf8(asset_bytes)
                .context("Terrain Palette JSON is not valid UTF-8")?;
            crate::engine::assets::terrain_palette::import_terrain_palette_from_json_with_endian(
                json_str, endian,
            )
        }
        AssetKind::Vfx => {
            let json_str =
                std::str::from_utf8(asset_bytes).context("VFX JSON is not valid UTF-8")?;
            crate::engine::assets::vfx::import_vfx_from_json_with_endian(json_str, endian)
        }
        AssetKind::Event => {
            let json_str =
                std::str::from_utf8(asset_bytes).context("Event JSON is not valid UTF-8")?;
            crate::engine::assets::event::import_event_from_json(json_str)
        }
        AssetKind::Collision => {
            let json_str =
                std::str::from_utf8(asset_bytes).context("Collision JSON is not valid UTF-8")?;
            crate::engine::assets::collision::import_collision_from_json(json_str, baseline_chunk)
        }
        AssetKind::Font => {
            let json_str =
                std::str::from_utf8(asset_bytes).context("Font JSON is not valid UTF-8")?;
            crate::engine::assets::font::import_font_from_json_with_endian(
                json_str,
                baseline_chunk,
                endian,
            )
        }
        AssetKind::FaceFx => {
            if let Some(proj) = project_dir
                && let Some(rel) = rel_asset_path
            {
                let fxe_path = proj.join(rel).with_extension("fxe");
                if fxe_path.exists() {
                    let fxe_data = fs::read(&fxe_path)?;
                    if let Some(pos) = baseline_chunk.windows(4).position(|w| w == b"FACE") {
                        let mut new_chunk = baseline_chunk[..pos].to_vec();
                        new_chunk.extend_from_slice(&fxe_data);
                        return Ok(new_chunk);
                    }
                }
            }
            Ok(baseline_chunk.to_vec())
        }
        AssetKind::Animation => Ok(baseline_chunk.to_vec()),
        AssetKind::Dta => {
            let json_str =
                std::str::from_utf8(asset_bytes).context("DTA JSON is not valid UTF-8")?;
            crate::engine::assets::dta::import_dta_from_json(json_str, baseline_chunk)
        }
        AssetKind::VoicePackage => {
            let json_str = std::str::from_utf8(asset_bytes)
                .context("Voice Package JSON is not valid UTF-8")?;
            crate::engine::assets::vpk::import_vpk_from_json(json_str, baseline_chunk)
        }
        AssetKind::M8ldMap => {
            let xml_str =
                std::str::from_utf8(asset_bytes).context("M8LD XML file is not valid UTF-8")?;
            let crc = if baseline_chunk.len() >= 8 && baseline_chunk.starts_with(b"M8LD") {
                u32::from_le_bytes(baseline_chunk[4..8].try_into().unwrap_or_default())
            } else {
                0x3707714B
            };
            Ok(crate::engine::assets::m8ld::compile_xml_to_8ld(
                xml_str, crc,
            ))
        }
        AssetKind::UiSprite => {
            let json_str =
                std::str::from_utf8(asset_bytes).context("UiSprite JSON is not valid UTF-8")?;
            if baseline_chunk.starts_with(b"CPTX") {
                crate::engine::assets::cptx::import_cptx_from_json_with_endian(
                    json_str,
                    baseline_chunk,
                    endian,
                )
            } else {
                crate::engine::assets::ui_sprite::import_ui_sprite_collection_with_endian(
                    json_str, endian,
                )
            }
        }
        AssetKind::Environment => {
            let json_str =
                std::str::from_utf8(asset_bytes).context("Environment JSON is not valid UTF-8")?;
            crate::engine::assets::environment::import_environment_from_json(
                json_str,
                baseline_chunk,
            )
        }
        AssetKind::Parameter => crate::engine::assets::parameter::import_parameter_from_json(
            asset_bytes,
            baseline_chunk,
        ),
        AssetKind::Xml => {
            crate::engine::assets::xml::import_xml_payload(baseline_chunk, asset_bytes)
        }
        AssetKind::Generic | AssetKind::Behavior => Ok(asset_bytes.to_vec()),
    }
}
