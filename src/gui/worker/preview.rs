use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::engine::assets::animation::{
    AnimationClip, ObjectBone, parse_animation_clip, parse_object_bone_container,
};
use crate::engine::math::Vector3;
use crate::gui::RenderSubmesh;
use crate::utils::dds_decoder;

pub struct ResolvedTexture {
    pub filename: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn s_normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

pub fn load_mesh_with_smart_texture(
    bytes: &[u8],
    mesh_stem: &str,
    project_dir: Option<&Path>,
) -> Option<RenderSubmesh> {
    let parsed = crate::engine::assets::mesh::extract_mesh_geometry(bytes).ok()?;
    let resolved_tex = resolve_smart_texture_for_mesh(project_dir, mesh_stem);
    let tex_arc = resolved_tex.map(|t| Arc::new((t.width, t.height, t.rgba)));

    Some(RenderSubmesh {
        name: mesh_stem.to_string(),
        positions: parsed.positions.clone(),
        normals: parsed.normals.clone(),
        rest_positions: parsed.positions,
        rest_normals: parsed.normals,
        joints: parsed.joints,
        weights: parsed.weights,
        bones: parsed.bones,
        indices: parsed.indices,
        uvs: parsed.uvs,
        texture: tex_arc,
    })
}

/// Finds the best matching skeleton for the given mesh using dependency_graph.json or the master object chunk.
pub fn find_master_skeleton_for_mesh(
    mesh_stem: &str,
    project_dir: Option<&Path>,
) -> Option<Vec<ObjectBone>> {
    let base_dir = project_dir?;
    let chunks_dir = base_dir.join("chunks");
    let assets_dir = base_dir.join("assets");

    if !chunks_dir.exists() {
        return None;
    }

    let norm_mesh = s_normalize(mesh_stem);

    // 1. Try resolving via objects directory (find an object JSON referencing this mesh)
    let objects_dir = assets_dir.join("objects");
    if objects_dir.exists()
        && let Ok(entries) = fs::read_dir(&objects_dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && let Ok(content) = fs::read_to_string(&path)
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&content)
            {
                let mut matches = false;
                if let Some(bindings) = v["mesh_bindings"].as_array() {
                    for b in bindings {
                        let m_path = b["mesh_path"].as_str().unwrap_or_default();
                        let m_name = b["mesh_part_name"].as_str().unwrap_or_default();
                        if s_normalize(m_path).contains(&norm_mesh)
                            || norm_mesh.contains(&s_normalize(m_path))
                            || s_normalize(m_name).contains(&norm_mesh)
                        {
                            matches = true;
                            break;
                        }
                    }
                }

                if matches
                    && let Some(bones_val) = v["bones"].as_array()
                    && !bones_val.is_empty()
                    && let Ok(obj_bones) = serde_json::from_value::<
                        Vec<crate::engine::assets::object::FullObjectBoneJson>,
                    >(v["bones"].clone())
                {
                    return Some(obj_bones.into_iter().map(|b| b.to_object_bone()).collect());
                }
            }
        }
    }

    // 2. Prioritize candidate chunks with full adult skeletons (e.g., chunk_0071)
    let candidates = [
        "chunk_0071_id0x2F.bin",
        "chunk_0072_id0x30.bin",
        "chunk_0066_id0x2A.bin",
    ];

    for c in &candidates {
        let p = chunks_dir.join(c);
        if p.exists()
            && let Ok(bytes) = fs::read(&p)
            && let Ok(bones) = parse_object_bone_container(&bytes)
            && bones.len() >= 40
        {
            return Some(bones);
        }
    }

    // 3. Fallback: Search any chunk containing a complete bone hierarchy
    let mut fallback = None;
    if let Ok(entries) = fs::read_dir(&chunks_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file()
                && p.extension().is_some_and(|e| e == "bin")
                && let Ok(bytes) = fs::read(&p)
                && let Ok(bones) = parse_object_bone_container(&bytes)
                && !bones.is_empty()
            {
                let len = bones.len();
                if len >= 40 {
                    return Some(bones);
                }
                if fallback.is_none() {
                    fallback = Some(bones);
                }
            }
        }
    }

    fallback
}

pub fn find_master_skeleton(project_dir: Option<&Path>) -> Option<Vec<ObjectBone>> {
    find_master_skeleton_for_mesh("", project_dir)
}

