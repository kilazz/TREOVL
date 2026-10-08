use super::parse_typed_container;
use crate::engine::common::{magic, read_length_prefixed_string};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    Collision = 10,
    Environment = 11,
    M8ldMap = 12,
    UiSprite = 13,
    Dta = 14,
    VoicePackage = 15,
    Attachment = 16,
    FaceFx = 17,
    Character = 18,
    Event = 19,
    Xml = 20,
    Parameter = 21,
    Vfx = 22,
    Font = 23,
    Behavior = 24,
    Projectile = 25,
}

impl From<AssetKind> for i32 {
    fn from(kind: AssetKind) -> Self {
        match kind {
            AssetKind::Texture => 0,
            AssetKind::Audio => 1,
            AssetKind::Material => 2,
            AssetKind::Mesh => 3,
            AssetKind::Lua => 4,
            AssetKind::Generic => 5,
            AssetKind::UI => 6,
            AssetKind::Object => 7,
            AssetKind::Animation => 8,
            AssetKind::TerrainPalette => 9,
            AssetKind::Collision => 10,
            AssetKind::Environment => 11,
            AssetKind::M8ldMap => 12,
            AssetKind::UiSprite => 13,
            AssetKind::Dta => 14,
            AssetKind::VoicePackage => 15,
            AssetKind::Attachment => 16,
            AssetKind::FaceFx => 17,
            AssetKind::Character => 18,
            AssetKind::Event => 10,
            AssetKind::Xml => 11,
            AssetKind::Parameter => 12,
            AssetKind::Vfx => 13,
            AssetKind::Font => 14,
            AssetKind::Behavior => 15,
            AssetKind::Projectile => 25,
        }
    }
}

impl TryFrom<i32> for AssetKind {
    type Error = ();

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Texture),
            1 => Ok(Self::Audio),
            2 => Ok(Self::Material),
            3 => Ok(Self::Mesh),
            4 => Ok(Self::Lua),
            5 => Ok(Self::Generic),
            6 => Ok(Self::UI),
            7 => Ok(Self::Object),
            8 => Ok(Self::Animation),
            9 => Ok(Self::TerrainPalette),
            10 => Ok(Self::Collision),
            11 => Ok(Self::Environment),
            12 => Ok(Self::M8ldMap),
            13 => Ok(Self::UiSprite),
            14 => Ok(Self::Dta),
            15 => Ok(Self::VoicePackage),
            16 => Ok(Self::Attachment),
            17 => Ok(Self::FaceFx),
            18 => Ok(Self::Character),
            25 => Ok(Self::Projectile),
            _ => Err(()),
        }
    }
}

impl AssetKind {
    #[inline]
    pub fn to_ui_kind_id(&self) -> i32 {
        i32::from(*self)
    }
}

pub struct SniffedAsset {
    pub kind: AssetKind,
    pub kind_name: &'static str,
    pub icon: &'static str,
    pub display_name: String,
}

fn is_string_list_data(data: &[u8]) -> bool {
    if data.len() < 8 {
        return false;
    }
    let count = u32::from_le_bytes(data[0..4].try_into().unwrap_or_default()) as usize;
    if count == 0 || count > 100 {
        return false;
    }
    let mut pos = 4;
    for _ in 0..count {
        if pos + 4 > data.len() {
            return false;
        }
        let len = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap_or_default()) as usize;
        pos += 4;
        if len == 0 || pos + len > data.len() {
            return false;
        }
        let slice = &data[pos..pos + len];
        if !slice.iter().all(|&b| (0x20..=0x7E).contains(&b)) {
            return false;
        }
        pos += len;
    }
    pos == data.len()
}

fn is_audio_container(data: &[u8]) -> bool {
    if data.starts_with(magic::AUDIO_WAV) || data.starts_with(b"RIFF") {
        return true;
    }
    if let Ok((_, elements)) = parse_typed_container(data) {
        for (id, d) in elements {
            if (id == 22 || id == 30) && (d.starts_with(magic::AUDIO_WAV) || d.starts_with(b"RIFF"))
            {
                return true;
            }
        }
    }
    false
}

