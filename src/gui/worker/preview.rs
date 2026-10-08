use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::engine::assets::animation::{
    AnimationClip, ObjectBone, parse_animation_clip, parse_object_bone_container,
};
use crate::engine::math::Vector3;
use crate::gui::{AvailableRig, RenderSubmesh};
use crate::utils::dds_decoder;
use crate::utils::tangents::generate_tangents;

pub struct ResolvedTexture {
    pub filename: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[inline]
pub fn s_normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// Discovers all unique rigs across the project: embedded mesh bones, objects/*.json, and binary bone containers in chunks/.
pub fn discover_all_project_rigs(
    mesh_embedded_bones: &[ObjectBone],
    project_dir: Option<&Path>,
) -> Vec<AvailableRig> {
    let mut rigs = Vec::new();

    // 1. Embedded rig from the mesh itself (if present)
    if !mesh_embedded_bones.is_empty() {
        rigs.push(AvailableRig {
            name: format!("Embedded Mesh Rig ({} bones)", mesh_embedded_bones.len()),
            bone_count: mesh_embedded_bones.len(),
            bones: mesh_embedded_bones.to_vec(),
            source_file: "embedded".into(),
        });
    }

    let base_dir = match project_dir {
        Some(d) => d,
        None => return rigs,
    };

    let objects_dir = base_dir.join("assets").join("objects");
    let chunks_dir = base_dir.join("chunks");

    // 2. Discover rigs from assets/objects/*.json
    if objects_dir.exists()
        && let Ok(entries) = fs::read_dir(&objects_dir)
    {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|ext| ext == "json")
                && let Ok(content) = fs::read_to_string(&p)
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&content)
                && let Some(bones_val) = v.get("bones")
                && let Ok(obj_bones) = serde_json::from_value::<
                    Vec<crate::engine::assets::object::FullObjectBoneJson>,
                >(bones_val.clone())
                && !obj_bones.is_empty()
            {
                let name = v["entity_name"]
                    .as_str()
                    .or_else(|| v["group_tag"].as_str())
                    .unwrap_or_else(|| p.file_stem().unwrap().to_str().unwrap());

                let bones: Vec<ObjectBone> =
                    obj_bones.into_iter().map(|b| b.to_object_bone()).collect();
                rigs.push(AvailableRig {
                    name: format!("{} ({} bones)", name, bones.len()),
                    bone_count: bones.len(),
                    bones,
                    source_file: p.file_name().unwrap().to_string_lossy().to_string(),
                });
            }
        }
    }

    // 3. Discover rigs directly from chunks/*.bin
    if chunks_dir.exists()
        && let Ok(entries) = fs::read_dir(&chunks_dir)
    {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_file()
                && p.extension().is_some_and(|ext| ext == "bin")
                && let Ok(bytes) = fs::read(&p)
                && let Ok(bones) = parse_object_bone_container(&bytes)
                && bones.len() >= 3
            {
                let stem = p.file_stem().unwrap().to_string_lossy();
                let sniffed = crate::engine::assets::sniffer::sniff_asset(&bytes, &stem);

                let already_has = rigs.iter().any(|r| {
                    r.bone_count == bones.len()
                        && r.bones.first().map(|b| &b.name) == bones.first().map(|b| &b.name)
                });

                if !already_has {
                    let clean_title = sniffed
                        .display_name
                        .split('(')
                        .next()
                        .unwrap_or(&stem)
                        .trim();
                    rigs.push(AvailableRig {
                        name: format!("{} [{}] ({} bones)", clean_title, stem, bones.len()),
                        bone_count: bones.len(),
                        bones,
                        source_file: format!("{}.bin", stem),
                    });
                }
            }
        }
    }

    rigs
}

/// Loads a submesh with its geometry, computed MikkTSpace tangents, and resolved Diffuse & Normal textures.
pub fn load_mesh_with_smart_texture(
    bytes: &[u8],
    mesh_stem: &str,
    project_dir: Option<&Path>,
) -> Option<RenderSubmesh> {
    let parsed = crate::engine::assets::mesh::extract_mesh_geometry(bytes).ok()?;
    let (resolved_diffuse, resolved_normal) =
        resolve_smart_textures_for_mesh(project_dir, mesh_stem, Some(bytes));

    let tex_arc = resolved_diffuse.map(|t| Arc::new((t.width, t.height, t.rgba)));
    let norm_arc = resolved_normal.map(|t| Arc::new((t.width, t.height, t.rgba)));

    let tangents = generate_tangents(
        &parsed.positions,
        &parsed.normals,
        &parsed.uvs,
        &parsed.indices,
    );

    Some(RenderSubmesh {
        name: mesh_stem.to_string(),
        positions: parsed.positions.clone(),
        normals: parsed.normals.clone(),
        tangents: tangents.clone(),
        rest_positions: parsed.positions,
        rest_normals: parsed.normals,
        rest_tangents: tangents,
        joints: parsed.joints,
        weights: parsed.weights,
        bones: parsed.bones,
        indices: parsed.indices,
        uvs: parsed.uvs,
        texture: tex_arc,
        normal_texture: norm_arc,
    })
}

