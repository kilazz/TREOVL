use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Texture = 0,
    Audio = 1,
    Material = 2,
    Mesh = 3,
    Lua = 4,
    Generic = 5,
    Xml = 6,
    Animation = 7,
    Object = 8,
    Event = 9,
    Parameter = 10,
}

pub struct SniffedAsset {
    pub kind: AssetKind,
    pub kind_name: &'static str,
    pub icon: &'static str,
    pub display_name: String,
}

/// Sniffs the binary chunk to detect its engine asset type and extracts readable names.
pub fn sniff_asset(data: &[u8], filename_hint: &str) -> SniffedAsset {
    if data.len() < 4 {
        return SniffedAsset {
            kind: AssetKind::Parameter,
            kind_name: "Scalar Parameter",
            icon: "⚙️",
            display_name: filename_hint.to_string(),
        };
    }

    let magic = &data[0..4];

    // 1. High-Level Signature Check
    let (kind, kind_name, icon) = match magic {
        b"\x3D\x00\x41\x00" => (AssetKind::Texture, "Texture (DDS)", "🎨"),
        b"\x99\x00\x41\x00" => (AssetKind::Texture, "Cubemap (DDS)", "🌐"),
        b"\x98\x00\x41\x00" => (AssetKind::Texture, "Interface Image (TGA)", "🖼️"),
        b"\x00\x00\xA1\x00" => (AssetKind::Audio, "Sound / Voice (WAV)", "🎵"),
        b"\x35\x00\x41\x00" => (AssetKind::Mesh, "3D Mesh Geometry", "🗿"),
        b"\x05\x00\x41\x00" => (AssetKind::Animation, "Skeletal Animation", "🎬"),
        b"\x4B\x00\x41\x00" => (AssetKind::Object, "Object Entity", "🧊"),
        b"\xB0\x00\x00\x04" => (AssetKind::Event, "Animation Sound Event", "👣"),
        b"\x1bLua" => (AssetKind::Lua, "Lua 5.0 Bytecode", "📜"),
        b"RIFF" => (AssetKind::Audio, "Raw WAV Audio", "🎵"),
        b"DDS " => (AssetKind::Texture, "Raw DDS Texture", "🎨"),
        // Materials Type 1 through 13 & Overlord 2 PBR
        b"\x08\x06\x41\x00" | b"\x0A\x06\x41\x00" | b"\x0F\x06\x41\x00" | b"\x12\x06\x41\x00"
        | b"\x16\x06\x41\x00" | b"\x1B\x06\x41\x00" | b"\x20\x06\x41\x00" | b"\x24\x06\x41\x00"
        | b"\x26\x06\x41\x00" | b"\x28\x06\x41\x00" | b"\x2A\x06\x41\x00" | b"\x32\x06\x41\x00"
        | b"\x36\x06\x41\x00" => (AssetKind::Material, "Shader Material", "🛠️"),
        _ => {
            if data.windows(5).any(|w| w == b"<?xml") {
                (AssetKind::Xml, "XML Document", "📋")
            } else if data.windows(4).any(|w| w == b"\x24\x00\x41\x00") {
                (AssetKind::Texture, "Texture (MipMap)", "🎨")
            } else if data.len() <= 64 {
                (AssetKind::Parameter, "Engine Parameter", "⚙️")
            } else {
                (AssetKind::Generic, "Binary Chunk", "📦")
            }
        }
    };

    let extracted_name = extract_internal_strings(data);

    let display_name = match extracted_name {
        Some(name) => {
            if kind == AssetKind::Xml && !name.to_lowercase().ends_with(".xml") {
                format!("{}.xml ({})", name, filename_hint)
            } else {
                format!("{} ({})", name, filename_hint)
            }
        }
        None => filename_hint.to_string(),
    };

    SniffedAsset {
        kind,
        kind_name,
        icon,
        display_name,
    }
}

fn extract_internal_strings(data: &[u8]) -> Option<String> {
    let mut best_candidate = None;
    let mut cur = Cursor::new(data);

    while (cur.position() as usize) + 8 <= data.len() {
        if let Ok(len) = cur.read_u32::<LittleEndian>() {
            let len = len as usize;
            let pos = cur.position() as usize;

            if (3..=120).contains(&len) && pos + len <= data.len() {
                let slice = &data[pos..pos + len];
                if slice.iter().all(|&b| (0x20..=0x7E).contains(&b)) {
                    let clean = std::str::from_utf8(slice).unwrap_or_default().trim();
                    if clean.ends_with(".dds")
                        || clean.ends_with(".wav")
                        || clean.ends_with(".tga")
                        || clean.ends_with(".xml")
                    {
                        return Some(clean.to_string());
                    }
                    if (clean.starts_with('[') && clean.contains(']'))
                        || (best_candidate.is_none() && clean.len() >= 3)
                    {
                        best_candidate = Some(clean.to_string());
                    }
                }
            }
        }
        cur.set_position(cur.position() + 1);
    }

    best_candidate
}