pub fn discover_companion_animations(
    display_name: &str,
    mesh_stem: &str,
    project_dir: Option<&Path>,
) -> Vec<AnimationClip> {
    let base_dir = match project_dir {
        Some(d) => d,
        None => return Vec::new(),
    };

    let mut clips = Vec::new();
    let chunks_dir = base_dir.join("chunks");

    let pkg_name = display_name
        .split(']')
        .next()
        .map(|s| s.trim_start_matches('['))
        .unwrap_or(display_name);

    let norm_pkg = s_normalize(pkg_name);
    let norm_stem = s_normalize(mesh_stem);
    let norm_disp = s_normalize(display_name);

    if chunks_dir.exists()
        && let Ok(entries) = fs::read_dir(&chunks_dir)
    {
        for e in entries.flatten() {
            let path = e.path();
            if path.is_file()
                && path.extension().is_some_and(|ext| ext == "bin")
                && let Ok(bytes) = fs::read(&path)
                && let Ok(clip) = parse_animation_clip(&bytes)
            {
                let norm_rig = s_normalize(&clip.target_rig);
                let norm_clip = s_normalize(&clip.name);

                let is_match = !norm_pkg.is_empty()
                    && (norm_rig.contains(&norm_pkg) || norm_pkg.contains(&norm_rig))
                    || norm_disp.contains(&norm_rig)
                    || norm_rig.contains(&norm_stem)
                    || norm_clip.contains(&norm_stem)
                    || norm_disp.contains("beetle")
                        && (norm_rig.contains("beetle") || norm_clip.contains("beetle"))
                    || norm_disp.contains("minion")
                        && (norm_rig.contains("minion") || norm_clip.contains("minion"));

                if is_match {
                    clips.push(clip);
                }
            }
        }

        if clips.is_empty() {
            for e in fs::read_dir(&chunks_dir)
                .ok()
                .into_iter()
                .flatten()
                .flatten()
            {
                let path = e.path();
                if path.is_file()
                    && path.extension().is_some_and(|ext| ext == "bin")
                    && let Ok(bytes) = fs::read(&path)
                    && let Ok(clip) = parse_animation_clip(&bytes)
                {
                    clips.push(clip);
                }
            }
        }
    }

    clips.sort_by(|a, b| a.name.cmp(&b.name));
    clips.dedup_by(|a, b| a.name == b.name);
    clips
}

pub fn resolve_smart_texture_for_mesh(
    project_dir: Option<&Path>,
    mesh_stem: &str,
) -> Option<ResolvedTexture> {
    let base_dir = project_dir?;
    let assets_dir = base_dir.join("assets");
    let textures_dir = assets_dir.join("textures");
    let objects_dir = assets_dir.join("objects");
    let materials_dir = assets_dir.join("materials");

    let norm_mesh = s_normalize(mesh_stem);

    let mut bound_material_name: Option<String> = None;
    if objects_dir.exists()
        && let Ok(entries) = fs::read_dir(&objects_dir)
    {
        for e in entries.flatten() {
            if e.path().extension().is_some_and(|ext| ext == "json")
                && let Ok(content) = fs::read_to_string(e.path())
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&content)
                && let Some(bindings) = v["mesh_bindings"].as_array()
            {
                for b in bindings {
                    let m_path = b["mesh_path"].as_str().unwrap_or_default();
                    let m_name = b["mesh_part_name"].as_str().unwrap_or_default();
                    let norm_path = s_normalize(m_path);
                    let norm_name = s_normalize(m_name);

                    if (norm_mesh.contains(&norm_path)
                        || norm_mesh.contains(&norm_name)
                        || norm_path.contains(&norm_mesh))
                        && let Some(mat_path) = b["material_path"].as_str()
                    {
                        bound_material_name = Some(mat_path.to_string());
                        break;
                    }
                }
            }
        }
    }

    let mut matched_texture_filename: Option<String> = None;
    if let Some(ref mat_target) = bound_material_name
        && materials_dir.exists()
        && let Ok(entries) = fs::read_dir(&materials_dir)
    {
        for e in entries.flatten() {
            if e.path().extension().is_some_and(|ext| ext == "json")
                && let Ok(content) = fs::read_to_string(e.path())
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&content)
            {
                let mat_val = v["blocks"]
                    .as_array()
                    .and_then(|blocks| {
                        blocks
                            .iter()
                            .find(|b| b["id"] == 20)
                            .and_then(|b| b["value"].as_str())
                    })
                    .unwrap_or_default();

                if s_normalize(mat_val) == s_normalize(mat_target)
                    && let Some(blocks) = v["blocks"].as_array()
                {
                    for b in blocks {
                        let role = b["role"].as_str().unwrap_or_default();
                        if (role.contains("Diffuse")
                            || role.contains("Base Color")
                            || b["id"] == 30)
                            && let Some(tex_name) = b["name"].as_str()
                        {
                            matched_texture_filename = Some(tex_name.to_string());
                            break;
                        }
                    }
                }
            }
        }
    }

    if textures_dir.exists()
        && let Ok(entries) = fs::read_dir(&textures_dir)
    {
        let mut fallback_dds: Option<PathBuf> = None;
        let mut diff_dds: Option<PathBuf> = None;
        let clean_target = matched_texture_filename.as_deref().map(s_normalize);

        for e in entries.flatten() {
            let p = e.path();
            if p.is_file() && p.extension().is_some_and(|ext| ext == "dds") {
                let fname = p.file_name().unwrap_or_default().to_string_lossy();
                let norm_fname = s_normalize(&fname);

                if fallback_dds.is_none() {
                    fallback_dds = Some(p.clone());
                }
                if norm_fname.contains("diff") || norm_fname.contains("base") {
                    diff_dds = Some(p.clone());
                }

                if let Some(ref target) = clean_target
                    && (norm_fname.contains(target) || target.contains(&norm_fname))
                {
                    diff_dds = Some(p);
                    break;
                }
            }
        }

        let chosen_texture = diff_dds.or(fallback_dds)?;
        if let Ok(dds_bytes) = fs::read(&chosen_texture)
            && let Ok(parsed_tex) = crate::engine::assets::texture::parse_texture_chunk(&dds_bytes)
        {
            let rgba = dds_decoder::decode_to_rgba(
                parsed_tex.width,
                parsed_tex.height,
                parsed_tex.format,
                &parsed_tex.pixel_data,
            );
            return Some(ResolvedTexture {
                filename: chosen_texture
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                width: parsed_tex.width,
                height: parsed_tex.height,
                rgba,
            });
        }
    }

    None
}

