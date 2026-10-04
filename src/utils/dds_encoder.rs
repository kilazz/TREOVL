use crate::utils::dds_decoder::TextureFormat;

/// Generates a standard 128-byte DirectDraw Surface (DDS) header without unwrap() or Cursor allocations.
pub fn generate_dds_header(
    width: u32,
    height: u32,
    format: TextureFormat,
    mipmap_count: u32,
    is_cubemap: bool,
) -> Vec<u8> {
    let mut header = vec![0u8; 128];

    // Magic "DDS "
    header[0..4].copy_from_slice(b"DDS ");
    header[4..8].copy_from_slice(&124u32.to_le_bytes()); // dwSize

    let mut flags: u32 = 0x1 | 0x2 | 0x4 | 0x1000;
    if mipmap_count > 1 {
        flags |= 0x20000;
    }

    let is_compressed = format != TextureFormat::UncompressedRGBA;
    if is_compressed {
        flags |= 0x80000;
    } else {
        flags |= 0x8;
    }
    header[8..12].copy_from_slice(&flags.to_le_bytes());
    header[12..16].copy_from_slice(&height.to_le_bytes());
    header[16..20].copy_from_slice(&width.to_le_bytes());

    let block_size: u32 = if format == TextureFormat::DXT1 { 8 } else { 16 };
    let pitch_or_linear = if is_compressed {
        std::cmp::max(1, width.div_ceil(4)) * std::cmp::max(1, height.div_ceil(4)) * block_size
    } else {
        width * 4
    };
    header[20..24].copy_from_slice(&pitch_or_linear.to_le_bytes());
    header[28..32].copy_from_slice(&mipmap_count.to_le_bytes());

    // Pixel Format at offset 76
    header[76..80].copy_from_slice(&32u32.to_le_bytes());
    if is_compressed {
        header[80..84].copy_from_slice(&0x4u32.to_le_bytes());
        let fourcc = match format {
            TextureFormat::DXT1 => b"DXT1",
            TextureFormat::DXT3 => b"DXT3",
            TextureFormat::DXT5 => b"DXT5",
            _ => b"DXT5",
        };
        header[84..88].copy_from_slice(fourcc);
    } else {
        header[80..84].copy_from_slice(&0x41u32.to_le_bytes());
        header[88..92].copy_from_slice(&32u32.to_le_bytes());
        header[92..96].copy_from_slice(&0x00FF0000u32.to_le_bytes());
        header[96..100].copy_from_slice(&0x0000FF00u32.to_le_bytes());
        header[100..104].copy_from_slice(&0x000000FFu32.to_le_bytes());
        header[104..108].copy_from_slice(&0xFF000000u32.to_le_bytes());
    }

    // Caps1 at offset 108
    let mut caps1 = 0x1000u32;
    if mipmap_count > 1 {
        caps1 |= 0x8 | 0x400000;
    }
    if is_cubemap {
        caps1 |= 0x8;
    }
    header[108..112].copy_from_slice(&caps1.to_le_bytes());

    // Caps2 at offset 112 (Cubemap + all 6 face flags)
    if is_cubemap {
        let caps2: u32 = 0x200 | 0x400 | 0x800 | 0x1000 | 0x2000 | 0x4000 | 0x8000;
        header[112..116].copy_from_slice(&caps2.to_le_bytes());
    }

    header
}