/// Discovers both diffuse texture and normal map (_n, _norm, or through material descriptors).
pub fn resolve_smart_textures_for_mesh(
    project_dir: Option<&Path>,
    mesh_stem: &str,
    mesh_bytes: Option<&[u8]>,
) -> (Option<ResolvedTexture>, Option<ResolvedTexture>) {
    let base_dir = match project_dir {
        Some(d) => d,
        None => return (None, None),
    };

    let assets_dir = base_dir.join("assets");
    let textures_dir = assets_dir.join("textures");
    let objects_dir = assets_dir.join("objects");
    let materials_dir = assets_dir.join("materials");

    let mut candidate_tags: Vec<String> = Vec::new();
    candidate_tags.push(s_normalize(mesh_stem));

    if let Some(bytes) = mesh_bytes {
        let sniffed = crate::engine::assets::sniffer::sniff_asset(bytes, mesh_stem);
        candidate_tags.push(s_normalize(&sniffed.display_name));
    }

    if let Ok(cache_content) = fs::read_to_string(base_dir.join(".asset_cache.json"))
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&cache_content)
        && let Some(entries) = v["entries"].as_object()
    {
        for (glb_path, entry_val) in entries {
            let rel = entry_val["chunk_rel_path"].as_str().unwrap_or_default();
            if rel.contains(mesh_stem) || mesh_stem.contains(rel) {
                candidate_tags.push(s_normalize(glb_path));
            }
        }
    }

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
                    let norm_path = s_normalize(m_path);

                    if candidate_tags
                        .iter()
                        .any(|tag| tag.contains(&norm_path) || norm_path.contains(tag))
                        && let Some(mat_path) = b["material_path"].as_str()
                    {
                        bound_material_name = Some(mat_path.to_string());
                        break;
                    }
                }
            }
            if bound_material_name.is_some() {
                break;
            }
        }
    }

    let mut diffuse_filename: Option<String> = None;
    let mut normal_filename: Option<String> = None;

    if let Some(ref mat_target) = bound_material_name
        && materials_dir.exists()
        && let Ok(entries) = fs::read_dir(&materials_dir)
    {
        let target_norm = s_normalize(mat_target);

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

                if s_normalize(mat_val) == target_norm
                    && let Some(blocks) = v["blocks"].as_array()
                {
                    for b in blocks {
                        let role = b["role"].as_str().unwrap_or_default();
                        let b_id = b["id"].as_u64().unwrap_or(0);

                        if let Some(tex_name) = b["name"].as_str() {
                            if (role.contains("Diffuse")
                                || role.contains("Base Color")
                                || b_id == 30)
                                && diffuse_filename.is_none()
                            {
                                diffuse_filename = Some(tex_name.to_string());
                            } else if (role.contains("Normal") || b_id == 42 || b_id == 50)
                                && normal_filename.is_none()
                            {
                                normal_filename = Some(tex_name.to_string());
                            }
                        }
                    }

                    if diffuse_filename.is_none() {
                        let stem = e
                            .path()
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        if let Some(pos) = stem.find(".dds") {
                            diffuse_filename = Some(stem[..pos + 4].to_string());
                        }
                    }
                }
            }
        }
    }

    let load_texture_by_target = |name_opt: Option<&str>| -> Option<ResolvedTexture> {
        let name = name_opt?;
        let norm_name = s_normalize(name);

        if textures_dir.exists()
            && let Ok(entries) = fs::read_dir(&textures_dir)
        {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_file() && p.extension().is_some_and(|ext| ext == "dds") {
                    let fname = p.file_name().unwrap_or_default().to_string_lossy();
                    let norm_fname = s_normalize(&fname);

                    if (norm_fname.contains(&norm_name) || norm_name.contains(&norm_fname))
                        && let Ok(bytes) = fs::read(&p)
                        && let Ok(parsed) =
                            crate::engine::assets::texture::parse_texture_chunk(&bytes)
                    {
                        let rgba = dds_decoder::decode_to_rgba(
                            parsed.width,
                            parsed.height,
                            parsed.format,
                            &parsed.pixel_data,
                        );
                        return Some(ResolvedTexture {
                            filename: fname.to_string(),
                            width: parsed.width,
                            height: parsed.height,
                            rgba,
                        });
                    }
                }
            }
        }
        None
    };

    let resolved_diffuse = load_texture_by_target(diffuse_filename.as_deref());

    let resolved_normal = if let Some(ref n) = normal_filename {
        load_texture_by_target(Some(n))
    } else if let Some(ref diff) = diffuse_filename {
        let base_stem = diff
            .trim_end_matches(".dds")
            .trim_end_matches(".tga")
            .trim_end_matches("_d")
            .trim_end_matches("_diffuse");

        let n_candidates = [
            format!("{}_n.dds", base_stem),
            format!("{}_norm.dds", base_stem),
            format!("{}_normal.dds", base_stem),
        ];

        n_candidates
            .iter()
            .find_map(|c| load_texture_by_target(Some(c)))
    } else {
        None
    };

    (resolved_diffuse, resolved_normal)
}

pub fn resolve_smart_texture_for_mesh(
    project_dir: Option<&Path>,
    mesh_stem: &str,
    mesh_bytes: Option<&[u8]>,
) -> Option<ResolvedTexture> {
    let (diffuse, _) = resolve_smart_textures_for_mesh(project_dir, mesh_stem, mesh_bytes);
    diffuse
}

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
                        && (norm_rig.contains("minion") || norm_clip.contains("minion"))
                    || norm_disp.contains("scarecrow")
                        && (norm_rig.contains("scarecrow") || norm_clip.contains("scarecrow"));

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
        let tex_status = match (sm.texture.is_some(), sm.normal_texture.is_some()) {
            (true, true) => "Diffuse + Normal Map Linked",
            (true, false) => "Diffuse Linked (Flat Normal)",
            (false, _) => "Neutral Slate (No Texture)",
        };
        stats_lines.push(format!("Submesh [{}]: {} ({})", i, sm.name, tex_status).into());
    }

    stats_lines
}
