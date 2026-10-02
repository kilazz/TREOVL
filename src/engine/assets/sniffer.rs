use super::parse_typed_container;
use crate::engine::common::magic;

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
    Character = 18,
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
            Self::Object | Self::Character | Self::Attachment => 7,
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

    // Priority 1: Scene Graph Objects, 3D Models & Entities (MUST check first by magic 0x0041004B)
    let (kind, kind_name, icon) = if magic_bytes == magic::OBJECT
        || magic_bytes == b"\x21\x46\x46\x00"
        || magic_bytes == b"\x67\x00\x41\x00"
    {
        (AssetKind::Object, "TREModelResource (Entity/Prop)", "🧊")

    // Priority 2: Character / Actor Controllers (STRUCTURAL VALIDATION)
    } else if data.len() >= 4 && data[0] == 0x03 && data[1] == 0x40 && data[2] == 0x46 {
        let mut is_valid = false;
        // Verify mathematical structure to prevent false positives
        if let Ok((_, elements)) = parse_typed_container(data) {
            // Must contain AI (112), Animations (50), Lua script (115), or Skeleton (33)
            is_valid = elements
                .iter()
                .any(|(id, _)| *id == 115 || *id == 112 || *id == 50 || *id == 33);
        }
        if is_valid {
            (
                AssetKind::Character,
                "Character / NPC Controller (TREActor)",
                "🧙‍♂️",
            )
        } else {
            (AssetKind::Generic, "Triumph Binary Container", "📦")
        }

    // Priority 3: Attached Items / Equipment (STRUCTURAL VALIDATION)
    } else if data.windows(4).any(|w| w == b"\x0D\x20\x46\x00") {
        let mut is_valid = false;
        if let Ok((_, elements)) = parse_typed_container(data) {
            // Item equipment MUST link to an active 3D Mesh (contains "OBJ\" or "MESH\")
            is_valid = elements
                .iter()
                .any(|(_, chunk)| chunk.windows(4).any(|w| w == b"OBJ\\" || w == b"MESH"));
        }
        if is_valid {
            (
                AssetKind::Attachment,
                "Attached Item / Prop (TREItemResource)",
                "🍽️",
            )
        } else {
            (AssetKind::Generic, "Triumph Binary Container", "📦")
        }

    // Priority 4: Textures and Surface Maps
    } else if magic_bytes == magic::TEX_3D {
        (AssetKind::Texture, "TRETexture (3D/2D)", "🎨")
    } else if magic_bytes == magic::TEX_CUBEMAP {
        (AssetKind::Texture, "TRECubeMap", "🌐")
    } else if magic_bytes == magic::TEX_INTERFACE {
        (AssetKind::Texture, "TREInterfaceImage (TGA)", "🖼️")
    // Priority 5: Audio Containers
    } else if magic_bytes == magic::AUDIO_WAV {
        (AssetKind::Audio, "Sound / Voice (WAV)", "🎵")
    // Priority 6: 3D Meshes & Geometry
    } else if magic_bytes == magic::MESH {
        (AssetKind::Mesh, "TREMeshResource", "🗿")
    } else if magic_bytes == b"\x41\x00\x41\x00" {
        (AssetKind::Mesh, "TREMeshGeometry", "📐")
    // Priority 7: Skeletal Animation Clips
    } else if magic_bytes == magic::ANIM_CLIP {
        (AssetKind::Animation, "Skeletal Animation Clip", "🎬")
    // Priority 8: FaceFX Facial Animation Actors
    } else if magic_bytes == b"FACE"
        || (data.len() >= 4 && data[0] == 0x00 && data[1] == 0xBA && data[2] == 0x46)
        || data[..data.len().min(1024)]
            .windows(4)
            .any(|w| w == b"FACE")
    {
        (AssetKind::FaceFx, "FaceFX Facial Animation (.fxe)", "🗣️")
    // Priority 9: Terrain Biome Palettes
    } else if magic_bytes == b"\x7E\x00\x00\x04" {
        (AssetKind::TerrainPalette, "Terrain Texture Palette", "🗺️")
    // Priority 10: Particle Systems & VFX
    } else if data.len() >= 4 && data[2] == 0x73 && data[3] == 0x00 {
        (AssetKind::Vfx, "TREParticleSystem / VFX", "🔥")
    // Priority 11: Fonts and UI Layouts
    } else if magic_bytes == b"\x72\x00\x41\x00" {
        (AssetKind::Font, "TREFont / Sprite Sheet", "🔤")
    } else if magic_bytes == b"\x76\x00\x41\x00" {
        (AssetKind::UI, "UI Sprite Slice", "🖼️")
    } else if magic_bytes == b"\x77\x00\x41\x00" {
        (AssetKind::UI, "UI Control State", "🔘")
    } else if magic_bytes == b"\x71\x00\x41\x00" {
        (AssetKind::UI, "UI / Menu Layout", "🖥️")
    // Priority 12: Animation Timeline Events
    } else if magic_bytes == magic::EVENT || magic_bytes == b"\x0B\x01\x41\x00" {
        (AssetKind::Event, "Animation Event Marker", "👣")
    // Priority 13: Lua Bytecode
    } else if magic_bytes == magic::LUA
        || data[..data.len().min(512)]
            .windows(4)
            .any(|w| w == magic::LUA)
    {
        if magic_bytes == magic::LUA {
            (AssetKind::Lua, "Lua 5.0.2 Bytecode", "📜")
        } else {
            (AssetKind::Lua, "Scripted Logic Node", "📜")
        }
    // Priority 14: Raw Binary Formats
    } else if magic_bytes == b"RIFF" {
        (AssetKind::Audio, "Raw WAV Audio", "🎵")
    } else if magic_bytes == b"DDS " {
        (AssetKind::Texture, "Raw DDS Texture", "🎨")
    // Priority 15: Shader Materials (Strict Overlord 1 & Overlord 2 Material Type IDs)
    } else if (data.len() >= 4 && data[2] == 0x41 && data[1] == 0x06)
        || (data.len() >= 4
            && data[2] == 0x46
            && (data[1] == 0x00
                || data[1] == 0x46
                || (data[1] == 0x06 && (data[0] == 0x24 || data[0] == 0x32))))
        || (data.len() >= 4 && data[2] == 0x40 && (data[1] == 0x00 || data[1] == 0x59))
    {
        (AssetKind::Material, "Shader Material (TREMaterial)", "🛠️")
    // Priority 16: Miscellaneous Engine Structures
    } else if magic_bytes == b"\x4E\x00\x41\x00" {
        (AssetKind::Generic, "Scene Node / Transform", "📍")
    } else if data[..data.len().min(1024)]
        .windows(5)
        .any(|w| w == b"<?xml")
    {
        (AssetKind::Xml, "XML Document", "📋")
    } else if data[..data.len().min(1024)]
        .windows(4)
        .any(|w| w == b".clb")
    {
        (AssetKind::Parameter, "Collision Bounds (.clb)", "🧱")
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

/// Fast header metadata scanner that inspects the first 4 KB to prevent CPU stalls on large assets.
fn extract_internal_strings(data: &[u8]) -> Option<String> {
    let scan_limit = data.len().min(4096);
    let header_slice = &data[..scan_limit];

    let mut best_candidate = None;
    let mut i = 0;

    while i + 8 <= header_slice.len() {
        let len =
            u32::from_le_bytes(header_slice[i..i + 4].try_into().unwrap_or_default()) as usize;
        if (3..=120).contains(&len) && i + 4 + len <= header_slice.len() {
            let slice = &header_slice[i + 4..i + 4 + len];
            if slice.iter().all(|&b| (0x20..=0x7E).contains(&b)) {
                let clean = std::str::from_utf8(slice).unwrap_or_default().trim();
                if clean.ends_with(".dds")
                    || clean.ends_with(".wav")
                    || clean.ends_with(".tga")
                    || clean.ends_with(".xml")
                    || clean.ends_with(".clb")
                    || clean.ends_with(".fxe")
                    || clean.ends_with(".fxa")
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
        i += 1;
    }

    best_candidate
}
