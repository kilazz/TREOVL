use byteorder::{LittleEndian, ReadBytesExt};
use crc32fast::Hasher;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use crate::engine::assets::animation::{
    export_animation_to_glb, export_animation_to_json, export_skeleton_to_glb,
    parse_object_bone_container,
};
use crate::engine::assets::audio::{export_wav, replace_wav};
use crate::engine::assets::lua::{
    disassemble_lua_bytecode, extract_lua_bytecode, replace_lua_bytecode,
};
use crate::engine::assets::material::{export_material_to_json, import_material_from_json};
use crate::engine::assets::mesh::{export_mesh_to_glb, import_glb_to_mesh, import_obj_to_mesh};
use crate::engine::assets::shader::{ShaderType, export_shader};
use crate::engine::assets::sniffer::{AssetKind, sniff_asset};
use crate::engine::assets::texture::{export_to_dds, replace_texture_in_chunk};
use crate::engine::assets::{parse_chunk_elements, parse_typed_container};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AssetSyncEntry {
    pub chunk_rel_path: String,
    pub asset_kind: String,
    pub crc32: u32,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct AssetSyncCache {
    pub entries: HashMap<String, AssetSyncEntry>,
}

fn calculate_crc32(data: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

fn sanitize_filename(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '[' | ']' => '_',
            _ => c,
        })
        .collect();
    clean.trim_matches('_').to_string()
}

fn build_asset_filename(display_name: &str, stem: &str, ext: &str) -> String {
    let mut clean_title = sanitize_filename(display_name);

    if let Some(pos) = clean_title.find('(') {
        clean_title.truncate(pos);
    }
    let clean_title = clean_title.trim().trim_matches('_');

    if clean_title.is_empty() || clean_title == stem {
        format!("{}.{}", stem, ext)
    } else {
        format!("{}_{}.{}", clean_title, stem, ext)
    }
}

/// Extracts any plain-text Lua source code embedded inside binary containers.
fn extract_embedded_lua_text(data: &[u8]) -> Option<String> {
    if let Some(start_pos) = data.windows(2).position(|w| w == b"--") {
        let mut script_lines = Vec::new();
        let mut pos = start_pos;

        while pos < data.len() {
            if pos + 4 <= data.len() {
                let slen =
                    u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap_or_default()) as usize;
                if slen > 0 && slen < 500 && pos + 4 + slen <= data.len() {
                    let slice = &data[pos + 4..pos + 4 + slen];
                    if slice.iter().all(|&b| {
                        (0x20..=0x7E).contains(&b)
                            || b == b'\n'
                            || b == b'\r'
                            || b == b'\t'
                            || b == 0
                    }) && let Ok(s) = std::str::from_utf8(slice)
                    {
                        let clean = s.trim_matches(char::from(0));
                        if !clean.is_empty() {
                            script_lines.push(clean.to_string());
                        }
                        pos += 4 + slen;
                        continue;
                    }
                }
            }
            pos += 1;
            if pos - start_pos > 8000 {
                break;
            }
        }

        if !script_lines.is_empty() {
            return Some(script_lines.join("\n"));
        }
    }
    None
}

/// Extracts embedded FaceFX (.fxa) binary graph data.
fn extract_embedded_facefx(data: &[u8]) -> Option<Vec<u8>> {
    if let Some(pos) = data.windows(4).position(|w| w == b"FACE") {
        if pos + 8 <= data.len() {
            let mut cur = Cursor::new(&data[pos + 4..pos + 8]);
            let len = cur.read_u32::<LittleEndian>().unwrap_or(0) as usize;
            if len > 0 && pos + 8 + len <= data.len() {
                return Some(data[pos..pos + 8 + len].to_vec());
            }
        }
        return Some(data[pos..].to_vec());
    }
    None
}