pub fn sniff_asset(data: &[u8], filename_hint: &str) -> SniffedAsset {
    if data.len() < 4 {
        return create_sniffed(
            AssetKind::Parameter,
            "Scalar Parameter",
            "⚙️",
            data,
            filename_hint,
        );
    }

    let magic_bytes = &data[0..4];

    // Priority 0: Exact Header & Container Matches
    if is_audio_container(data) {
        let label = if data.starts_with(b"RIFF") {
            "Raw WAV Audio"
        } else {
            "Sound / Voice (WAV)"
        };
        return create_sniffed(AssetKind::Audio, label, "🎵", data, filename_hint);
    } else if magic_bytes == magic::TEX_3D {
        return create_sniffed(
            AssetKind::Texture,
            "Texture (3D/2D)",
            "🎨",
            data,
            filename_hint,
        );
    } else if magic_bytes == magic::TEX_CUBEMAP {
        return create_sniffed(
            AssetKind::Texture,
            "CubeMap Texture",
            "🌐",
            data,
            filename_hint,
        );
    } else if magic_bytes == magic::TEX_INTERFACE {
        return create_sniffed(
            AssetKind::Texture,
            "Interface Image (TGA)",
            "🖼️",
            data,
            filename_hint,
        );
    } else if magic_bytes == magic::MESH {
        return create_sniffed(AssetKind::Mesh, "Mesh Resource", "🗿", data, filename_hint);
    } else if magic_bytes == b"\x41\x00\x41\x00" {
        return create_sniffed(AssetKind::Mesh, "Mesh Geometry", "📐", data, filename_hint);
    } else if magic_bytes == magic::ANIM_CLIP {
        return create_sniffed(
            AssetKind::Animation,
            "Skeletal Animation Clip",
            "🎬",
            data,
            filename_hint,
        );
    } else if magic_bytes == b"\x7E\x00\x00\x04" {
        return create_sniffed(
            AssetKind::TerrainPalette,
            "Terrain Texture Palette",
            "🗺️",
            data,
            filename_hint,
        );
    } else if magic_bytes == b"\x72\x00\x41\x00" {
        return create_sniffed(
            AssetKind::Font,
            "Font / Sprite Sheet",
            "🔤",
            data,
            filename_hint,
        );
    } else if magic_bytes == b"\x76\x00\x41\x00" {
        return create_sniffed(AssetKind::UI, "UI Sprite Slice", "🖼️", data, filename_hint);
    } else if magic_bytes == b"\x77\x00\x41\x00" {
        return create_sniffed(AssetKind::UI, "UI Control State", "🔘", data, filename_hint);
    } else if magic_bytes == b"\x71\x00\x41\x00" {
        return create_sniffed(AssetKind::UI, "UI / Menu Layout", "🖥️", data, filename_hint);
    } else if magic_bytes == magic::EVENT || magic_bytes == b"\x0B\x01\x41\x00" {
        return create_sniffed(
            AssetKind::Event,
            "Animation Event Marker",
            "👣",
            data,
            filename_hint,
        );
    } else if magic_bytes == b"FACE" {
        return create_sniffed(
            AssetKind::FaceFx,
            "FaceFX Facial Animation (.fxe)",
            "🗣️",
            data,
            filename_hint,
        );
    } else if magic_bytes == b"M8LD" {
        return create_sniffed(
            AssetKind::M8ldMap,
            "Level Map Logic Layer (.8ld)",
            "🗺️",
            data,
            filename_hint,
        );
    } else if magic_bytes == b"CPTX" || magic_bytes == b"CRL\0" {
        let label = if magic_bytes == b"CPTX" {
            "UI Texture Atlas Map (CPTX)"
        } else {
            "UI Sprite Collection (CRL)"
        };
        return create_sniffed(AssetKind::UiSprite, label, "🖼️", data, filename_hint);
    } else if magic_bytes == b"DDS " {
        return create_sniffed(
            AssetKind::Texture,
            "Raw DDS Texture",
            "🎨",
            data,
            filename_hint,
        );
    } else if magic_bytes == b"\x83\x00\x00\x04" {
        return create_sniffed(
            AssetKind::Environment,
            "Environment / Light Profile",
            "🌌",
            data,
            filename_hint,
        );
    }

    // Priority 1: Scene Graph Objects, Lights, Mechanisms & Placement Objects
    let is_object_family = if data.len() >= 4 {
        let t = u32::from_le_bytes(data[0..4].try_into().unwrap_or_default());
        let hi = t >> 8;
        hi == 0x004646 || hi == 0x004621 || hi == 0x004650 || t == 0x0041004B
    } else {
        false
    };

    let is_object_direct = magic_bytes == magic::OBJECT
        || magic_bytes == b"\x21\x46\x46\x00"
        || magic_bytes == b"\x67\x00\x41\x00"
        || magic_bytes == b"\x69\x46\x46\x00"
        || magic_bytes == b"\x81\x46\x46\x00"
        || magic_bytes == b"\x03\x21\x46\x00"
        || is_object_family;

    let is_object_wrapped = data[..data.len().min(512)].windows(4).any(|w| {
        w == b"\x4B\x00\x41\x00"
            || w == b"\x21\x46\x46\x00"
            || w == b"\x61\x46\x46\x00"
            || w == b"\x69\x46\x46\x00"
            || w == b"\x81\x46\x46\x00"
            || w == b"\x03\x21\x46\x00"
            || (w[3] == 0x00 && w[2] == 0x46 && (w[1] == 0x50 || w[1] == 0x46 || w[1] == 0x21))
    });

    let is_character_container = if data.len() >= 4 {
        let t = u32::from_le_bytes(data[0..4].try_into().unwrap_or_default());
        ((t >> 8) == 0x004640) || t == 0x00463018 || t == 0x0046305B || t == 0x0046300D
    } else {
        false
    };

    let (kind, kind_name, icon) = if is_object_direct || is_object_wrapped {
        let type_id = if is_object_direct {
            u32::from_le_bytes(magic_bytes.try_into().unwrap_or_default())
        } else {
            let pos = data[..data.len().min(512)]
                .windows(4)
                .position(|w| {
                    w == b"\x4B\x00\x41\x00"
                        || w == b"\x21\x46\x46\x00"
                        || w == b"\x61\x46\x46\x00"
                        || w == b"\x69\x46\x46\x00"
                        || w == b"\x81\x46\x46\x00"
                        || w == b"\x03\x21\x46\x00"
                        || (w[3] == 0x00
                            && w[2] == 0x46
                            && (w[1] == 0x50 || w[1] == 0x46 || w[1] == 0x21))
                })
                .unwrap_or(0);
            u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap_or_default())
        };

        let label = match type_id {
            0x00462107 => "Point Light Object (TREPointLight)",
            0x00462103 => "Logic/Environment Marker (TRELogicMarker)",
            0x00464661 => "Sound Marker Object (TRESoundMarker)",
            0x00464665 | 0x00464669 => "Pushable Wheel Mechanism (TREPushableWheel)",
            0x00464181 => "Minion Spawner Gate (TREMinionGate)",
            0x00464681 => "Upgrade Portal (TREUpgradePortal)",
            0x00464621 => "Placement Object (TREPlacementObject)",
            _ if (type_id >> 8) == 0x004650 => "Interactive Mechanism / Door (TREDoorController)",
            _ if (type_id >> 8) == 0x004621 => "World Marker / Light (TRELightMarker)",
            _ if (type_id >> 8) == 0x004646 => "Scene Object / Mechanism (TREMechanism)",
            _ => "Model Resource (Entity/Prop)",
        };
        let icon_str = match type_id {
            0x00462107 => "💡",
            0x00464665 | 0x00464669 => "⚙️",
            _ if (type_id >> 8) == 0x004650 => "🚪",
            _ => "🧊",
        };
        (AssetKind::Object, label, icon_str)

    // Priority 2: Character / Actor / Player / Breakable / Critter Controllers
    } else if is_character_container {
        let mut is_valid = false;
        if let Ok((_, elements)) = parse_typed_container(data) {
            is_valid = elements.iter().any(|(id, _)| {
                *id == 115
                    || *id == 112
                    || *id == 50
                    || *id == 37
                    || *id == 31
                    || *id == 33
                    || *id == 29
                    || *id == 86
                    || *id == 21
                    || *id == 25
                    || *id == 63
                    || *id == 67
                    || *id == 70
            });
        }
        if is_valid {
            let t_id = u32::from_le_bytes(data[0..4].try_into().unwrap_or_default());
            let (label, icon) = match t_id {
                0x00463018 => ("Breakable Prop / Destructible Object", "💥"),
                0x0046305B => ("Interactive Prop / Tower Captive", "👸"),
                0x0046300D => ("Ambient Creature / Critter (TRECritter)", "🦇"),
                0x0046401C => ("Player Controller (TREPlayerActor)", "👑"),
                _ => ("Character / NPC Controller (TREActor)", "🧙‍♂️"),
            };
            (AssetKind::Character, label, icon)
        } else {
            (AssetKind::Generic, "Triumph Binary Container", "📦")
        }

    // Priority 3: All Weapons, Items, Attachments, Armor & Pickups
    } else if (data.len() >= 4 && data[2] == 0x46 && data[1] == 0x20)
        || data[..data.len().min(512)].windows(4).any(|w| {
            w[3] == 0x00
                && w[2] == 0x46
                && w[1] == 0x20
                && matches!(w[0], 0x0B | 0x0D | 0x11 | 0x15 | 0x17 | 0x1B | 0x21 | 0x3F)
        })
    {
        let t_id = if data.len() >= 4 && data[2] == 0x46 && data[1] == 0x20 {
            data[0]
        } else if let Some(pos) = data[..data.len().min(512)].windows(4).position(|w| {
            w[3] == 0x00
                && w[2] == 0x46
                && w[1] == 0x20
                && matches!(w[0], 0x0B | 0x0D | 0x11 | 0x15 | 0x17 | 0x1B | 0x21 | 0x3F)
        }) {
            data[pos]
        } else {
            0x0D
        };

        let (label, icon) = match t_id {
            0x11 | 0x15 | 0x17 | 0x21 => ("Weapon Resource (TREWeaponResource)", "🗡️"),
            0x0B => ("Armor / Equipment (TREEquipmentResource)", "🛡️"),
            0x1B => ("Consumable / Held Prop (TREConsumableResource)", "🍺"),
            0x3F => ("Breakable Pickup / Carryable Item (TREBreakableItem)", "🥚"),
            _ => ("Attached Item / Prop (TREItemResource)", "🍽️"),
        };
        (AssetKind::Attachment, label, icon)

    // Priority 4: Combat Projectiles, Spells & Hazards
    } else if data.len() >= 4 && {
        let type_id = u32::from_le_bytes(data[0..4].try_into().unwrap_or_default());
        matches!(type_id, 0x00463006 | 0x00463063 | 0x00463065 | 0x00463028)
            || ((type_id >> 8) == 0x004630
                && type_id != 0x00463018
                && type_id != 0x0046305B
                && type_id != 0x0046300D)
    } {
        let type_id = u32::from_le_bytes(data[0..4].try_into().unwrap_or_default());
        let (label, icon) = match type_id {
            0x00463063 => ("Spell Effect / Lure Trap (TRELure)", "✨"),
            0x00463065 => ("Area Buff / Corruption Spell (TRESpell)", "🔮"),
            0x00463028 => ("Hazard Zone / Damage Dealer (TREDamageDealer)", "🔥"),
            _ => ("Combat Projectile / Spell (TREProjectile)", "🏹"),
        };
        (AssetKind::Projectile, label, icon)

    // FaceFX Facial Animations
    } else if data[..data.len().min(1024)]
        .windows(4)
        .any(|w| w == b"FACE")
        || (data.len() >= 4 && data[0] == 0x00 && data[1] == 0xBA && data[2] == 0x46)
    {
        (AssetKind::FaceFx, "FaceFX Facial Animation (.fxe)", "🗣️")

    // Particle FX
    } else if data.len() >= 4 && data[2] == 0x73 && data[3] == 0x00 {
        (AssetKind::Vfx, "Particle System / VFX", "🔥")

    // Lua Scripts
    } else if data[..data.len().min(512)]
        .windows(4)
        .any(|w| w == magic::LUA)
    {
        (AssetKind::Lua, "Scripted Logic Node", "📜")

    // Shader Materials
    } else if (data.len() >= 4 && data[2] == 0x41 && data[1] == 0x06)
        || (data.len() >= 4
            && data[2] == 0x46
            && (data[1] == 0x00
                || (data[1] == 0x46 && matches!(data[0], 0x08 | 0x14 | 0x20))
                || (data[1] == 0x06 && (data[0] == 0x24 || data[0] == 0x32))))
        || (data.len() >= 4 && data[2] == 0x40 && (data[1] == 0x00 || data[1] == 0x59))
    {
        (AssetKind::Material, "Shader Material", "🛠️")

    // Collision Boundaries
    } else if filename_hint.to_lowercase().ends_with(".clb")
        && !data.starts_with(b"CRL\0")
        && !data.windows(4).any(|w| w == b"\x60\x00\x41\x00")
    {
        (AssetKind::Collision, "Collision Boundary (.clb)", "🧱")

    // Environment & Sky Profiles
    } else if filename_hint.to_lowercase().ends_with(".env")
        || (data.len() > 13 && data[0] == 0x80 && &data[5..9] == b"\xCC\x0B\x00\x00")
        || data[..data.len().min(1024)]
            .windows(4)
            .any(|w| w == b".env")
    {
        (
            AssetKind::Environment,
            "Environment / Sky Profile (.env)",
            "🌌",
        )

    // Lighting Sets
    } else if filename_hint.to_lowercase().ends_with(".dta")
        || (data.len() > 64
            && crate::engine::container::footer::check_footer(data).is_some()
            && data.len() < 150_000
            && !data.starts_with(b"PRP")
            && !data.starts_with(b"OMP"))
    {
        (AssetKind::Dta, "Lighting Set / Binary Data (.dta)", "💡")

    // Voice Packages
    } else if filename_hint.to_lowercase().ends_with(".debug-vpk")
        || filename_hint.to_lowercase().ends_with(".vpk")
        || (data.starts_with(b"RPK\0")
            && data.len() < 50_000
            && data.windows(12).any(|w| w == b"Voice" || w == b"voice"))
    {
        (
            AssetKind::VoicePackage,
            "Voice Package Descriptor (.debug-vpk)",
            "🗣️",
        )

    // Level Map Logic
    } else if filename_hint.to_lowercase().ends_with(".8ld")
        || (!data.is_empty()
            && (data[..data.len().min(256)]
                .windows(4)
                .any(|w| w == b"\xA7\xA8\x47\x18")
                || data[..data.len().min(256)]
                    .windows(4)
                    .any(|w| w == b"\x72\xCD\xDD\xC4")))
    {
        (AssetKind::M8ldMap, "Level Map Logic Layer (.8ld)", "🗺️")

    // UI Atlas Collections
    } else if data
        .windows(4)
        .any(|w| w == b"\x60\x00\x41\x00" || w == b"\x78\x00\x41\x00")
    {
        (AssetKind::UiSprite, "UI Sprite Collection (CRL)", "🖼️")

    // Scene Nodes
    } else if magic_bytes == b"\x4E\x00\x41\x00" {
        (AssetKind::Generic, "Scene Node / Transform", "📍")

    // XML Documents
    } else if data[..data.len().min(1024)]
        .windows(5)
        .any(|w| w == b"<?xml")
    {
        (AssetKind::Xml, "XML Document", "📋")

    // Generic Parameters
    } else if is_string_list_data(data) || data.len() <= 128 {
        (AssetKind::Parameter, "Engine Parameter", "⚙️")
    } else {
        (AssetKind::Generic, "Triumph Binary Container", "📦")
    };

    create_sniffed(kind, kind_name, icon, data, filename_hint)
}

