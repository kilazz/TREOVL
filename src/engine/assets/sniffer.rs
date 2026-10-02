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
    UI = 6,
    Object = 7,
    Animation = 8,
    TerrainPalette = 9,
    Event = 10,
    Xml = 11,
    Parameter = 12,
    Vfx = 13,
    Font = 14,
    Behavior = 15,
    Attachment = 16,
    FaceFx = 17,
}

impl AssetKind {
    pub fn to_ui_kind_id(&self) -> i32 {
        match self {
            Self::Texture => 0,
            Self::Audio => 1,
            Self::Material => 2,
            Self::Mesh => 3,
            Self::Lua => 4,
            Self::UI => 6,
            Self::Object => 7,
            Self::Animation => 8,
            Self::TerrainPalette => 9,
            _ => 5,
        }
    }
}

pub struct SniffedAsset {
    pub kind: AssetKind,
    pub kind_name: &'static str,
    pub icon: &'static str,
    pub display_name: String,
}

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

    let (kind, kind_name, icon) = if magic_bytes == magic::TEX_3D {
        (AssetKind::Texture, "TRETexture (3D/2D)", "🎨")
    } else if magic_bytes == magic::TEX_CUBEMAP {
        (AssetKind::Texture, "TRECubeMap", "🌐")
    } else if magic_bytes == magic::TEX_INTERFACE {
        (AssetKind::Texture, "TREInterfaceImage (TGA)", "🖼️")
    } else if magic_bytes == magic::AUDIO_WAV {
        (AssetKind::Audio, "Sound / Voice (WAV)", "🎵")
    } else if magic_bytes == magic::MESH {
        (AssetKind::Mesh, "TREMeshResource", "🗿")
    } else if magic_bytes == b"\x41\x00\x41\x00" {
        (AssetKind::Mesh, "TREMeshGeometry", "📐")
    } else if magic_bytes == magic::ANIM_CLIP {
        (AssetKind::Animation, "Skeletal Animation Clip", "🎬")
    }
    // FaceFX Facial Animation Actors (0x0046BA00 or "FACE" signature)
    else if magic_bytes == b"FACE"
        || (data.len() >= 4 && data[0] == 0x00 && data[1] == 0xBA && data[2] == 0x46)
        || data.windows(4).any(|w| w == b"FACE")
    {
        (AssetKind::FaceFx, "FaceFX Facial Animation (.fxe)", "🗣️")
    } else if magic_bytes == magic::OBJECT
        || magic_bytes == b"\x21\x46\x46\x00"
        || magic_bytes == b"\x67\x00\x41\x00"
    {
        (AssetKind::Object, "TREModelResource (Entity/Prop)", "🧊")
    } else if magic_bytes == b"\x7E\x00\x00\x04" {
        (AssetKind::TerrainPalette, "Terrain Texture Palette", "🗺️")
    } else if data.len() >= 4 && data[2] == 0x73 && data[3] == 0x00 {
        (AssetKind::Vfx, "TREParticleSystem / VFX", "🔥")
    } else if magic_bytes == b"\x72\x00\x41\x00" {
        (AssetKind::Font, "TREFont / Sprite Sheet", "🔤")
    } else if magic_bytes == b"\x76\x00\x41\x00" {
        (AssetKind::UI, "UI Sprite Slice", "🖼️")
    } else if magic_bytes == b"\x77\x00\x41\x00" {
        (AssetKind::UI, "UI Control State", "🔘")
    } else if magic_bytes == b"\x71\x00\x41\x00" {
        (AssetKind::UI, "UI / Menu Layout", "🖥️")
    } else if magic_bytes == magic::EVENT || magic_bytes == b"\x0B\x01\x41\x00" {
        (AssetKind::Event, "Animation Event Marker", "👣")
    } else if magic_bytes == magic::LUA || data.windows(4).any(|w| w == magic::LUA) {
        if magic_bytes == magic::LUA {
            (AssetKind::Lua, "Lua 5.0.2 Bytecode", "📜")
        } else {
            (AssetKind::Lua, "Scripted Logic Node", "📜")
        }
    } else if magic_bytes == b"RIFF" {
        (AssetKind::Audio, "Raw WAV Audio", "🎵")
    } else if magic_bytes == b"DDS " {
        (AssetKind::Texture, "Raw DDS Texture", "🎨")
    } else if (data.len() >= 4 && data[2] == 0x41 && data[1] == 0x06)
        || (data.len() >= 4
            && (data[2] == 0x46 || (data[2] == 0x40 && (data[1] == 0x00 || data[1] == 0x59))))
    {
        (AssetKind::Material, "Shader Material (TREMaterial)", "🛠️")
    } else if magic_bytes == b"\x4E\x00\x41\x00" {
        (AssetKind::Generic, "Scene Node / Transform", "📍")
    } else if data.windows(5).any(|w| w == b"<?xml") {
        (AssetKind::Xml, "XML Document", "📋")
    } else if data.windows(4).any(|w| w == b".clb") {
        (AssetKind::Parameter, "Collision Bounds (.clb)", "🧱")
    } else if data.windows(4).any(|w| w == b"FACE")
        || data.windows(8).any(|w| w == b"Triumph ")
        || data.windows(6).any(|w| w == b"--Drop")
    {
        (AssetKind::Behavior, "Character AI & FaceFX", "🧠")
    } else if data.windows(5).any(|w| w == b"Plate")
        || data.windows(11).any(|w| w == b"plate_metal")
    {
        (AssetKind::Attachment, "Item Attachment Slot", "🍽️")
    } else if data.len() <= 64 {
        (AssetKind::Parameter, "Engine Parameter", "⚙️")
    } else {
        (AssetKind::Generic, "Triumph Binary Container", "📦")
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
                        || clean.ends_with(".fxe")
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
