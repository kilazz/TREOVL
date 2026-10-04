use anyhow::{Context, Result};
use crc32fast::Hasher;
use flate2::Compression;
use flate2::write::ZlibEncoder;
use std::io::Write;

fn write_png_chunk(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(data);

    let mut hasher = Hasher::new();
    hasher.update(tag);
    hasher.update(data);
    let crc = hasher.finalize();
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Encodes an RGBA8 pixel buffer into a standard, compliant PNG byte stream.
pub fn encode_rgba_to_png(width: u32, height: u32, rgba_pixels: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    // Standard PNG header signature
    out.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    // 1. IHDR Chunk (13 bytes)
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // 8 bits per channel
    ihdr.push(6); // Color type 6: RGBA with Alpha
    ihdr.push(0); // Compression: Deflate
    ihdr.push(0); // Filter method: Standard
    ihdr.push(0); // Interlace: None
    write_png_chunk(&mut out, b"IHDR", &ihdr);

    // 2. IDAT Chunk (Scanlines filtered with filter-type 0 = None)
    let row_len = (width * 4) as usize;
    let mut raw_filtered = Vec::with_capacity((row_len + 1) * height as usize);
    for y in 0..height as usize {
        raw_filtered.push(0); // Filter byte: 0 (None)
        let start = y * row_len;
        let end = (start + row_len).min(rgba_pixels.len());
        if start < rgba_pixels.len() {
            raw_filtered.extend_from_slice(&rgba_pixels[start..end]);
        }
    }

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(&raw_filtered)
        .context("Failed to compress PNG image scanlines")?;
    let compressed_idat = encoder
        .finish()
        .context("Failed to finalize PNG Zlib compression")?;
    write_png_chunk(&mut out, b"IDAT", &compressed_idat);

    // 3. IEND Chunk
    write_png_chunk(&mut out, b"IEND", &[]);

    Ok(out)
}
