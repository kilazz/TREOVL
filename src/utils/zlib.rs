#![allow(dead_code)]

use flate2::Compression;
use flate2::read::{ZlibDecoder, ZlibEncoder};
use std::io::Read;

/// Validates whether a byte slice has a compliant RFC-1950 ZLIB header.
/// Checks compression method, window size, and FCHECK divisibility by 31.
pub fn is_zlib_compressed(data: &[u8]) -> bool {
    if data.len() < 6 {
        return false;
    }
    let cmf = data[0];
    let flg = data[1];

    // CMF: Compression method must be Deflate (8) and window size CINFO <= 7
    if (cmf & 0x0F) != 8 || (cmf >> 4) > 7 {
        return false;
    }

    // FCHECK validation: (CMF * 256 + FLG) must be a multiple of 31
    if !(cmf as u16 * 256 + flg as u16).is_multiple_of(31) {
        return false;
    }

    // Preset dictionary (FDICT) flag is normally 0 for game archive data
    (flg & 0x20) == 0
}

pub fn decompress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = ZlibDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

pub fn compress(data: &[u8], level: u32) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = ZlibEncoder::new(data, Compression::new(level));
    let mut compressed = Vec::new();
    encoder.read_to_end(&mut compressed)?;
    Ok(compressed)
}
