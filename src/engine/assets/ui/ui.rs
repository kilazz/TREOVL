use anyhow::{Result, bail};
use byteorder::{LittleEndian, WriteBytesExt};
use serde_json::{Value, json};

use crate::engine::assets::{
    build_chunk_from_elements, build_typed_container, parse_chunk_elements, parse_typed_container,
};

pub fn export_ui_to_json(data: &[u8]) -> Result<String> {
    let root_node = decode_ui_node(data);
    serde_json::to_string_pretty(&root_node).map_err(|e| anyhow::anyhow!(e))
}

pub fn import_ui_from_json(json_str: &str) -> Result<Vec<u8>> {
    let val: Value = serde_json::from_str(json_str)?;
    encode_ui_node(&val)
}

fn decode_ui_node(data: &[u8]) -> Value {
    if data.is_empty() {
        return json!({ "type": "empty" });
    }

    // 1. Root Typed UI Container (TypeID: 0x0071xxxx, 0x00410076, 0x00410077)
    if data.len() >= 5
        && let Ok((type_id, elements)) = parse_typed_container(data)
    {
        let children: Vec<Value> = elements
            .into_iter()
            .map(|(id, child_data)| {
                json!({
                    "id": id,
                    "data": decode_ui_node(&child_data)
                })
            })
            .collect();

        return json!({
            "type": "typed_container",
            "type_id_hex": format!("{:08X}", type_id),
            "children": children
        });
    }

    // 2. Sub-containers (Windows, Panels, Lists, State blocks)
    if let Ok((has_magic, elements)) = parse_chunk_elements(data)
        && !elements.is_empty()
    {
        let children: Vec<Value> = elements
            .into_iter()
            .map(|(id, child_data)| {
                json!({
                    "id": id,
                    "data": decode_ui_node(&child_data)
                })
            })
            .collect();

        return json!({
            "type": "container",
            "has_magic": has_magic,
            "children": children
        });
    }

    // 3. String parameter (Length + ASCII + Null terminator)
    if data.len() >= 5 {
        let len = u32::from_le_bytes(data[0..4].try_into().unwrap_or_default()) as usize;
        if len > 0 && (len == data.len() - 4 || len == data.len() - 5) {
            let text_end = if data.last() == Some(&0) {
                data.len() - 1
            } else {
                data.len()
            };
            let s_bytes = &data[4..text_end];
            if s_bytes
                .iter()
                .all(|&b| (0x20..=0x7E).contains(&b) || b == b'\n' || b == b'\r' || b == b'\t')
                && let Ok(s) = std::str::from_utf8(s_bytes)
            {
                return json!({
                    "type": "string",
                    "value": s
                });
            }
        }
    }

    // 4. Raw Hex fallback for numeric bounds, anchors and colors
    json!({
        "type": "raw",
        "hex": hex::encode_upper(data)
    })
}

fn encode_ui_node(val: &Value) -> Result<Vec<u8>> {
    let node_type = val["type"].as_str().unwrap_or("raw");

    match node_type {
        "typed_container" => {
            let hex_id = val["type_id_hex"].as_str().unwrap_or("00710023");
            let type_id = u32::from_str_radix(hex_id, 16)?;
            let children = val["children"].as_array().cloned().unwrap_or_default();
            let mut elements = Vec::new();
            for child in children {
                let id = child["id"].as_u64().unwrap_or(0) as u32;
                let child_data = encode_ui_node(&child["data"])?;
                elements.push((id, child_data));
            }
            Ok(build_typed_container(type_id, &elements))
        }
        "container" => {
            let has_magic = val["has_magic"].as_bool().unwrap_or(false);
            let children = val["children"].as_array().cloned().unwrap_or_default();
            let mut elements = Vec::new();
            for child in children {
                let id = child["id"].as_u64().unwrap_or(0) as u32;
                let child_data = encode_ui_node(&child["data"])?;
                elements.push((id, child_data));
            }
            Ok(build_chunk_from_elements(has_magic, &elements))
        }
        "string" => {
            let s = val["value"].as_str().unwrap_or("");
            let s_bytes = s.as_bytes();
            let mut out = Vec::with_capacity(4 + s_bytes.len() + 1);
            out.write_u32::<LittleEndian>((s_bytes.len() + 1) as u32)?;
            out.extend_from_slice(s_bytes);
            out.push(0);
            Ok(out)
        }
        "empty" => Ok(Vec::new()),
        "raw" => {
            let hex_str = val["hex"].as_str().unwrap_or("");
            Ok(hex::decode(hex_str)?)
        }
        _ => bail!("Unknown UI node type: {}", node_type),
    }
}