pub fn build_composite_mesh_assembly(
    _stem: &str,
    project_dir: Option<&Path>,
) -> (Vec<RenderSubmesh>, bool) {
    let base_dir = match project_dir {
        Some(d) => d,
        None => return (Vec::new(), false),
    };

    let assets_dir = base_dir.join("assets");
    let meshes_dir = assets_dir.join("meshes");
    let chunks_dir = base_dir.join("chunks");

    let mut submeshes = Vec::new();

    if meshes_dir.exists()
        && let Ok(entries) = fs::read_dir(&meshes_dir)
    {
        let mut mesh_paths: Vec<_> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "glb"))
            .map(|e| e.path())
            .collect();
        mesh_paths.sort();

        for m_path in mesh_paths {
            let m_stem = m_path.file_stem().unwrap_or_default().to_string_lossy();
            if m_stem.ends_with("_MASTER_RIG") {
                continue;
            }

            let chunk_name = m_stem.split('_').find(|s| s.starts_with("chunk"));
            let chunk_file = chunk_name.map(|c| chunks_dir.join(format!("{}.bin", c)));

            let bytes_opt = if let Some(ref cf) = chunk_file
                && cf.exists()
            {
                fs::read(cf).ok()
            } else {
                fs::read(&m_path).ok()
            };

            if let Some(bytes) = bytes_opt
                && let Some(sm) = load_mesh_with_smart_texture(&bytes, &m_stem, project_dir)
            {
                submeshes.push(sm);
            }
        }
    }

    let has_composite = submeshes.len() > 1;
    (submeshes, has_composite)
}

pub fn build_stats_lines(submeshes: &[RenderSubmesh]) -> Vec<slint::SharedString> {
    let mut stats_lines: Vec<slint::SharedString> = Vec::new();
    let total_verts: usize = submeshes.iter().map(|s| s.positions.len()).sum();
    let total_tris: usize = submeshes.iter().map(|s| s.indices.len() / 3).sum();

    let mut min = Vector3 {
        x: f32::INFINITY,
        y: f32::INFINITY,
        z: f32::INFINITY,
    };
    let mut max = Vector3 {
        x: f32::NEG_INFINITY,
        y: f32::NEG_INFINITY,
        z: f32::NEG_INFINITY,
    };

    for sm in submeshes {
        for p in &sm.positions {
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            min.z = min.z.min(p.z);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
            max.z = max.z.max(p.z);
        }
    }

    let sx = (max.x - min.x).abs();
    let sy = (max.y - min.y).abs();
    let sz = (max.z - min.z).abs();

    stats_lines.push(format!("Total Vertices: {}", total_verts).into());
    stats_lines.push(format!("Total Triangles: {}", total_tris).into());
    stats_lines.push(format!("Submesh Count: {}", submeshes.len()).into());
    stats_lines.push(format!("Size: {:.2}m × {:.2}m × {:.2}m", sx, sy, sz).into());
    stats_lines.push(format!("Bounds Min: [{:.2}, {:.2}, {:.2}]", min.x, min.y, min.z).into());
    stats_lines.push(format!("Bounds Max: [{:.2}, {:.2}, {:.2}]", max.x, max.y, max.z).into());

    for (i, sm) in submeshes.iter().enumerate() {
        let tex_status = if sm.texture.is_some() {
            "Texture: Linked"
        } else {
            "Texture: Neutral Slate"
        };
        stats_lines.push(format!("Submesh [{}]: {} ({})", i, sm.name, tex_status).into());
    }

    stats_lines
}
