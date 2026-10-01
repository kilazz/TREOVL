use crate::engine::common::magic;
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
    Behavior = 10,
    Attachment = 11,
    Parameter = 12,
    UI = 13,
    Vfx = 14,
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

    let magic_bytes = &data[0..4];

    // 1. High-Level Signature Check
    let (kind, kind_name, icon) = if magic_bytes == magic::TEX_3D {
        (AssetKind::Texture, "Texture (DDS)", "🎨")
    } else if magic_bytes == magic::TEX_CUBEMAP {
        (AssetKind::Texture, "Cubemap (DDS)", "🌐")
    } else if magic_bytes == magic::TEX_INTERFACE {
        (AssetKind::Texture, "Interface Image (TGA)", "🖼️")
    } else if magic_bytes == magic::AUDIO_WAV {
        (AssetKind::Audio, "Sound / Voice (WAV)", "🎵")
    } else if magic_bytes == magic::MESH {
        (AssetKind::Mesh, "3D Mesh Geometry", "🗿")
    } else if magic_bytes == magic::ANIM_CLIP {
        (AssetKind::Animation, "Skeletal Animation", "🎬")
    } else if magic_bytes == magic::OBJECT {
        (AssetKind::Object, "3D Object Entity", "🧊")
    } else if data.len() >= 4 && data[2] == 0x73 && data[3] == 0x00 {
        // Triumph Particle / VFX System: 0x0073xxxx
        (AssetKind::Vfx, "Particle System / VFX", "🔥")
    } else if magic_bytes == b"\x76\x00\x41\x00" {
        (AssetKind::UI, "UI Sprite Slice", "🖼️")
    } else if magic_bytes == b"\x77\x00\x41\x00" {
        (AssetKind::UI, "UI Control State", "🔘")
    } else if magic_bytes == magic::EVENT || magic_bytes == b"\x04\x00\x00\xB0" {
        (AssetKind::Event, "Animation Sound Event", "👣")
    } else if magic_bytes == magic::LUA {
        (AssetKind::Lua, "Lua 5.0 Bytecode", "📜")
    } else if magic_bytes == b"RIFF" {
        (AssetKind::Audio, "Raw WAV Audio", "🎵")
    } else if magic_bytes == b"DDS " {
        (AssetKind::Texture, "Raw DDS Texture", "🎨")
    } else if (data.len() >= 4 && data[2] == 0x41 && data[1] == 0x06)
        || (data.len() >= 4 && data[3] == 0x00 && data[2] == 0x46)
    {
        (AssetKind::Material, "Shader Material", "🛠️")
    } else if data.len() >= 4 && data[2] == 0x71 && data[3] == 0x00 {
        (AssetKind::UI, "UI / Menu Layout", "🖥️")
    } else if data.windows(5).any(|w| w == b"<?xml") {
        (AssetKind::Xml, "XML Document", "📋")
    } else if data.windows(4).any(|w| w == b".clb") {
        (AssetKind::Parameter, "Collision Boundary Ref (.clb)", "🧱")
    } else if data.windows(4).any(|w| w == b"FACE")
        || data.windows(8).any(|w| w == b"Triumph ")
        || data.windows(6).any(|w| w == b"--Drop")
    {
        (AssetKind::Behavior, "Character AI & FaceFX", "🧠")
    } else if data.windows(5).any(|w| w == b"Plate")
        || data.windows(11).any(|w| w == b"plate_metal")
    {
        (AssetKind::Attachment, "Item Attachment Slot", "🍽️")
    } else if data.windows(4).any(|w| w == magic::TEX_MIPMAP) {
        (AssetKind::Texture, "Texture (MipMap)", "🎨")
    } else if data.len() <= 64 {
        (AssetKind::Parameter, "Engine Parameter", "⚙️")
    } else {
        (AssetKind::Generic, "Binary Chunk", "📦")
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
                        || clean.ends_with(".clb")
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
