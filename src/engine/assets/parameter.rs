use anyhow::{Context, Result};
use serde_json::json;

use crate::engine::assets::{
    build_chunk_from_elements_with_endian, build_typed_container_with_endian, parse_chunk_elements,
    parse_typed_container,
};
use crate::engine::common::{Endian, read_length_prefixed_string, write_length_prefixed_string};

pub fn export_parameter_to_json(data: &[u8], stem: &str) -> Result<String> {
    // 1. Parse File Lists / Collision Reference Tables (.clb)
    if data.len() >= 8 && is_string_list_container(data) {
        let files = extract_string_list(data);
        let param_json = json!({
            "type": "file_list",
            "count": files.len(),
            "files": files
        });
        return serde_json::to_string_pretty(&param_json)
            .context(format!("Failed to format parameter JSON for {}", stem));
    }

    // 2. Parse Asset Slot & Sound Bank Descriptors (e.g. Chunk 1881 "39440" + "jester")
    if let Ok((_, root_elements)) = parse_chunk_elements(data) {
        let mut slot_tag = None;
        let mut flags_hex = None;
        let mut sound_bank = None;

        for (id, chunk) in root_elements {
            match id {
                20 => {
                    slot_tag = read_length_prefixed_string(&chunk);
                }
                21 => {
                    flags_hex = Some(hex::encode_upper(&chunk));
                }
                30 => {
                    // Try parsing sub-container holding 0x04000057 Sound Bank
                    if let Ok((_, sub_parts)) = parse_chunk_elements(&chunk) {
                        for (_, pdata) in sub_parts {
                            if let Ok((type_id, fields)) = parse_typed_container(&pdata)
                                && type_id == 0x04000057
                            {
                                for (fid, fdata) in fields {
                                    if fid == 10 {
                                        sound_bank = read_length_prefixed_string(&fdata);
                                    }
                                }
                            }
                        }
                    } else if chunk.starts_with(b"\x57\x00\x00\x04")
                        && let Ok((_, fields)) = parse_typed_container(&chunk)
                    {
                        for (fid, fdata) in fields {
                            if fid == 10 {
                                sound_bank = read_length_prefixed_string(&fdata);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if let Some(tag) = slot_tag {
            let mut out = json!({
                "type": "asset_slot_descriptor",
                "slot_tag": tag,
                "flags_hex": flags_hex.unwrap_or_else(|| "01010000".into()),
            });
            if let Some(sb) = sound_bank {
                out["sound_bank"] = json!(sb);
            }
            return serde_json::to_string_pretty(&out)
                .context(format!("Failed to format parameter JSON for {}", stem));
        }
    }

    // 3. Direct Sound Bank Descriptors (0x04000057)
    if data.starts_with(b"\x57\x00\x00\x04") && data.len() >= 14 {
        let mut sfx_label = String::new();
        if let Ok((_, sub_elem)) = parse_chunk_elements(&data[4..]) {
            for (sid, sval) in sub_elem {
                if sid == 10
                    && let Some(s) = read_length_prefixed_string(&sval)
                {
                    sfx_label = s;
                }
            }
        }
        let param_json = json!({
            "type": "sound_bank_descriptor",
            "label": sfx_label
        });
        return serde_json::to_string_pretty(&param_json)
            .context(format!("Failed to format parameter JSON for {}", stem));
    }

    // 4. Scalar 32-bit (Float vs UInt)
    if data.len() == 4 {
        let val_u32 = u32::from_le_bytes(data[0..4].try_into().unwrap_or_default());
        let val_f32 = f32::from_le_bytes(data[0..4].try_into().unwrap_or_default());

        let is_clean_float = val_f32.is_finite()
            && !val_f32.is_nan()
            && val_f32.abs() >= 1e-4
            && val_f32.abs() <= 100_000.0;

        let param_json = if is_clean_float {
            json!({
                "type": "scalar_32bit",
                "uint_value": val_u32,
                "float_value": val_f32,
            })
        } else {
            json!({
                "type": "scalar_32bit",
                "uint_value": val_u32,
            })
        };
        return serde_json::to_string_pretty(&param_json)
            .context(format!("Failed to format parameter JSON for {}", stem));
    }

    // 5. Scalar 16-bit
    if data.len() == 2 {
        let val_u16 = u16::from_le_bytes(data[0..2].try_into().unwrap_or_default());
        let param_json = json!({
            "type": "scalar_16bit",
            "uint_value": val_u16,
        });
        return serde_json::to_string_pretty(&param_json)
            .context(format!("Failed to format parameter JSON for {}", stem));
    }

    // 6. Scalar 8-bit
    if data.len() == 1 {
        let param_json = json!({
            "type": "scalar_8bit",
            "uint_value": data[0],
        });
        return serde_json::to_string_pretty(&param_json)
            .context(format!("Failed to format parameter JSON for {}", stem));
    }

    // 7. Plaintext string
    if let Some(text) = read_length_prefixed_string(data) {
        let param_json = json!({ "type": "string", "value": text });
        return serde_json::to_string_pretty(&param_json)
            .context(format!("Failed to format parameter JSON for {}", stem));
    }

    // 8. Raw fallback for arbitrary byte arrays
    let param_json = json!({
        "type": "raw_bytes",
        "size": data.len(),
        "hex": hex::encode_upper(data)
    });
    serde_json::to_string_pretty(&param_json)
        .context(format!("Failed to format parameter JSON for {}", stem))
}

pub fn import_parameter_from_json(json_bytes: &[u8], baseline_chunk: &[u8]) -> Result<Vec<u8>> {
    let v: serde_json::Value = serde_json::from_slice(json_bytes)?;
    let p_type = v["type"].as_str().unwrap_or("raw_bytes");
    let endian = Endian::Little;

    match p_type {
        "asset_slot_descriptor" => {
            let slot_tag = v["slot_tag"].as_str().unwrap_or("0");
            let flags_hex = v["flags_hex"].as_str().unwrap_or("01010000");
            let flags_bytes = hex::decode(flags_hex).unwrap_or_else(|_| vec![1, 1, 0, 0]);

            let chunk_30 = if let Some(sb) = v["sound_bank"].as_str() {
                let sound_elems = vec![
                    (10, write_length_prefixed_string(sb)),
                    (11, vec![0, 0, 0, 0]),
                ];
                let sound_typed =
                    build_typed_container_with_endian(0x04000057, &sound_elems, endian);
                build_chunk_from_elements_with_endian(true, &[(0, sound_typed)], endian)
            } else {
                vec![1, 1, 0, 0]
            };

            let root_elements = vec![
                (20, write_length_prefixed_string(slot_tag)),
                (21, flags_bytes),
                (30, chunk_30),
            ];

            Ok(build_chunk_from_elements_with_endian(
                false,
                &root_elements,
                endian,
            ))
        }
        "asset_group_slot" => {
            if let Some(slot_id) = v["slot_id"].as_str() {
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
                Ok(buf)
            } else {
                Ok(baseline_chunk.to_vec())
            }
        }
        "file_list" => {
            if let Some(files) = v["files"].as_array() {
                let mut buf = Vec::new();
                buf.extend_from_slice(&(files.len() as u32).to_le_bytes());
                for f in files {
                    if let Some(s) = f.as_str() {
                        let s_bytes = s.as_bytes();
                        buf.extend_from_slice(&(s_bytes.len() as u32).to_le_bytes());
                        buf.extend_from_slice(s_bytes);
                    }
                }
                Ok(buf)
            } else {
                Ok(baseline_chunk.to_vec())
            }
        }
        "sound_bank_descriptor" => {
            if let Some(label) = v["label"].as_str() {
                let sound_elems = vec![
                    (10, write_length_prefixed_string(label)),
                    (11, vec![0, 0, 0, 0]),
                ];
                Ok(build_typed_container_with_endian(
                    0x04000057,
                    &sound_elems,
                    endian,
                ))
            } else {
                Ok(baseline_chunk.to_vec())
            }
        }
        "string" => {
            if let Some(s) = v["value"].as_str() {
                let mut buf = Vec::new();
                let s_bytes = s.as_bytes();
                buf.extend_from_slice(&(s_bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(s_bytes);
                Ok(buf)
            } else {
                Ok(baseline_chunk.to_vec())
            }
        }
        "scalar_32bit" => {
            let u = v["uint_value"].as_u64().unwrap_or(0) as u32;
            Ok(u.to_le_bytes().to_vec())
        }
        "scalar_16bit" => {
            let u = v["uint_value"].as_u64().unwrap_or(0) as u16;
            Ok(u.to_le_bytes().to_vec())
        }
        "scalar_8bit" => {
            let u = v["uint_value"].as_u64().unwrap_or(0) as u8;
            Ok(vec![u])
        }
        _ => {
            if let Some(hex_str) = v["hex"].as_str() {
                Ok(hex::decode(hex_str).unwrap_or_else(|_| baseline_chunk.to_vec()))
            } else {
                Ok(baseline_chunk.to_vec())
            }
        }
    }
}

fn is_string_list_container(data: &[u8]) -> bool {
    let count = u32::from_le_bytes(data[0..4].try_into().unwrap_or_default()) as usize;
    if count == 0 || count > 100 {
        return false;
    }

    let mut pos = 4;
    for _ in 0..count {
        if pos + 4 > data.len() {
            return false;
        }
        let len = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap_or_default()) as usize;
        pos += 4;
        if len == 0 || pos + len > data.len() {
            return false;
        }
        let slice = &data[pos..pos + len];
        if !slice.iter().all(|&b| (0x20..=0x7E).contains(&b)) {
            return false;
        }
        pos += len;
    }
    pos == data.len()
}

fn extract_string_list(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let count = u32::from_le_bytes(data[0..4].try_into().unwrap_or_default()) as usize;
    let mut pos = 4;

    for _ in 0..count {
        if pos + 4 > data.len() {
            break;
        }
        let len = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap_or_default()) as usize;
        pos += 4;
        if pos + len > data.len() {
            break;
        }
        if let Ok(s) = std::str::from_utf8(&data[pos..pos + len]) {
            out.push(s.to_string());
        }
        pos += len;
    }
    out
}
