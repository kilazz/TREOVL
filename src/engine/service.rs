use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::engine::assets::animation::{export_animation_to_glb, export_animation_to_json};
use crate::engine::assets::audio::{export_wav, replace_wav};
use crate::engine::assets::collision::{
    export_collision_to_glb, import_collision_from_glb, import_collision_from_json,
};
use crate::engine::assets::dta::{export_dta_to_json, import_dta_from_json};
use crate::engine::assets::environment::import_environment_from_json;
use crate::engine::assets::font::import_font_from_json;
use crate::engine::assets::lua::{compile_lua_script, extract_lua_bytecode, replace_lua_bytecode};
use crate::engine::assets::m8ld::{
    M8LD_MAGIC, M8ldMetaJson, compile_xml_to_8ld, decompile_8ld_to_xml, import_m8ld_from_json,
};
use crate::engine::assets::material::import_material_from_json;
use crate::engine::assets::mesh::{
    MeshStats, export_mesh_to_glb, export_mesh_to_obj, import_glb_to_mesh, import_obj_to_mesh,
};
use crate::engine::assets::object::import_object_from_json;
use crate::engine::assets::terrain::{export_terrain_to_glb, export_terrain_to_obj};
use crate::engine::assets::terrain_palette::import_terrain_palette_from_json;
use crate::engine::assets::texture::{export_to_dds, replace_texture_in_chunk};
use crate::engine::assets::ui::import_ui_from_json;
use crate::engine::assets::ui_sprite::import_ui_sprite_collection;
use crate::engine::assets::vpk::{export_vpk_to_json, import_vpk_from_json};

pub fn export_texture(chunk_path: &Path, out_path: &Path) -> Result<()> {
    let data = fs::read(chunk_path).with_context(|| format!("Failed to read {:?}", chunk_path))?;
    let dds = export_to_dds(&data)?;
    fs::write(out_path, dds)?;
    Ok(())
}

pub fn import_texture(chunk_path: &Path, in_path: &Path) -> Result<()> {
    let chunk_data = fs::read(chunk_path)?;
    let dds_data = fs::read(in_path)?;
    let new_chunk = replace_texture_in_chunk(&chunk_data, &dds_data)?;
    fs::write(chunk_path, new_chunk)?;
    Ok(())
}

pub fn export_audio(chunk_path: &Path, out_path: &Path) -> Result<()> {
    let data = fs::read(chunk_path)?;
    let wav = export_wav(&data)?;
    fs::write(out_path, wav)?;
    Ok(())
}

pub fn import_audio(chunk_path: &Path, in_path: &Path) -> Result<()> {
    let chunk_data = fs::read(chunk_path)?;
    let wav_data = fs::read(in_path)?;
    let new_chunk = replace_wav(&chunk_data, &wav_data)?;
    fs::write(chunk_path, new_chunk)?;
    Ok(())
}

pub fn export_mesh(chunk_path: &Path, out_path: &Path, is_glb: bool) -> Result<MeshStats> {
    let data = fs::read(chunk_path)?;
    if is_glb {
        let (glb, stats) = export_mesh_to_glb(&data)?;
        fs::write(out_path, glb)?;
        Ok(stats)
    } else {
        let (obj, stats) = export_mesh_to_obj(&data)?;
        fs::write(out_path, obj)?;
        Ok(stats)
    }
}

pub fn import_mesh(chunk_path: &Path, in_path: &Path, is_glb: bool) -> Result<()> {
    let chunk_data = fs::read(chunk_path)?;
    let new_chunk = if is_glb {
        let glb_bytes = fs::read(in_path)?;
        import_glb_to_mesh(&chunk_data, &glb_bytes)?
    } else {
        let obj_text = fs::read_to_string(in_path)?;
        import_obj_to_mesh(&chunk_data, &obj_text)?
    };
    fs::write(chunk_path, new_chunk)?;
    Ok(())
}

pub fn export_terrain(chunk_path: &Path, out_path: &Path, is_glb: bool) -> Result<(usize, usize)> {
    let data = fs::read(chunk_path)?;
    if is_glb {
        let (glb, v_count, tri_count) = export_terrain_to_glb(&data)?;
        fs::write(out_path, glb)?;
        Ok((v_count, tri_count))
    } else {
        let (obj, v_count, tri_count) = export_terrain_to_obj(&data)?;
        fs::write(out_path, obj)?;
        Ok((v_count, tri_count))
    }
}

pub fn export_collision_glb(chunk_path: &Path, out_path: &Path) -> Result<usize> {
    let data = fs::read(chunk_path).with_context(|| format!("Failed to read {:?}", chunk_path))?;
    let glb = export_collision_to_glb(&data)?;
    fs::write(out_path, &glb)?;
    Ok(glb.len())
}

