use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use byteorder::{LittleEndian, ReadBytesExt};
use glam::Quat;
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use crate::engine::assets::parse_chunk_elements;
use crate::engine::common::{magic, read_length_prefixed_string};
use crate::engine::math::{BoneRotation, Vector3, Vector4};

#[inline]
pub(crate) fn sanitize_f32(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v } else { fallback }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyframeTranslation {
    pub time_seconds: f32,
    pub position: Vector3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyframeRotation {
    pub time_seconds: f32,
    pub rotation_euler: BoneRotation,
    pub rotation_quat: Vector4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoneTrack {
    pub bone_name: String,
    pub translations: Vec<KeyframeTranslation>,
    pub rotations: Vec<KeyframeRotation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationClip {
    pub name: String,
    pub target_rig: String,
    pub frame_rate: f32,
    pub duration_seconds: f32,
    pub bone_tracks: Vec<BoneTrack>,
}

#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct RawTranslationKey {
    micros: u32,
    px: f32,
    py: f32,
    pz: f32,
}

pub fn parse_animation_clip(chunk_data: &[u8]) -> Result<AnimationClip> {
    let payload = if chunk_data.len() > 4 && &chunk_data[0..4] == magic::ANIM_CLIP {
        &chunk_data[4..]
    } else {
        chunk_data
    };

    let (_, elements) = parse_chunk_elements(payload)?;

    let mut name = String::from("Unnamed_Animation");
    let mut target_rig = String::from("Generic_Rig");
    let mut frame_rate = 15.0f32;
    let mut duration_seconds = 1.0f32;
    let mut bone_tracks = Vec::new();

    for (id, chunk) in &elements {
        match *id {
            20 => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    target_rig = s;
                }
            }
            21 => {
                if let Some(s) = read_length_prefixed_string(chunk) {
                    name = s;
                }
            }
            30 if chunk.len() >= 4 => {
                let fps = Cursor::new(chunk)
                    .read_f32::<LittleEndian>()
                    .unwrap_or(15.0);
                if fps > 0.0 && fps < 240.0 {
                    frame_rate = fps;
                }
            }
            31 if chunk.len() >= 8 => {
                let duration_micros = Cursor::new(chunk)
                    .read_u64::<LittleEndian>()
                    .unwrap_or(1_000_000);
                duration_seconds = duration_micros as f32 / 1_000_000.0;
            }
            1 => {
                if let Ok((_, data_sub)) = parse_chunk_elements(chunk) {
                    for (sub_id, list_container) in data_sub {
                        if (sub_id == 10 || sub_id == 1)
                            && let Ok((_, bone_entries)) = parse_chunk_elements(&list_container)
                        {
                            for (_, bone_chunk) in bone_entries {
                                if let Ok(track) =
                                    parse_single_bone_track(&bone_chunk, duration_seconds)
                                {
                                    bone_tracks.push(track);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(AnimationClip {
        name,
        target_rig,
        frame_rate,
        duration_seconds,
        bone_tracks,
    })
}

fn parse_single_bone_track(data: &[u8], total_duration: f32) -> Result<BoneTrack> {
    let payload = if data.len() > 4 && &data[0..4] == magic::ANIM_TRACK {
        &data[4..]
    } else {
        data
    };

    let (_, elements) = parse_chunk_elements(payload)?;
    let mut bone_name = String::from("Unnamed_Bone");
    let mut translations = Vec::new();
    let mut rotations = Vec::new();

    for (id, chunk) in elements {
        match id {
            20 => {
                if let Some(s) = read_length_prefixed_string(&chunk) {
                    bone_name = s;
                }
            }
            22 | 24 => {
                if let Ok((_, trans_sub)) = parse_chunk_elements(&chunk) {
                    for (tid, tchunk) in trans_sub {
                        if tid == 22 || tid == 21 || tid == 0 {
                            parse_translation_blob(&tchunk, total_duration, &mut translations);
                        }
                    }
                } else {
                    parse_translation_blob(&chunk, total_duration, &mut translations);
                }
            }
            23 | 25 => {
                parse_rotation_container(&chunk, total_duration, &mut rotations);
            }
            _ => {}
        }
    }

    Ok(BoneTrack {
        bone_name,
        translations,
        rotations,
    })
}

fn parse_rotation_container(
    chunk: &[u8],
    total_duration: f32,
    rotations: &mut Vec<KeyframeRotation>,
) {
    if let Ok((_, rot_sub)) = parse_chunk_elements(chunk) {
        for (rid, rchunk) in rot_sub {
            if rid == 21 {
                if let Ok((_, sub_parts)) = parse_chunk_elements(&rchunk) {
                    let count = sub_parts
                        .iter()
                        .find(|(id, _)| *id == 22)
                        .and_then(|(_, d)| Cursor::new(d).read_u32::<LittleEndian>().ok())
                        .unwrap_or(0) as usize;

                    let keys_data = sub_parts
                        .iter()
                        .find(|(id, _)| *id == 23)
                        .map(|(_, d)| d.as_slice());
                    let signs_data = sub_parts
                        .iter()
                        .find(|(id, _)| *id == 24)
                        .map(|(_, d)| d.as_slice());

                    if let Some(data_blob) = keys_data {
                        parse_rotation_stream(
                            data_blob,
                            signs_data,
                            count,
                            total_duration,
                            rotations,
                        );
                        return;
                    }
                }
                parse_rotation_blob(&rchunk, total_duration, rotations);
            } else if rid == 22 || rid == 23 || rid == 0 {
                if let Ok((_, data_sub)) = parse_chunk_elements(&rchunk) {
                    for (did, dchunk) in data_sub {
                        if did == 23 || did == 22 || did == 0 {
                            parse_rotation_blob(&dchunk, total_duration, rotations);
                        }
                    }
                } else {
                    parse_rotation_blob(&rchunk, total_duration, rotations);
                }
            }
        }
    } else {
        parse_rotation_blob(chunk, total_duration, rotations);
    }
}

fn parse_rotation_stream(
    data: &[u8],
    signs: Option<&[u8]>,
    explicit_count: usize,
    total_duration: f32,
    rotations: &mut Vec<KeyframeRotation>,
) {
    let count = if explicit_count > 0 {
        explicit_count
    } else {
        data.len() / 6
    };

    if count == 0 {
        return;
    }

    if data.len() >= count * 6 {
        let mut previous_q: Option<Quat> = None;

        for i in 0..count {
            let chunk = &data[i * 6..(i + 1) * 6];
            let raw: [i16; 3] = bytemuck::pod_read_unaligned(chunk);

            let x = raw[0] as f32 / 32767.0;
            let y = raw[1] as f32 / 32767.0;
            let z = raw[2] as f32 / 32767.0;

            let len_sq = x * x + y * y + z * z;
            let mut w = if len_sq < 1.0 {
                (1.0 - len_sq).sqrt()
            } else {
                0.0
            };

            // Sign bitmask recovery for W component
            if let Some(sign_array) = signs {
                let byte_idx = i >> 3;
                let bit_idx = i & 7;
                if byte_idx < sign_array.len() && ((sign_array[byte_idx] >> bit_idx) & 1) != 0 {
                    w = -w;
                }
            }

            let mut q = Quat::from_xyzw(x, y, z, w).normalize();

            // Enforce shortest-path continuity to prevent 180-degree flips
            if let Some(prev) = previous_q
                && prev.dot(q) < 0.0
            {
                q = -q;
            }
            previous_q = Some(q);

            let time_seconds = if count > 1 {
                (i as f32 * total_duration) / (count - 1) as f32
            } else {
                0.0
            };

            rotations.push(KeyframeRotation {
                time_seconds,
                rotation_euler: BoneRotation::default(),
                rotation_quat: Vector4 {
                    x: q.x,
                    y: q.y,
                    z: q.z,
                    w: q.w,
                },
            });
        }
    }
}

fn parse_translation_blob(
    data: &[u8],
    total_duration: f32,
    translations: &mut Vec<KeyframeTranslation>,
) {
    let chunk_len = data.len();
    if chunk_len < 12 {
        return;
    }

    if chunk_len >= 16 && chunk_len.is_multiple_of(16) {
        let count = chunk_len / 16;
        for i in 0..count {
            let chunk = &data[i * 16..(i + 1) * 16];
            let raw: RawTranslationKey = bytemuck::pod_read_unaligned(chunk);
            let time_seconds = (raw.micros as f32 / 1_000_000.0).min(total_duration);
            translations.push(KeyframeTranslation {
                time_seconds,
                position: Vector3 {
                    x: sanitize_f32(raw.px, 0.0),
                    y: sanitize_f32(raw.py, 0.0),
                    z: sanitize_f32(raw.pz, 0.0),
                },
            });
        }
    } else if chunk_len >= 12 {
        let raw: [f32; 3] = bytemuck::pod_read_unaligned(&data[0..12]);
        translations.push(KeyframeTranslation {
            time_seconds: 0.0,
            position: Vector3 {
                x: sanitize_f32(raw[0], 0.0),
                y: sanitize_f32(raw[1], 0.0),
                z: sanitize_f32(raw[2], 0.0),
            },
        });
    }
}

fn parse_rotation_blob(data: &[u8], total_duration: f32, rotations: &mut Vec<KeyframeRotation>) {
    let chunk_len = data.len();
    if chunk_len < 6 {
        return;
    }

    if chunk_len.is_multiple_of(16) {
        let count = chunk_len / 16;
        for i in 0..count {
            let chunk = &data[i * 16..(i + 1) * 16];
            let q: [f32; 4] = bytemuck::pod_read_unaligned(chunk);
            let time_seconds = if count > 1 {
                i as f32 * total_duration / (count - 1) as f32
            } else {
                0.0
            };
            rotations.push(KeyframeRotation {
                time_seconds,
                rotation_euler: BoneRotation::default(),
                rotation_quat: Vector4 {
                    x: sanitize_f32(q[0], 0.0),
                    y: sanitize_f32(q[1], 0.0),
                    z: sanitize_f32(q[2], 0.0),
                    w: sanitize_f32(q[3], 1.0),
                },
            });
        }
    } else if chunk_len.is_multiple_of(12) {
        let count = chunk_len / 12;
        for i in 0..count {
            let chunk = &data[i * 12..(i + 1) * 12];
            let micros = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
            let q: [i16; 4] = bytemuck::pod_read_unaligned(&chunk[4..12]);
            let time_seconds = (micros as f32 / 1_000_000.0).min(total_duration);
            rotations.push(KeyframeRotation {
                time_seconds,
                rotation_euler: BoneRotation::default(),
                rotation_quat: Vector4 {
                    x: q[0] as f32 / 32767.0,
                    y: q[1] as f32 / 32767.0,
                    z: q[2] as f32 / 32767.0,
                    w: q[3] as f32 / 32767.0,
                },
            });
        }
    } else if chunk_len.is_multiple_of(10) {
        let count = chunk_len / 10;
        let mut previous_q: Option<Quat> = None;
        for i in 0..count {
            let chunk = &data[i * 10..(i + 1) * 10];
            let micros = u32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
            let raw: [i16; 3] = bytemuck::pod_read_unaligned(&chunk[4..10]);

            let x = raw[0] as f32 / 32767.0;
            let y = raw[1] as f32 / 32767.0;
            let z = raw[2] as f32 / 32767.0;
            let len_sq = x * x + y * y + z * z;
            let w = if len_sq < 1.0 {
                (1.0 - len_sq).sqrt()
            } else {
                0.0
            };

            let mut q = Quat::from_xyzw(x, y, z, w).normalize();

            if let Some(prev) = previous_q
                && prev.dot(q) < 0.0
            {
                q = -q;
            }
            previous_q = Some(q);

            let time_seconds = (micros as f32 / 1_000_000.0).min(total_duration);
            rotations.push(KeyframeRotation {
                time_seconds,
                rotation_euler: BoneRotation::default(),
                rotation_quat: Vector4 {
                    x: q.x,
                    y: q.y,
                    z: q.z,
                    w: q.w,
                },
            });
        }
    } else if chunk_len.is_multiple_of(8) {
        parse_rotation_stream(data, None, chunk_len / 8, total_duration, rotations);
    } else if chunk_len.is_multiple_of(6) {
        parse_rotation_stream(data, None, chunk_len / 6, total_duration, rotations);
    }
}
