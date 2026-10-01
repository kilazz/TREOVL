use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;

pub fn export_wav(chunk_data: &[u8]) -> Result<Vec<u8>, String> {
    if chunk_data.len() < 9 || &chunk_data[0..4] != b"\x00\x00\xA1\x00" {
        return Err("Not a valid audio container".into());
    }

    let num_offsets = chunk_data[4] as usize;
    let mut pos = 5;

    let mut offsets = Vec::with_capacity(num_offsets);
    for _ in 0..num_offsets {
        if pos + 2 > chunk_data.len() {
            return Err("Corrupted audio offset table".into());
        }
        let tid = chunk_data[pos];
        let val = chunk_data[pos + 1] as usize;
        offsets.push((tid, val));
        pos += 2;
    }

    let base_pos = pos;
    if offsets.is_empty() {
        return Err("Audio container is empty".into());
    }

    // Mathematically calculate the exact position of the WAV payload using the last offset
    let (_, last_val) = offsets.last().unwrap();
    let marker_pos = base_pos + last_val;

    if marker_pos + 9 > chunk_data.len() {
        return Err("Audio payload is truncated".into());
    }

    // [5 bytes marker] + [4 bytes size]
    let mut cur = Cursor::new(&chunk_data[marker_pos + 5..marker_pos + 9]);
    let engine_size = cur.read_u32::<LittleEndian>().unwrap() as usize;
    let audio_start = marker_pos + 9;

    if audio_start + engine_size > chunk_data.len() {
        return Err("Invalid WAV payload size".into());
    }

    Ok(chunk_data[audio_start..audio_start + engine_size].to_vec())
}

pub fn replace_wav(chunk_data: &[u8], wav_data: &[u8]) -> Result<Vec<u8>, String> {
    if wav_data.len() < 12 || &wav_data[0..4] != b"RIFF" {
        return Err("Invalid WAV file selected. Must be standard RIFF/WAV.".into());
    }

    if chunk_data.len() < 9 || &chunk_data[0..4] != b"\x00\x00\xA1\x00" {
        return Err("Original chunk is corrupted".into());
    }

    let num_offsets = chunk_data[4] as usize;
    let mut pos = 5;

    let mut offsets = Vec::with_capacity(num_offsets);
    for _ in 0..num_offsets {
        let tid = chunk_data[pos];
        let val = chunk_data[pos + 1] as usize;
        offsets.push((tid, val));
        pos += 2;
    }

    let base_pos = pos;
    let (_, last_val) = offsets.last().unwrap();
    let marker_pos = base_pos + last_val;

    let marker = &chunk_data[marker_pos..marker_pos + 5];

    let mut cur = Cursor::new(&chunk_data[marker_pos + 5..marker_pos + 9]);
    let old_wav_size = cur.read_u32::<LittleEndian>().unwrap() as usize;

    let audio_start = marker_pos + 9;
    let trailing_bytes = if audio_start + old_wav_size <= chunk_data.len() {
        &chunk_data[audio_start + old_wav_size..]
    } else {
        &[]
    };

    // Reconstruct the chunk: Old Header + Offset Table + New Payload + Old Trailing Bytes
    let mut new_chunk = chunk_data[..marker_pos].to_vec();
    new_chunk.extend_from_slice(marker);
    new_chunk.extend_from_slice(&(wav_data.len() as u32).to_le_bytes());
    new_chunk.extend_from_slice(wav_data);
    new_chunk.extend_from_slice(trailing_bytes);

    Ok(new_chunk)
}
