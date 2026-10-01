use flate2::Compression;
use flate2::read::{ZlibDecoder, ZlibEncoder};
use std::io::Read;

#[allow(dead_code)]
pub fn is_zlib_compressed(data: &[u8]) -> bool {
    if data.len() < 2 {
        return false;
    }
    matches!(
        &data[0..2],
        b"\x78\x9C" | b"\x78\xDA" | b"\x78\x01" | b"\x78\x5E"
    )
}

#[allow(dead_code)]
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = ZlibDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

#[allow(dead_code)]
pub fn compress(data: &[u8], level: u32) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = ZlibEncoder::new(data, Compression::new(level));
    let mut compressed = Vec::new();
    encoder.read_to_end(&mut compressed)?;
    Ok(compressed)
}