fn create_sniffed(
    kind: AssetKind,
    kind_name: &'static str,
    icon: &'static str,
    data: &[u8],
    filename_hint: &str,
) -> SniffedAsset {
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
    if let Ok((_, elements)) = parse_typed_container(data) {
        if let Some((_, chunk25)) = elements.iter().find(|(id, _)| *id == 25)
            && let Some(s) = read_length_prefixed_string(chunk25)
            && !s.is_empty()
            && !s.starts_with('[')
        {
            return Some(s);
        }
        if let Some((_, chunk21)) = elements.iter().find(|(id, _)| *id == 21)
            && let Some(s) = read_length_prefixed_string(chunk21)
            && !s.is_empty()
            && !s.starts_with('[')
            && s != "noname"
            && s != "Item"
        {
            return Some(s);
        }
    }

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
                    || clean.ends_with(".env")
                    || clean.ends_with(".dta")
                    || clean.ends_with(".debug-vpk")
                    || clean.ends_with(".vpk")
                    || clean.ends_with(".8ld")
                    || clean.ends_with(".fxe")
                    || clean.ends_with(".fxa")
                    || clean.ends_with(".map")
                {
                    return Some(clean.to_string());
                }
                if (clean.starts_with('[') && clean.contains(']'))
                    || (best_candidate.is_none() && clean.len() >= 3 && clean != "17040")
                {
                    best_candidate = Some(clean.to_string());
                }
            }
        }
        i += 1;
    }

    best_candidate
}
