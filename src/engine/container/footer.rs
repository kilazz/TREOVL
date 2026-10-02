use crate::engine::common::Endian;
pub use crate::engine::common::calculate_triumph_crc32;

pub const FOOTER_SIZE: usize = 16;
pub const MAGIC_FOOTER_1: u32 = 0xDEADBEEF;
pub const MAGIC_FOOTER_2: u32 = 0xFEEDDEAF;

#[derive(Debug)]
pub struct PrpFooter {
    pub original_crc: u32,
    pub hash2: u32,
    pub endian: Endian,
}

pub fn check_footer(data: &[u8]) -> Option<PrpFooter> {
    if data.len() < FOOTER_SIZE {
        return None;
    }

    let footer_start = data.len() - FOOTER_SIZE;
    let slice = &data[footer_start..];

    // Check Little-Endian (PC)
    let m1_le = u32::from_le_bytes(slice[0..4].try_into().unwrap_or_default());
    let m2_le = u32::from_le_bytes(slice[4..8].try_into().unwrap_or_default());
    if m1_le == MAGIC_FOOTER_1 && m2_le == MAGIC_FOOTER_2 {
        let original_crc = u32::from_le_bytes(slice[8..12].try_into().unwrap_or_default());
        let hash2 = u32::from_le_bytes(slice[12..16].try_into().unwrap_or_default());
        return Some(PrpFooter {
            original_crc,
            hash2,
            endian: Endian::Little,
        });
    }

    // Check Big-Endian (Xbox 360 / PS3)
    let m1_be = u32::from_be_bytes(slice[0..4].try_into().unwrap_or_default());
    let m2_be = u32::from_be_bytes(slice[4..8].try_into().unwrap_or_default());
    if m1_be == MAGIC_FOOTER_1 && m2_be == MAGIC_FOOTER_2 {
        let original_crc = u32::from_be_bytes(slice[8..12].try_into().unwrap_or_default());
        let hash2 = u32::from_be_bytes(slice[12..16].try_into().unwrap_or_default());
        return Some(PrpFooter {
            original_crc,
            hash2,
            endian: Endian::Big,
        });
    }

    None
}
