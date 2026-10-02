use anyhow::{Context, Result};
use serde_json::json;

use crate::engine::assets::parse_chunk_elements;
use crate::engine::common::read_length_prefixed_string;

pub fn export_parameter_to_json(data: &[u8], stem: &str) -> Result<String> {
    let param_json = if data.len() == 24 && data.starts_with(&[3, 20, 0, 21]) {
        let slen = u32::from_le_bytes(data[7..11].try_into().unwrap_or_default()) as usize;
        if slen <= 13
            && let Ok(slot_id) = std::str::from_utf8(&data[11..11 + slen])
        {
            json!({
                "type": "asset_group_slot",
                "slot_id": slot_id.trim_matches(char::from(0)),
            })
        } else {
            json!({ "type": "raw_bytes", "hex": hex::encode_upper(data) })
        }
    } else if data.starts_with(b"\x57\x00\x00\x04") && data.len() >= 14 {
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
        json!({
            "type": "sound_bank_descriptor",
            "label": sfx_label
        })
    } else if data.len() == 4 {
        let val_u32 = u32::from_le_bytes(data[0..4].try_into().unwrap_or_default());
        let val_f32 = f32::from_le_bytes(data[0..4].try_into().unwrap_or_default());
        json!({
            "type": "scalar_32bit",
            "uint_value": val_u32,
            "float_value": if val_f32.is_finite() { val_f32 } else { 0.0 },
            "hex": hex::encode_upper(data)
        })
    } else if let Some(text) = read_length_prefixed_string(data) {
        json!({ "type": "string", "value": text })
    } else {
        json!({ "type": "raw_bytes", "size": data.len(), "hex": hex::encode_upper(data) })
    };

    serde_json::to_string_pretty(&param_json)
        .context(format!("Failed to format parameter JSON for {}", stem))
}

pub fn import_parameter_from_json(json_bytes: &[u8], baseline_chunk: &[u8]) -> Result<Vec<u8>> {
    let v: serde_json::Value = serde_json::from_slice(json_bytes)?;

    if v["type"] == "asset_group_slot"
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
        Ok(buf)
    } else if v["type"] == "string"
        && let Some(s) = v["value"].as_str()
    {
        let mut buf = Vec::new();
        let s_bytes = s.as_bytes();
        buf.extend_from_slice(&(s_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(s_bytes);
        Ok(buf)
    } else if v["type"] == "scalar_32bit"
        && let Some(hex_str) = v["hex"].as_str()
    {
        Ok(hex::decode(hex_str).unwrap_or_else(|_| baseline_chunk.to_vec()))
    } else if let Some(hex_str) = v["hex"].as_str() {
        Ok(hex::decode(hex_str).unwrap_or_else(|_| baseline_chunk.to_vec()))
    } else {
        Ok(baseline_chunk.to_vec())
    }
}