pub fn import_collision_glb(chunk_path: &Path, in_path: &Path) -> Result<()> {
    let glb_bytes = fs::read(in_path).with_context(|| format!("Failed to read {:?}", in_path))?;
    let clb_binary = import_collision_from_glb(&glb_bytes)?;
    fs::write(chunk_path, clb_binary)?;
    Ok(())
}

pub fn export_lua(chunk_path: &Path, out_path: &Path) -> Result<()> {
    let data = fs::read(chunk_path)?;
    let bytecode = extract_lua_bytecode(&data)?;
    fs::write(out_path, bytecode)?;
    Ok(())
}

pub fn import_lua(chunk_path: &Path, in_path: &Path) -> Result<()> {
    let chunk_data = fs::read(chunk_path)?;
    let bytecode = if in_path.extension().is_some_and(|ext| ext == "lua") {
        compile_lua_script(in_path)?
    } else {
        fs::read(in_path)?
    };
    let new_chunk = replace_lua_bytecode(&chunk_data, &bytecode)?;
    fs::write(chunk_path, new_chunk)?;
    Ok(())
}

pub fn export_anim_glb(chunk_path: &Path, out_path: &Path) -> Result<()> {
    let data = fs::read(chunk_path)?;
    let glb = export_animation_to_glb(&data)?;
    fs::write(out_path, glb)?;
    Ok(())
}

pub fn export_anim_json(chunk_path: &Path, out_path: &Path) -> Result<()> {
    let data = fs::read(chunk_path)?;
    let json_str = export_animation_to_json(&data)?;
    fs::write(out_path, json_str)?;
    Ok(())
}

pub fn save_material(chunk_path: &Path, json_data: &str) -> Result<()> {
    let bin = import_material_from_json(json_data)?;
    fs::write(chunk_path, bin)?;
    Ok(())
}

pub fn save_ui(chunk_path: &Path, json_data: &str) -> Result<()> {
    let bin = import_ui_from_json(json_data)?;
    fs::write(chunk_path, bin)?;
    Ok(())
}

pub fn save_object(chunk_path: &Path, json_data: &str) -> Result<()> {
    let bin = import_object_from_json(json_data)?;
    fs::write(chunk_path, bin)?;
    Ok(())
}

pub fn save_terrain_palette(chunk_path: &Path, json_data: &str) -> Result<()> {
    let bin = import_terrain_palette_from_json(json_data)?;
    fs::write(chunk_path, bin)?;
    Ok(())
}

pub fn save_collision(chunk_path: &Path, json_data: &str) -> Result<()> {
    let baseline = fs::read(chunk_path)?;
    let bin = import_collision_from_json(json_data, &baseline)?;
    fs::write(chunk_path, bin)?;
    Ok(())
}

pub fn save_font(chunk_path: &Path, json_data: &str) -> Result<()> {
    let baseline = fs::read(chunk_path)?;
    let bin = import_font_from_json(json_data, &baseline)?;
    fs::write(chunk_path, bin)?;
    Ok(())
}

pub fn save_environment(chunk_path: &Path, json_data: &str) -> Result<()> {
    let baseline = fs::read(chunk_path)?;
    let bin = import_environment_from_json(json_data, &baseline)?;
    fs::write(chunk_path, bin)?;
    Ok(())
}

pub fn save_m8ld(chunk_path: &Path, xml_or_json_data: &str) -> Result<()> {
    let bin = import_m8ld_from_json(xml_or_json_data)?;
    fs::write(chunk_path, bin)?;
    Ok(())
}

pub fn export_dta(chunk_path: &Path, out_path: &Path) -> Result<()> {
    let data = fs::read(chunk_path).with_context(|| format!("Failed to read {:?}", chunk_path))?;
    let stem = chunk_path.file_stem().unwrap_or_default().to_string_lossy();
    let json_str = export_dta_to_json(&data, &stem)?;
    fs::write(out_path, json_str.as_bytes())?;
    Ok(())
}

pub fn save_dta(chunk_path: &Path, json_data: &str) -> Result<()> {
    let baseline = fs::read(chunk_path)?;
    let bin = import_dta_from_json(json_data, &baseline)?;
    fs::write(chunk_path, bin)?;
    Ok(())
}

pub fn export_vpk(chunk_path: &Path, out_path: &Path) -> Result<()> {
    let data = fs::read(chunk_path).with_context(|| format!("Failed to read {:?}", chunk_path))?;
    let json_str = export_vpk_to_json(&data)?;
    fs::write(out_path, json_str.as_bytes())?;
    Ok(())
}

