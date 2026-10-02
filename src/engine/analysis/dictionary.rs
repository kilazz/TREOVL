#![allow(dead_code)]

use crate::engine::common::{calculate_crc32, calculate_triumph_crc32};
use std::collections::HashMap;
use std::sync::LazyLock;

const KNOWN_KEYWORDS: &[&str] = &[
    "Overlord",
    "Minion_Brown",
    "Minion_Red",
    "Minion_Green",
    "Minion_Blue",
    "Player_Spawn",
    "Archie",
    "Gate_Wood",
    "Castle_Wall",
    "Spawning_Pool",
    "Head",
    "Spine",
    "Pelvis",
    "L_Hand",
    "R_Hand",
    "L_Foot",
    "R_Foot",
    "Bip01",
    "Diffuse_Texture",
    "Normal_Map",
    "Specular_Color",
    "Reflection_Map",
    "Flow_Speed",
    "Wave_Amplitude",
    "UV_Tiling_X",
    "UV_Tiling_Y",
    "Roughness",
    "Metalness",
    "Alpha_Threshold",
    "Collision_Box",
    "Trigger_Zone",
    "Waypoint",
    "Footstep_Left",
    "Footstep_Right",
    "Attack_Light",
    "Attack_Heavy",
    "Death",
];

pub struct HashDictionary {
    triumph_crc_map: HashMap<u32, &'static str>,
    standard_crc_map: HashMap<u32, &'static str>,
}

impl HashDictionary {
    pub fn new() -> Self {
        let mut triumph_crc_map = HashMap::new();
        let mut standard_crc_map = HashMap::new();

        for &word in KNOWN_KEYWORDS {
            let bytes = word.as_bytes();
            let crc = calculate_crc32(bytes);
            let triumph_crc = calculate_triumph_crc32(bytes);

            standard_crc_map.insert(crc, word);
            triumph_crc_map.insert(triumph_crc, word);
        }

        Self {
            triumph_crc_map,
            standard_crc_map,
        }
    }

    pub fn lookup(&self, hash: u32) -> Option<&'static str> {
        self.triumph_crc_map
            .get(&hash)
            .copied()
            .or_else(|| self.standard_crc_map.get(&hash).copied())
    }
}

impl Default for HashDictionary {
    fn default() -> Self {
        Self::new()
    }
}

pub static DICTIONARY: LazyLock<HashDictionary> = LazyLock::new(HashDictionary::new);
