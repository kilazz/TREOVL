use byteorder::{LittleEndian, ReadBytesExt};
use serde_json::{Value, json};
use std::io::Cursor;

pub struct StrideAnalysis {
    pub detected_stride: usize,
    pub element_count: usize,
    pub pattern_type: &'static str,
    pub samples: Value,
}

pub fn analyze_stride_and_pattern(data: &[u8]) -> Option<StrideAnalysis> {
    if data.len() < 24 {
        return None;
    }

    let candidate_strides = [12, 16, 24, 32, 64];

    for &stride in &candidate_strides {
        if data.len().is_multiple_of(stride) {
            let count = data.len() / stride;
            if count < 2 {
                continue;
            }

            let mut cur = Cursor::new(data);
            let mut all_floats_valid = true;
            let mut sample_values = Vec::new();

            for _ in 0..count.min(3) {
                let mut elem_floats = Vec::new();
                for _ in 0..(stride / 4) {
                    if let Ok(f) = cur.read_f32::<LittleEndian>() {
                        if f.is_finite() && f.abs() < 100_000.0 {
                            elem_floats.push(f);
                        } else {
                            all_floats_valid = false;
                            break;
                        }
                    } else {
                        all_floats_valid = false;
                        break;
                    }
                }
                if !all_floats_valid {
                    break;
                }
                sample_values.push(elem_floats);
            }

            if all_floats_valid {
                let pattern_type = match stride {
                    12 => "Array of 3D Coordinates / Vector3",
                    16 => "Array of Vector4 / Quaternions / RGBA Colors",
                    24 => "Bounding Boxes (Min Vec3 + Max Vec3)",
                    32 => "Structured Vertex or Entity Records",
                    64 => "Array of 4x4 Transformation Matrices",
                    _ => "Regular Structured Array",
                };

                return Some(StrideAnalysis {
                    detected_stride: stride,
                    element_count: count,
                    pattern_type,
                    samples: json!(sample_values),
                });
            }
        }
    }

    None
}