pub fn save_vpk(chunk_path: &Path, json_data: &str) -> Result<()> {
    let baseline = fs::read(chunk_path)?;
    let bin = import_vpk_from_json(json_data, &baseline)?;
    fs::write(chunk_path, bin)?;
    Ok(())
}

pub fn decompile_8ld_file(src_path: &Path, dst_path: &Path) -> Result<PathBuf> {
    let data = fs::read(src_path).with_context(|| format!("Failed to read {:?}", src_path))?;
    let (seed, xml_str) = decompile_8ld_to_xml(&data)?;

    let target_file = if dst_path.is_dir() {
        let stem = src_path.file_stem().unwrap_or_default();
        dst_path.join(format!("{}.xml", stem.to_string_lossy()))
    } else {
        dst_path.to_path_buf()
    };

    if let Some(parent) = target_file.parent() {
        fs::create_dir_all(parent)?;
    }

    // 1. Write clean XML text document
    fs::write(&target_file, xml_str.as_bytes())?;

    // 2. Write companion .meta.json file alongside the .xml
    let meta_path = target_file.with_extension("meta.json");
    let meta = M8ldMetaJson {
        magic: "M8LD".to_string(),
        crc_or_flags_hex: format!("0x{:02X}", seed),
        original_payload_size: data.len(),
        unmapped_binary_hex: None,
    };
    if let Ok(meta_str) = serde_json::to_string_pretty(&meta) {
        let _ = fs::write(meta_path, meta_str.as_bytes());
    }

    Ok(target_file)
}

pub fn compile_8ld_file(src_path: &Path, dst_path: &Path) -> Result<PathBuf> {
    let xml_text =
        fs::read_to_string(src_path).with_context(|| format!("Failed to read {:?}", src_path))?;

    let target_file = if dst_path.is_dir() {
        let stem = src_path.file_stem().unwrap_or_default();
        dst_path.join(format!("{}.8ld", stem.to_string_lossy()))
    } else {
        dst_path.to_path_buf()
    };

    if let Some(parent) = target_file.parent() {
        fs::create_dir_all(parent)?;
    }

    // Recover seed from companion .meta.json, target container header, or fallback to default
    let meta_path = src_path.with_extension("meta.json");
    let seed = if meta_path.exists()
        && let Ok(meta_str) = fs::read_to_string(&meta_path)
        && let Ok(meta) = serde_json::from_str::<M8ldMetaJson>(&meta_str)
    {
        u32::from_str_radix(meta.crc_or_flags_hex.trim_start_matches("0x"), 16).unwrap_or(0x91)
    } else if target_file.exists()
        && let Ok(old_data) = fs::read(&target_file)
        && old_data.len() >= 5
        && old_data.starts_with(M8LD_MAGIC)
    {
        old_data[4] as u32
    } else {
        0x91
    };

    let binary = compile_xml_to_8ld(&xml_text, seed);
    fs::write(&target_file, binary)?;
    Ok(target_file)
}

/// Recursively batch-converts all .8ld files in `src_dir` into .xml files in `dst_dir`,
/// preserving the exact subfolder structure.
pub fn batch_decompile_8ld(src_dir: &Path, dst_dir: &Path) -> Result<usize> {
    fs::create_dir_all(dst_dir)?;
    let mut count = 0;

    for entry in WalkDir::new(src_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file()
            && path.extension().is_some_and(|ext| ext == "8ld")
            && let Ok(rel) = path.strip_prefix(src_dir)
        {
            let out_file = dst_dir.join(rel).with_extension("xml");
            if let Some(parent) = out_file.parent() {
                fs::create_dir_all(parent)?;
            }
            if decompile_8ld_file(path, &out_file).is_ok() {
                count += 1;
            }
        }
    }

    Ok(count)
}

/// Recursively batch-converts all .xml files in `src_dir` into .8ld files in `dst_dir`,
/// preserving the exact subfolder structure.
pub fn batch_compile_8ld(src_dir: &Path, dst_dir: &Path) -> Result<usize> {
    fs::create_dir_all(dst_dir)?;
    let mut count = 0;

    for entry in WalkDir::new(src_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file()
            && path.extension().is_some_and(|ext| ext == "xml")
            && let Ok(rel) = path.strip_prefix(src_dir)
        {
            let out_file = dst_dir.join(rel).with_extension("8ld");
            if let Some(parent) = out_file.parent() {
                fs::create_dir_all(parent)?;
            }
            if compile_8ld_file(path, &out_file).is_ok() {
                count += 1;
            }
        }
    }

    Ok(count)
}

pub fn save_ui_sprite(chunk_path: &Path, json_data: &str) -> Result<()> {
    let bin = import_ui_sprite_collection(json_data)?;
    fs::write(chunk_path, bin)?;
    Ok(())
}