fn build_unknown_chunk_dossier(chunk_data: &[u8], stem: &str) -> serde_json::Value {
    let magic_hex = if chunk_data.len() >= 4 {
        hex::encode_upper(&chunk_data[..4])
    } else {
        String::from("TOO_SHORT")
    };

    let mut extracted_strings = Vec::new();
    let mut i = 0;
    while i < chunk_data.len() {
        if i + 4 <= chunk_data.len() {
            let str_len =
                u32::from_le_bytes(chunk_data[i..i + 4].try_into().unwrap_or_default()) as usize;
            if (3..=128).contains(&str_len)
                && i + 4 + str_len <= chunk_data.len()
                && let slice = &chunk_data[i + 4..i + 4 + str_len]
                && slice.iter().all(|&b| (0x20..=0x7E).contains(&b) || b == 0)
                && let Ok(s) = std::str::from_utf8(slice)
            {
                let clean = s.trim_matches(char::from(0)).trim();
                if clean.len() >= 3 && !extracted_strings.contains(&clean.to_string()) {
                    extracted_strings.push(clean.to_string());
                }
            }
        }
        i += 1;
    }

    let mut sub_elements_info = Vec::new();
    if let Ok((has_magic, elements)) = parse_chunk_elements(chunk_data) {
        for (sub_id, sub_data) in elements {
            sub_elements_info.push(json!({
                "sub_id": sub_id,
                "sub_id_hex": format!("0x{:X}", sub_id),
                "size_bytes": sub_data.len(),
                "hex_preview": hex::encode_upper(&sub_data[..sub_data.len().min(32)])
            }));
        }
        if !sub_elements_info.is_empty() {
            return json!({
                "chunk": stem,
                "structure_type": "Untyped Container Sub-Table",
                "has_container_magic": has_magic,
                "total_bytes": chunk_data.len(),
                "sub_elements_count": sub_elements_info.len(),
                "sub_elements": sub_elements_info,
                "embedded_strings": extracted_strings,
                "full_hex": hex::encode_upper(chunk_data)
            });
        }
    }

    if let Ok((type_id, elements)) = parse_typed_container(chunk_data) {
        for (sub_id, sub_data) in elements {
            sub_elements_info.push(json!({
                "sub_id": sub_id,
                "size_bytes": sub_data.len(),
                "hex_preview": hex::encode_upper(&sub_data[..sub_data.len().min(32)])
            }));
        }
        return json!({
            "chunk": stem,
            "structure_type": "Typed Container",
            "type_id_hex": format!("{:08X}", type_id),
            "total_bytes": chunk_data.len(),
            "sub_elements_count": sub_elements_info.len(),
            "sub_elements": sub_elements_info,
            "embedded_strings": extracted_strings,
            "full_hex": hex::encode_upper(chunk_data)
        });
    }

    let scalar_guesses = if chunk_data.len() == 4 {
        let u = u32::from_le_bytes(chunk_data[0..4].try_into().unwrap_or_default());
        let i = i32::from_le_bytes(chunk_data[0..4].try_into().unwrap_or_default());
        let f = f32::from_le_bytes(chunk_data[0..4].try_into().unwrap_or_default());
        json!({
            "as_u32": u,
            "as_i32": i,
            "as_f32": if f.is_finite() { f } else { 0.0 }
        })
    } else {
        json!(null)
    };

    json!({
        "chunk": stem,
        "structure_type": "Raw Leaf Data",
        "total_bytes": chunk_data.len(),
        "magic_header_hex": magic_hex,
        "scalar_guesses": scalar_guesses,
        "embedded_strings": extracted_strings,
        "full_hex": hex::encode_upper(chunk_data)
    })
}

