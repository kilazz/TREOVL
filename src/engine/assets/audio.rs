use crate::engine::common::magic;
use anyhow::{Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;

/// Helper to parse the audio container header and locate the WAV payload marker.
fn locate_wav_marker(chunk_data: &[u8]) -> Result<(usize, usize, usize)> {
    if chunk_data.len() < 9 || &chunk_data[0..4] != magic::AUDIO_WAV {
        bail!("Not a valid audio container (missing audio magic)");
    }

    let num_offsets = chunk_data[4] as usize;
    let mut pos = 5;

    let mut last_val = 0;
    for _ in 0..num_offsets {
        if pos + 2 > chunk_data.len() {
            bail!("Corrupted audio offset table");
        }
        last_val = chunk_data[pos + 1] as usize;
        pos += 2;
    }

    if num_offsets == 0 {
        bail!("Audio container is empty");
    }

    let marker_pos = pos + last_val;
    if marker_pos + 9 > chunk_data.len() {
        bail!("Audio payload is truncated");
    }

    let mut cur = Cursor::new(&chunk_data[marker_pos + 5..marker_pos + 9]);
    let wav_size = cur.read_u32::<LittleEndian>()? as usize;
    let audio_start = marker_pos + 9;

    if audio_start + wav_size > chunk_data.len() {
        bail!("Invalid WAV payload size (exceeds chunk boundaries)");
    }

    Ok((marker_pos, audio_start, wav_size))
}

pub fn export_wav(chunk_data: &[u8]) -> Result<Vec<u8>> {
    let (_, audio_start, wav_size) = locate_wav_marker(chunk_data)?;
    Ok(chunk_data[audio_start..audio_start + wav_size].to_vec())
}

pub fn replace_wav(chunk_data: &[u8], wav_data: &[u8]) -> Result<Vec<u8>> {
    if wav_data.len() < 12 || &wav_data[0..4] != b"RIFF" {
        bail!("Invalid WAV file selected. Must be standard RIFF/WAV.");
    }

    let (marker_pos, audio_start, old_wav_size) = locate_wav_marker(chunk_data)?;
    let marker = &chunk_data[marker_pos..marker_pos + 5];

    let trailing_bytes = if audio_start + old_wav_size <= chunk_data.len() {
        &chunk_data[audio_start + old_wav_size..]
    } else {
        &[]
    };

    let mut new_chunk = chunk_data[..marker_pos].to_vec();
    new_chunk.extend_from_slice(marker);
    new_chunk.extend_from_slice(&(wav_data.len() as u32).to_le_bytes());
    new_chunk.extend_from_slice(wav_data);
    new_chunk.extend_from_slice(trailing_bytes);

    Ok(new_chunk)
}
