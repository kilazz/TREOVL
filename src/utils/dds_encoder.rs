use crate::utils::dds_decoder::TextureFormat;
use byteorder::{LittleEndian, WriteBytesExt};
use std::io::{Cursor, Write};

/// Generates a standard 128-byte DirectDraw Surface (DDS) header.
pub fn generate_dds_header(
    width: u32,
    height: u32,
    format: TextureFormat,
    mipmap_count: u32,
    is_cubemap: bool,
) -> Vec<u8> {
    let mut header = vec![0u8; 128];
    let mut cur = Cursor::new(&mut header);

    // Magic "DDS "
    cur.write_all(b"DDS ").unwrap();
    cur.write_u32::<LittleEndian>(124).unwrap(); // Header size

    let mut flags = 0x1 | 0x2 | 0x4 | 0x1000;
    if mipmap_count > 1 {
        flags |= 0x20000;
    }

    let is_compressed = format != TextureFormat::UncompressedRGBA;
    if is_compressed {
        flags |= 0x80000;
    } else {
        flags |= 0x8;
    }
    cur.write_u32::<LittleEndian>(flags).unwrap();
    cur.write_u32::<LittleEndian>(height).unwrap();
    cur.write_u32::<LittleEndian>(width).unwrap();

    let block_size = if format == TextureFormat::DXT1 { 8 } else { 16 };
    if is_compressed {
        let linear_size =
            std::cmp::max(1, width.div_ceil(4)) * std::cmp::max(1, height.div_ceil(4)) * block_size;
        cur.write_u32::<LittleEndian>(linear_size).unwrap();
    } else {
        cur.write_u32::<LittleEndian>(width * 4).unwrap();
    }

    cur.write_u32::<LittleEndian>(0).unwrap(); // Depth
    cur.write_u32::<LittleEndian>(mipmap_count).unwrap();

    cur.set_position(76);

    cur.write_u32::<LittleEndian>(32).unwrap();
    if is_compressed {
        cur.write_u32::<LittleEndian>(0x4).unwrap();
        let fourcc = match format {
            TextureFormat::DXT1 => b"DXT1",
            TextureFormat::DXT3 => b"DXT3",
            TextureFormat::DXT5 => b"DXT5",
            _ => b"DXT5",
        };
        cur.write_all(fourcc).unwrap();
    } else {
        cur.write_u32::<LittleEndian>(0x41).unwrap();
        cur.write_u32::<LittleEndian>(0).unwrap();
        cur.write_u32::<LittleEndian>(32).unwrap();
        cur.write_u32::<LittleEndian>(0x00FF0000).unwrap();
        cur.write_u32::<LittleEndian>(0x0000FF00).unwrap();
        cur.write_u32::<LittleEndian>(0x000000FF).unwrap();
        cur.write_u32::<LittleEndian>(0xFF000000).unwrap();
    }

    cur.set_position(108);

    // Caps1
    let mut caps1 = 0x1000;
    if mipmap_count > 1 {
        caps1 |= 0x8 | 0x400000;
    }
    if is_cubemap {
        caps1 |= 0x8;
    }
    cur.write_u32::<LittleEndian>(caps1).unwrap();

    // Caps2
    if is_cubemap {
        // Flags for all 6 faces of the cubemap
        cur.write_u32::<LittleEndian>(0x200 | 0x400 | 0x800 | 0x1000 | 0x2000 | 0x4000 | 0x8000)
            .unwrap();
    } else {
        cur.write_u32::<LittleEndian>(0).unwrap();
    }

    header
}