/// Generates human-editable assets in the `assets/` folder for 100% of the unpacked chunks.
pub fn export_smart_assets(project_dir: &Path) -> Result<usize, String> {
    let chunks_dir = project_dir.join("chunks");
    if !chunks_dir.exists() {
        return Err("Project chunks folder does not exist.".into());
    }

    let assets_dir = project_dir.join("assets");
    let textures_dir = assets_dir.join("textures");
    let audio_dir = assets_dir.join("audio");
    let materials_dir = assets_dir.join("materials");
    let meshes_dir = assets_dir.join("meshes");
    let animations_dir = assets_dir.join("animations");
    let events_dir = assets_dir.join("events");
    let behavior_dir = assets_dir.join("behavior");
    let attachments_dir = assets_dir.join("attachments");
    let facial_dir = assets_dir.join("facial");
    let scripts_dir = assets_dir.join("scripts");
    let shaders_dir = assets_dir.join("shaders");
    let objects_dir = assets_dir.join("objects");
    let xml_dir = assets_dir.join("xml");
    let params_dir = assets_dir.join("parameters");
    let raw_dir = assets_dir.join("raw_chunks");

    fs::create_dir_all(&textures_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&audio_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&materials_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&meshes_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&animations_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&events_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&behavior_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&attachments_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&facial_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&scripts_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&shaders_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&objects_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&xml_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&params_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&raw_dir).map_err(|e| e.to_string())?;

    let entries: Vec<PathBuf> = fs::read_dir(&chunks_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "bin"))
        .collect();

    let sync_records: Vec<(String, AssetSyncEntry)> = entries
        .par_iter()
        .filter_map(|chunk_path| {
            let chunk_data = fs::read(chunk_path).ok()?;
            let stem = chunk_path.file_stem()?.to_string_lossy();
            let sniffed = sniff_asset(&chunk_data, &stem);
            let rel_chunk_str = format!("chunks/{}", chunk_path.file_name()?.to_string_lossy());

            // 1. TEXTURES (DDS / TGA)
            if sniffed.kind == AssetKind::Texture
                && let Ok(dds_or_tga) = export_to_dds(&chunk_data)
            {
                let ext = if chunk_data.starts_with(b"\x98\x00\x41\x00") {
                    "tga"
                } else {
                    "dds"
                };
                let out_name = build_asset_filename(&sniffed.display_name, &stem, ext);
                let rel_asset_path = format!("assets/textures/{}", out_name);
                let abs_path = textures_dir.join(&out_name);

                if fs::write(&abs_path, &dds_or_tga).is_ok() {
                    return Some((
                        rel_asset_path,
                        AssetSyncEntry {
                            chunk_rel_path: rel_chunk_str,
                            asset_kind: "Texture".into(),
                            crc32: calculate_crc32(&dds_or_tga),
                        },
                    ));
                }
            }

            // 2. AUDIO (WAV)
            if sniffed.kind == AssetKind::Audio
                && let Ok(wav_bytes) = export_wav(&chunk_data)
            {
                let out_name = build_asset_filename(&sniffed.display_name, &stem, "wav");
                let rel_asset_path = format!("assets/audio/{}", out_name);
                let abs_path = audio_dir.join(&out_name);

                if fs::write(&abs_path, &wav_bytes).is_ok() {
                    return Some((
                        rel_asset_path,
                        AssetSyncEntry {
                            chunk_rel_path: rel_chunk_str,
                            asset_kind: "Audio".into(),
                            crc32: calculate_crc32(&wav_bytes),
                        },
                    ));
                }
            }

            // 3. MATERIALS (JSON)
            if sniffed.kind == AssetKind::Material
                && let Ok(json_str) = export_material_to_json(&chunk_data)
            {
                let out_name = build_asset_filename(&sniffed.display_name, &stem, "json");
                let rel_asset_path = format!("assets/materials/{}", out_name);
                let abs_path = materials_dir.join(&out_name);

                if fs::write(&abs_path, json_str.as_bytes()).is_ok() {
                    return Some((
                        rel_asset_path,
                        AssetSyncEntry {
                            chunk_rel_path: rel_chunk_str,
                            asset_kind: "Material".into(),
                            crc32: calculate_crc32(json_str.as_bytes()),
                        },
                    ));
                }
            }

            // 4. MESHES (GLB)
            if sniffed.kind == AssetKind::Mesh
                && let Ok((glb_bytes, _)) = export_mesh_to_glb(&chunk_data)
            {
                let out_name = build_asset_filename(&sniffed.display_name, &stem, "glb");
                let rel_asset_path = format!("assets/meshes/{}", out_name);
                let abs_path = meshes_dir.join(&out_name);

                if fs::write(&abs_path, &glb_bytes).is_ok() {
                    return Some((
                        rel_asset_path,
                        AssetSyncEntry {
                            chunk_rel_path: rel_chunk_str,
                            asset_kind: "Mesh".into(),
                            crc32: calculate_crc32(&glb_bytes),
                        },
                    ));
                }
            }

            // 5. ANIMATIONS (GLB & JSON)
            if (sniffed.kind == AssetKind::Animation || chunk_data.starts_with(b"\x05\x00\x41\x00"))
                && let Ok(glb_bytes) = export_animation_to_glb(&chunk_data)
            {
                let out_name = build_asset_filename(&sniffed.display_name, &stem, "glb");
                let rel_asset_path = format!("assets/animations/{}", out_name);
                let abs_path = animations_dir.join(&out_name);

                if fs::write(&abs_path, &glb_bytes).is_ok() {
                    if let Ok(anim_json) = export_animation_to_json(&chunk_data) {
                        let json_path = animations_dir.join(format!("{}.anim.json", out_name));
                        let _ = fs::write(json_path, anim_json);
                    }

                    return Some((
                        rel_asset_path,
                        AssetSyncEntry {
                            chunk_rel_path: rel_chunk_str,
                            asset_kind: "Animation".into(),
                            crc32: calculate_crc32(&glb_bytes),
                        },
                    ));
                }
            }

            // 6. CHARACTER AI BEHAVIOR, EMBEDDED LUA & FACEFX
            if sniffed.kind == AssetKind::Behavior {
                let dossier = build_unknown_chunk_dossier(&chunk_data, &stem);
                let out_name = build_asset_filename(&sniffed.display_name, &stem, "json");
                let rel_asset_path = format!("assets/behavior/{}", out_name);
                let abs_path = behavior_dir.join(&out_name);

                // A. Extract embedded Lua source script
                if let Some(lua_text) = extract_embedded_lua_text(&chunk_data) {
                    let script_filename = format!("{}_quest_script.lua", sanitize_filename(&sniffed.display_name));
                    let script_path = scripts_dir.join(script_filename);
                    let _ = fs::write(script_path, lua_text);
                }

                // B. Extract embedded FaceFX (.fxa)
                if let Some(fxa_bytes) = extract_embedded_facefx(&chunk_data) {
                    let fxa_filename = format!("{}.fxa", sanitize_filename(&sniffed.display_name));
                    let fxa_path = facial_dir.join(fxa_filename);
                    let _ = fs::write(fxa_path, fxa_bytes);
                }

                // C. Write full behavior structure
                if let Ok(json_str) = serde_json::to_string_pretty(&dossier)
                    && fs::write(&abs_path, json_str.as_bytes()).is_ok()
                {
                    return Some((
                        rel_asset_path,
                        AssetSyncEntry {
                            chunk_rel_path: rel_chunk_str,
                            asset_kind: "Behavior".into(),
                            crc32: calculate_crc32(json_str.as_bytes()),
                        },
                    ));
                }
            }

            // 7. ITEM ATTACHMENT SLOTS (e.g. chunk_0035 Plate)
            if sniffed.kind == AssetKind::Attachment {
                let dossier = build_unknown_chunk_dossier(&chunk_data, &stem);
                let out_name = build_asset_filename(&sniffed.display_name, &stem, "json");
                let rel_asset_path = format!("assets/attachments/{}", out_name);
                let abs_path = attachments_dir.join(&out_name);

                if let Ok(json_str) = serde_json::to_string_pretty(&dossier)
                    && fs::write(&abs_path, json_str.as_bytes()).is_ok()
                {
                    return Some((
                        rel_asset_path,
                        AssetSyncEntry {
                            chunk_rel_path: rel_chunk_str,
                            asset_kind: "Attachment".into(),
                            crc32: calculate_crc32(json_str.as_bytes()),
                        },
                    ));
                }
            }

            // 8. ANIMATION SOUND EVENTS (B0 00 00 04, e.g. Foot1, Foot2, Archie Hit)
            if (sniffed.kind == AssetKind::Event || chunk_data.starts_with(b"\xB0\x00\x00\x04") || chunk_data.starts_with(b"\x04\x00\x00\xB0"))
                && let Ok((type_id, elements)) = parse_typed_container(&chunk_data)
            {
                let mut event_name = String::new();
                let mut event_tag = String::new();
                let mut linked_sfx = Vec::new();

                for (id, val) in &elements {
                    match *id {
                        20 if val.len() >= 4 => {
                            let slen = u32::from_le_bytes(val[0..4].try_into().unwrap_or_default()) as usize;
                            if slen <= val.len() - 4 {
                                event_tag = String::from_utf8_lossy(&val[4..4 + slen]).trim_matches(char::from(0)).to_string();
                            }
                        }
                        21 if val.len() >= 4 => {
                            let slen = u32::from_le_bytes(val[0..4].try_into().unwrap_or_default()) as usize;
                            if slen <= val.len() - 4 {
                                event_name = String::from_utf8_lossy(&val[4..4 + slen]).trim_matches(char::from(0)).to_string();
                            }
                        }
                        40 => {
                            if let Ok((_, sfx_sub)) = parse_chunk_elements(val) {
                                for (_, s_chunk) in sfx_sub {
                                    if let Ok((_, s_props)) = parse_chunk_elements(&s_chunk) {
                                        let mut s_ptr = String::new();
                                        let mut s_name = String::new();
                                        for (pid, pval) in s_props {
                                            if pid == 20 && pval.len() >= 4 {
                                                let slen = u32::from_le_bytes(pval[0..4].try_into().unwrap_or_default()) as usize;
                                                if slen <= pval.len() - 4 {
                                                    s_ptr = String::from_utf8_lossy(&pval[4..4 + slen]).trim_matches(char::from(0)).to_string();
                                                }
                                            } else if pid == 21 && pval.len() >= 4 {
                                                let slen = u32::from_le_bytes(pval[0..4].try_into().unwrap_or_default()) as usize;
                                                if slen <= pval.len() - 4 {
                                                    s_name = String::from_utf8_lossy(&pval[4..4 + slen]).trim_matches(char::from(0)).to_string();
                                                }
                                            }
                                        }
                                        if !s_ptr.is_empty() {
                                            linked_sfx.push(json!({ "sound_ptr": s_ptr, "sound_name": s_name }));
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }

                let event_json = json!({
                    "type_id_hex": format!("{:08X}", type_id),
                    "event_tag": event_tag,
                    "event_name": event_name,
                    "triggered_sounds": linked_sfx
                });

                let out_name = build_asset_filename(&sniffed.display_name, &stem, "json");
                let rel_asset_path = format!("assets/events/{}", out_name);
                let abs_path = events_dir.join(&out_name);

                if let Ok(json_str) = serde_json::to_string_pretty(&event_json)
                    && fs::write(&abs_path, json_str.as_bytes()).is_ok()
                {
                    return Some((
                        rel_asset_path,
                        AssetSyncEntry {
                            chunk_rel_path: rel_chunk_str,
                            asset_kind: "Event".into(),
                            crc32: calculate_crc32(json_str.as_bytes()),
                        },
                    ));
                }
            }

            // 9. LUA STANDALONE SCRIPTS
            if sniffed.kind == AssetKind::Lua
                && let Ok(bytecode) = extract_lua_bytecode(&chunk_data)
            {
                let out_name = build_asset_filename(&sniffed.display_name, &stem, "luac");
                let rel_asset_path = format!("assets/scripts/{}", out_name);
                let abs_path = scripts_dir.join(&out_name);

                if fs::write(&abs_path, &bytecode).is_ok() {
                    if let Ok(disasm) = disassemble_lua_bytecode(&bytecode) {
                        let txt_path = scripts_dir.join(format!("{}.lua.txt", out_name));
                        let _ = fs::write(txt_path, disasm);
                    }

                    return Some((
                        rel_asset_path,
                        AssetSyncEntry {
                            chunk_rel_path: rel_chunk_str,
                            asset_kind: "Lua".into(),
                            crc32: calculate_crc32(&bytecode),
                        },
                    ));
                }
            }

            // 10. OBJECT ENTITY CHUNKS (4B 00 41 00)
            if (sniffed.kind == AssetKind::Object || chunk_data.starts_with(b"\x4B\x00\x41\x00"))
                && let Ok((type_id, elements)) = parse_typed_container(&chunk_data)
            {
                let mut object_tag = String::new();
                let mut object_name = String::new();
                let mut scale = vec![1.0f32, 1.0f32, 1.0f32];
                let mut linked_anim = json!(null);
                let mut submeshes = Vec::new();
                let mut bones_data = Vec::new();
                let mut raw_elements = Vec::new();

                for (id, val) in &elements {
                    match *id {
                        20 if val.len() >= 4 => {
                            let slen = u32::from_le_bytes(val[0..4].try_into().unwrap_or_default()) as usize;
                            if slen <= val.len() - 4 {
                                object_tag = String::from_utf8_lossy(&val[4..4 + slen]).trim_matches(char::from(0)).to_string();
                            }
                        }
                        21 if val.len() >= 4 => {
                            let slen = u32::from_le_bytes(val[0..4].try_into().unwrap_or_default()) as usize;
                            if slen <= val.len() - 4 {
                                object_name = String::from_utf8_lossy(&val[4..4 + slen]).trim_matches(char::from(0)).to_string();
                            }
                        }
                        32 if val.len() >= 12 => {
                            let mut cur = Cursor::new(val);
                            scale = vec![
                                cur.read_f32::<LittleEndian>().unwrap_or(1.0),
                                cur.read_f32::<LittleEndian>().unwrap_or(1.0),
                                cur.read_f32::<LittleEndian>().unwrap_or(1.0),
                            ];
                        }
                        36 => {
                            if let Ok((_, anim_sub)) = parse_chunk_elements(val) {
                                let mut atag = String::new();
                                let mut aname = String::new();
                                for (aid, aval) in anim_sub {
                                    if aid == 20 && aval.len() >= 4 {
                                        let slen = u32::from_le_bytes(aval[0..4].try_into().unwrap_or_default()) as usize;
                                        if slen <= aval.len() - 4 {
                                            atag = String::from_utf8_lossy(&aval[4..4 + slen]).trim_matches(char::from(0)).to_string();
                                        }
                                    } else if aid == 21 && aval.len() >= 4 {
                                        let slen = u32::from_le_bytes(aval[0..4].try_into().unwrap_or_default()) as usize;
                                        if slen <= aval.len() - 4 {
                                            aname = String::from_utf8_lossy(&aval[4..4 + slen]).trim_matches(char::from(0)).to_string();
                                        }
                                    }
                                }
                                linked_anim = json!({ "tag": atag, "name": aname });
                            }
                        }
                        30 => {
                            if let Ok((_, mesh_sub)) = parse_chunk_elements(val) {
                                for (_, sub_cont) in mesh_sub {
                                    if let Ok((_, m_props)) = parse_chunk_elements(&sub_cont) {
                                        let mut m_tag = String::new();
                                        let mut m_name = String::new();
                                        for (pid, pval) in m_props {
                                            if pid == 20 && pval.len() >= 4 {
                                                let slen = u32::from_le_bytes(pval[0..4].try_into().unwrap_or_default()) as usize;
                                                if slen <= pval.len() - 4 {
                                                    m_tag = String::from_utf8_lossy(&pval[4..4 + slen]).trim_matches(char::from(0)).to_string();
                                                }
                                            } else if pid == 21 && pval.len() >= 4 {
                                                let slen = u32::from_le_bytes(pval[0..4].try_into().unwrap_or_default()) as usize;
                                                if slen <= pval.len() - 4 {
                                                    m_name = String::from_utf8_lossy(&pval[4..4 + slen]).trim_matches(char::from(0)).to_string();
                                                }
                                            }
                                        }
                                        if !m_tag.is_empty() {
                                            submeshes.push(json!({ "tag": m_tag, "name": m_name }));
                                        }
                                    }
                                }
                            }
                        }
                        33 => {
                            if let Ok(bones) = parse_object_bone_container(val) {
                                bones_data = bones;
                            }
                        }
                        _ => {
                            raw_elements.push(json!({
                                "id": id,
                                "size_bytes": val.len(),
                                "hex": hex::encode_upper(val)
                            }));
                        }
                    }
                }

                if !bones_data.is_empty()
                    && let Ok(glb_armature) = export_skeleton_to_glb(&bones_data, &object_name)
                {
                    let rig_name = build_asset_filename(&sniffed.display_name, &stem, "glb");
                    let rig_path = objects_dir.join(format!("{}_rig.glb", rig_name));
                    let _ = fs::write(rig_path, glb_armature);
                }

                let obj_json = json!({
                    "type_id_hex": format!("{:08X}", type_id),
                    "object_tag": object_tag,
                    "object_name": object_name,
                    "scale": scale,
                    "linked_animation": linked_anim,
                    "submeshes": submeshes,
                    "bones_count": bones_data.len(),
                    "bones": bones_data,
                    "other_elements": raw_elements
                });

                let out_name = build_asset_filename(&sniffed.display_name, &stem, "json");
                let rel_asset_path = format!("assets/objects/{}", out_name);
                let abs_path = objects_dir.join(&out_name);

                if let Ok(json_str) = serde_json::to_string_pretty(&obj_json)
                    && fs::write(&abs_path, json_str.as_bytes()).is_ok()
                {
                    return Some((
                        rel_asset_path,
                        AssetSyncEntry {
                            chunk_rel_path: rel_chunk_str,
                            asset_kind: "Object".into(),
                            crc32: calculate_crc32(json_str.as_bytes()),
                        },
                    ));
                }
            }

            // 11. SHADERS (HLSL / DXBC)
            if let Ok((payload, s_type, name)) = export_shader(&chunk_data) {
                let ext = match s_type {
                    ShaderType::InternalHLSL => "hlsl",
                    _ => "dxbc",
                };
                let out_name = build_asset_filename(&name, &stem, ext);
                let rel_asset_path = format!("assets/shaders/{}", out_name);
                let abs_path = shaders_dir.join(&out_name);

                if fs::write(&abs_path, &payload).is_ok() {
                    return Some((
                        rel_asset_path,
                        AssetSyncEntry {
                            chunk_rel_path: rel_chunk_str,
                            asset_kind: "Shader".into(),
                            crc32: calculate_crc32(&payload),
                        },
                    ));
                }
            }

            // 12. XML DOCUMENTS
            if chunk_data.windows(5).any(|w| w == b"<?xml") {
                let out_name = build_asset_filename(&sniffed.display_name, &stem, "xml");
                let rel_asset_path = format!("assets/xml/{}", out_name);
                let abs_path = xml_dir.join(&out_name);

                let xml_payload = if chunk_data.len() > 4 {
                    let mut cur = Cursor::new(&chunk_data[0..4]);
                    let str_len = cur.read_u32::<LittleEndian>().unwrap_or(0) as usize;
                    if str_len + 4 <= chunk_data.len() {
                        &chunk_data[4..4 + str_len]
                    } else {
                        &chunk_data
                    }
                } else {
                    &chunk_data
                };

                if fs::write(&abs_path, xml_payload).is_ok() {
                    return Some((
                        rel_asset_path,
                        AssetSyncEntry {
                            chunk_rel_path: rel_chunk_str,
                            asset_kind: "Xml".into(),
                            crc32: calculate_crc32(xml_payload),
                        },
                    ));
                }
            }

            // 13. PARAMETERS & SCALARS
            // A. Check for 24-byte NamedAssetContainer slot (e.g. "17039")
            if chunk_data.len() == 24 && chunk_data.starts_with(&[3, 20, 0, 21]) {
                let slen = u32::from_le_bytes(chunk_data[7..11].try_into().unwrap_or_default()) as usize;
                if slen <= 13 && let Ok(slot_id) = std::str::from_utf8(&chunk_data[11..11 + slen]) {
                    let param_json = json!({
                        "type": "asset_group_slot",
                        "slot_id": slot_id.trim_matches(char::from(0)),
                    });
                    let out_name = format!("{}.json", stem);
                    let rel_asset_path = format!("assets/parameters/{}", out_name);
                    let abs_path = params_dir.join(&out_name);

                    if let Ok(json_str) = serde_json::to_string_pretty(&param_json)
                        && fs::write(&abs_path, json_str.as_bytes()).is_ok()
                    {
                        return Some((
                            rel_asset_path,
                            AssetSyncEntry {
                                chunk_rel_path: rel_chunk_str,
                                asset_kind: "Parameter".into(),
                                crc32: calculate_crc32(json_str.as_bytes()),
                            },
                        ));
                    }
                }
            }

            // B. Check for Sound Bank Label (57 00 00 04, e.g. "Archie SFX")
            if chunk_data.starts_with(b"\x57\x00\x00\x04")
                && chunk_data.len() >= 14
                && let Ok((_, sub_elem)) = parse_chunk_elements(&chunk_data[4..])
            {
                let mut sfx_label = String::new();
                for (sid, sval) in sub_elem {
                    if sid == 10 && sval.len() >= 4 {
                        let slen = u32::from_le_bytes(sval[0..4].try_into().unwrap_or_default()) as usize;
                        if slen <= sval.len() - 4 {
                            sfx_label = String::from_utf8_lossy(&sval[4..4 + slen]).trim_matches(char::from(0)).to_string();
                        }
                    }
                }
                if !sfx_label.is_empty() {
                    let param_json = json!({
                        "type": "sound_bank_descriptor",
                        "label": sfx_label
                    });
                    let out_name = format!("{}.json", stem);
                    let rel_asset_path = format!("assets/parameters/{}", out_name);
                    let abs_path = params_dir.join(&out_name);

                    if let Ok(json_str) = serde_json::to_string_pretty(&param_json)
                        && fs::write(&abs_path, json_str.as_bytes()).is_ok()
                    {
                        return Some((
                            rel_asset_path,
                            AssetSyncEntry {
                                chunk_rel_path: rel_chunk_str,
                                asset_kind: "Parameter".into(),
                                crc32: calculate_crc32(json_str.as_bytes()),
                            },
                        ));
                    }
                }
            }

            // C. Check for Counted String List / Collision References (e.g. .clb files in chunk_0033)
            if chunk_data.len() >= 8 {
                let count = u32::from_le_bytes(chunk_data[0..4].try_into().unwrap_or_default()) as usize;
                if count > 0 && count <= 64 {
                    let mut cur_pos = 4;
                    let mut parsed_strings = Vec::new();
                    let mut valid = true;

                    for _ in 0..count {
                        if cur_pos + 4 > chunk_data.len() {
                            valid = false;
                            break;
                        }
                        let slen = u32::from_le_bytes(chunk_data[cur_pos..cur_pos + 4].try_into().unwrap_or_default()) as usize;
                        cur_pos += 4;
                        if cur_pos + slen > chunk_data.len() {
                            valid = false;
                            break;
                        }
                        let s_slice = &chunk_data[cur_pos..cur_pos + slen];
                        if s_slice.iter().all(|&b| (0x20..=0x7E).contains(&b) || b == 0)
                            && let Ok(s) = std::str::from_utf8(s_slice)
                        {
                            parsed_strings.push(s.trim_matches(char::from(0)).to_string());
                            cur_pos += slen;
                        } else {
                            valid = false;
                            break;
                        }
                    }

                    if valid && cur_pos == chunk_data.len() && !parsed_strings.is_empty() {
                        let is_collision = parsed_strings.iter().any(|s| s.to_lowercase().ends_with(".clb"));
                        let param_json = json!({
                            "type": if is_collision { "collision_references" } else { "string_list" },
                            "files": parsed_strings
                        });

                        let out_name = format!("{}.json", stem);
                        let rel_asset_path = format!("assets/parameters/{}", out_name);
                        let abs_path = params_dir.join(&out_name);

                        if let Ok(json_str) = serde_json::to_string_pretty(&param_json)
                            && fs::write(&abs_path, json_str.as_bytes()).is_ok()
                        {
                            return Some((
                                rel_asset_path,
                                AssetSyncEntry {
                                    chunk_rel_path: rel_chunk_str,
                                    asset_kind: "Parameter".into(),
                                    crc32: calculate_crc32(json_str.as_bytes()),
                                },
                            ));
                        }
                    }
                }
            }

            // D. Scalars & String Parameters (<= 64 bytes)
            if chunk_data.len() <= 64 {
                let string_candidate = if chunk_data.len() >= 4 {
                    let slen = u32::from_le_bytes(chunk_data[0..4].try_into().unwrap_or_default()) as usize;
                    if (slen == chunk_data.len() - 4 || slen == chunk_data.len() - 5)
                        && chunk_data[4..4 + slen].iter().all(|&b| (0x20..=0x7E).contains(&b) || b == 0)
                    {
                        std::str::from_utf8(&chunk_data[4..4 + slen]).ok().map(|s| s.trim_matches(char::from(0)).to_string())
                    } else {
                        None
                    }
                } else {
                    None
                };

                let param_json = if let Some(text) = string_candidate {
                    json!({
                        "type": "string",
                        "value": text
                    })
                } else if chunk_data.len() == 4 {
                    let val_u32 = u32::from_le_bytes(chunk_data[0..4].try_into().unwrap_or_default());
                    let val_f32 = f32::from_le_bytes(chunk_data[0..4].try_into().unwrap_or_default());
                    json!({
                        "type": "scalar_32bit",
                        "uint_value": val_u32,
                        "float_value": if val_f32.is_finite() { val_f32 } else { 0.0 },
                        "hex": hex::encode_upper(&chunk_data)
                    })
                } else {
                    json!({
                        "type": "raw_bytes",
                        "size": chunk_data.len(),
                        "hex": hex::encode_upper(&chunk_data)
                    })
                };

                let out_name = format!("{}.json", stem);
                let rel_asset_path = format!("assets/parameters/{}", out_name);
                let abs_path = params_dir.join(&out_name);

                if let Ok(json_str) = serde_json::to_string_pretty(&param_json)
                    && fs::write(&abs_path, json_str.as_bytes()).is_ok()
                {
                    return Some((
                        rel_asset_path,
                        AssetSyncEntry {
                            chunk_rel_path: rel_chunk_str,
                            asset_kind: "Parameter".into(),
                            crc32: calculate_crc32(json_str.as_bytes()),
                        },
                    ));
                }
            }

            // 14. 100% CATCH-ALL: UNKNOWN BINARY BLOCKS WITH DOSSIER
            let out_name = format!("{}.bin", stem);
            let rel_asset_path = format!("assets/raw_chunks/{}", out_name);
            let abs_path = raw_dir.join(&out_name);

            if fs::write(&abs_path, &chunk_data).is_ok() {
                let dossier = build_unknown_chunk_dossier(&chunk_data, &stem);
                let meta_path = raw_dir.join(format!("{}.meta.json", stem));
                let _ = fs::write(meta_path, serde_json::to_string_pretty(&dossier).unwrap_or_default());

                return Some((
                    rel_asset_path,
                    AssetSyncEntry {
                        chunk_rel_path: rel_chunk_str,
                        asset_kind: "Raw".into(),
                        crc32: calculate_crc32(&chunk_data),
                    },
                ));
            }

            None
        })
        .collect();

    let mut cache = AssetSyncCache::default();
    for (rel_path, record) in sync_records {
        cache.entries.insert(rel_path, record);
    }

    let cache_file = project_dir.join(".asset_cache.json");
    let serialized = serde_json::to_string_pretty(&cache).map_err(|e| e.to_string())?;
    fs::write(cache_file, serialized).map_err(|e| e.to_string())?;

    Ok(cache.entries.len())
}

/// Checks the `assets/` workspace for user modifications and injects updated files back into `chunks/`.
pub fn sync_assets_to_chunks(project_dir: &Path) -> Result<usize, String> {
    let cache_file = project_dir.join(".asset_cache.json");
    if !cache_file.exists() {
        return Ok(0);
    }

    let cache_data = fs::read_to_string(&cache_file).map_err(|e| e.to_string())?;
    let mut cache: AssetSyncCache = serde_json::from_str(&cache_data).map_err(|e| e.to_string())?;

    let mut synced_count = 0;

    for (rel_asset_path, entry) in cache.entries.iter_mut() {
        let abs_asset_path = project_dir.join(rel_asset_path);
        let abs_chunk_path = project_dir.join(&entry.chunk_rel_path);

        if !abs_asset_path.exists() || !abs_chunk_path.exists() {
            continue;
        }

        let asset_bytes = match fs::read(&abs_asset_path) {
            Ok(b) => b,
            Err(_) => continue,
        };

        let current_crc = calculate_crc32(&asset_bytes);

        if current_crc != entry.crc32 {
            let chunk_bytes = fs::read(&abs_chunk_path).map_err(|e| e.to_string())?;

            let updated_chunk = match entry.asset_kind.as_str() {
                "Texture" => replace_texture_in_chunk(&chunk_bytes, &asset_bytes)?,
                "Audio" => replace_wav(&chunk_bytes, &asset_bytes)?,
                "Material" => {
                    let json_str = String::from_utf8(asset_bytes)
                        .map_err(|_| "Material JSON is not valid UTF-8".to_string())?;
                    import_material_from_json(&json_str)?
                }
                "Mesh" => {
                    if rel_asset_path.ends_with(".glb") {
                        import_glb_to_mesh(&chunk_bytes, &asset_bytes)?
                    } else {
                        let obj_str = String::from_utf8(asset_bytes)
                            .map_err(|_| "OBJ file is not valid UTF-8".to_string())?;
                        import_obj_to_mesh(&chunk_bytes, &obj_str)?
                    }
                }
                "Lua" => replace_lua_bytecode(&chunk_bytes, &asset_bytes)?,
                "Parameter" => {
                    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&asset_bytes) {
                        if (v["type"] == "collision_references" || v["type"] == "string_list")
                            && let Some(arr) = v["files"].as_array()
                        {
                            let mut buf = Vec::new();
                            buf.extend_from_slice(&(arr.len() as u32).to_le_bytes());
                            for item in arr {
                                let s = item.as_str().unwrap_or_default();
                                let s_bytes = s.as_bytes();
                                buf.extend_from_slice(&(s_bytes.len() as u32).to_le_bytes());
                                buf.extend_from_slice(s_bytes);
                            }
                            buf
                        } else if v["type"] == "asset_group_slot"
                            && let Some(slot_id) = v["slot_id"].as_str()
                        {
                            let s_bytes = slot_id.as_bytes();
                            let mut buf = Vec::with_capacity(7 + 4 + s_bytes.len() + 8);
                            buf.push(3);
                            buf.extend_from_slice(&[20, 0]);
                            buf.extend_from_slice(&[21, (4 + s_bytes.len()) as u8]);
                            buf.extend_from_slice(&[30, (4 + s_bytes.len() + 4) as u8]);
                            buf.extend_from_slice(&(s_bytes.len() as u32).to_le_bytes());
                            buf.extend_from_slice(s_bytes);
                            buf.extend_from_slice(&[1, 1, 0, 0]);
                            buf.extend_from_slice(&[1, 1, 0, 0]);
                            buf
                        } else if v["type"] == "string"
                            && let Some(s) = v["value"].as_str()
                        {
                            let mut buf = Vec::new();
                            let s_bytes = s.as_bytes();
                            buf.extend_from_slice(&(s_bytes.len() as u32).to_le_bytes());
                            buf.extend_from_slice(s_bytes);
                            buf
                        } else if v["type"] == "scalar_32bit"
                            && let Some(hex_str) = v["hex"].as_str()
                        {
                            hex::decode(hex_str).unwrap_or(chunk_bytes)
                        } else if let Some(hex_str) = v["hex"].as_str() {
                            hex::decode(hex_str).unwrap_or(chunk_bytes)
                        } else {
                            chunk_bytes
                        }
                    } else {
                        chunk_bytes
                    }
                }
                "Raw" | "Behavior" | "Attachment" => asset_bytes,
                _ => continue,
            };

            fs::write(&abs_chunk_path, updated_chunk).map_err(|e| e.to_string())?;
            entry.crc32 = current_crc;
            synced_count += 1;
        }
    }

    if synced_count > 0 {
        let serialized = serde_json::to_string_pretty(&cache).map_err(|e| e.to_string())?;
        fs::write(cache_file, serialized).map_err(|e| e.to_string())?;
    }

    Ok(synced_count)
}
