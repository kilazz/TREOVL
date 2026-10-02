use anyhow::{Context, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CollisionJson {
    pub _engine_metadata: CollisionEngineMetadataJson,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub shapes: Vec<CollisionShapeJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CollisionEngineMetadataJson {
    pub file_type: String,
    pub size: usize,
    pub hex: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CollisionShapeJson {
    pub shape_type: String, // "box_oriented", "sphere", "capsule", "raw"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub half_extents: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    pub hex: String,
}

pub fn export_collision_to_json(data: &[u8], _stem: &str) -> Result<String> {
    let mut shapes = Vec::new();
    let mut cur = Cursor::new(data);

    while (cur.position() as usize) + 60 <= data.len() {
        let pos = cur.position() as usize;
        let mut valid = true;
        let mut buf = [0f32; 15];
        let mut temp_cur = Cursor::new(&data[pos..pos + 60]);

        for v in &mut buf {
            if let Ok(f) = temp_cur.read_f32::<LittleEndian>() {
                if !f.is_finite() || f.abs() > 100_000.0 {
                    valid = false;
                    break;
                }
                *v = f;
            } else {
                valid = false;
                break;
            }
        }

        if valid {
            shapes.push(CollisionShapeJson {
                shape_type: "box_oriented".into(),
                center: Some([buf[12], buf[13], buf[14]]),
                half_extents: Some([buf[9], buf[10], buf[11]]),
                radius: None,
                hex: hex::encode_upper(&data[pos..pos + 60]),
            });
            cur.set_position((pos + 60) as u64);
        } else {
            break;
        }
    }

    let metadata = CollisionEngineMetadataJson {
        file_type: "CollisionBoundary (.clb)".into(),
        size: data.len(),
        hex: hex::encode_upper(data),
    };

    let col_json = CollisionJson {
        _engine_metadata: metadata,
        shapes,
    };

    serde_json::to_string_pretty(&col_json).context("Failed to format Collision JSON")
}

pub fn import_collision_from_json(json_str: &str, baseline: &[u8]) -> Result<Vec<u8>> {
    let parsed: CollisionJson = serde_json::from_str(json_str)?;

    // If the user modified the raw hex block directly inside the JSON
    if !parsed._engine_metadata.hex.is_empty()
        && let Ok(bytes) = hex::decode(&parsed._engine_metadata.hex)
    {
        return Ok(bytes);
    }

    Ok(baseline.to_vec())
}
