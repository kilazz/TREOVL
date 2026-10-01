use byteorder::{LittleEndian, ReadBytesExt};
use crc32fast::Hasher;
use std::io::Cursor;

pub const FOOTER_SIZE: usize = 16;
pub const MAGIC_FOOTER_1: u32 = 0xDEADBEEF;
pub const MAGIC_FOOTER_2: u32 = 0xFEEDDEAF;

#[derive(Debug)]
pub struct PrpFooter {
    #[allow(dead_code)]
    pub is_valid: bool,
    pub original_crc: u32,
    pub hash2: u32,
}

pub fn check_footer(data: &[u8]) -> Option<PrpFooter> {
    if data.len() < FOOTER_SIZE {
        return None;
    }

    let footer_start = data.len() - FOOTER_SIZE;
    let mut cur = Cursor::new(&data[footer_start..]);

    let m1 = cur.read_u32::<LittleEndian>().unwrap();
    let m2 = cur.read_u32::<LittleEndian>().unwrap();
    let original_crc = cur.read_u32::<LittleEndian>().unwrap();
    let hash2 = cur.read_u32::<LittleEndian>().unwrap();

    if m1 == MAGIC_FOOTER_1 && m2 == MAGIC_FOOTER_2 {
        Some(PrpFooter {
            is_valid: true,
            original_crc,
            hash2,
        })
    } else {
        None
    }
}

pub fn calculate_triumph_crc32(data_without_footer: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(data_without_footer);
    // Инверсия битов (Triumph Engine style)
    !hasher.finalize()
}
