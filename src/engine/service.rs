use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::engine::assets::animation::{export_animation_to_glb, export_animation_to_json};
use crate::engine::assets::audio::{export_wav, replace_wav};
use crate::engine::assets::lua::{compile_lua_script, extract_lua_bytecode, replace_lua_bytecode};
use crate::engine::assets::material::import_material_from_json;
use crate::engine::assets::mesh::{
    MeshStats, export_mesh_to_glb, export_mesh_to_obj, import_glb_to_mesh, import_obj_to_mesh,
};
use crate::engine::assets::object::import_object_from_json;
use crate::engine::assets::terrain::{export_terrain_to_glb, export_terrain_to_obj};
use crate::engine::assets::terrain_palette::import_terrain_palette_from_json;
use crate::engine::assets::texture::{export_to_dds, replace_texture_in_chunk};
use crate::engine::assets::ui::import_ui_from_json;

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
        let obj = export_terrain_to_obj(&data)?;
        fs::write(out_path, obj)?;
        Ok((0, 0))
    }
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
