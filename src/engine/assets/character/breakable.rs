use serde::{Deserialize, Serialize};

use crate::engine::common::Endian;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BreakablePropsConfigJson {
    pub base_health: f32,
    pub collapse_target_model: String,
    pub physics_material_id: u32,
    pub debris_pieces_count: usize,
    pub sound_cue_id: u32,
    pub trigger_collapse_on_hit: bool,
    pub spawn_debris_particles: bool,
    pub can_be_carried: bool,
    pub center_offset: [f32; 3],
}

pub fn extract_collapse_target_from_script(script: &str) -> Option<String> {
    for line in script.lines() {
        if line.contains("Collapse(")
            && let Some(first_quote) = line.find('"')
        {
            let tail = &line[first_quote + 1..];
            if let Some(second_quote) = tail.find('"') {
                let target = &tail[..second_quote];
                return Some(target.replace("\\\\", "\\"));
            }
        }
    }
    None
}

pub fn build_breakable_blocks(
    brk: Option<&BreakablePropsConfigJson>,
    base_health: Option<f32>,
    updated_lua: Option<&str>,
    endian: Endian,
) -> Vec<(u32, Vec<u8>)> {
    let mut elements = Vec::new();
    let mat_id = brk.map(|b| b.physics_material_id).unwrap_or(2);
    let health = brk.map(|b| b.base_health).or(base_health).unwrap_or(5.0);
    let debris_count = brk.map(|b| b.debris_pieces_count).unwrap_or(8);
    let cue_id = brk.map(|b| b.sound_cue_id).unwrap_or(112);
    let trigger = brk.is_none_or(|b| b.trigger_collapse_on_hit);
    let particles = brk.is_none_or(|b| b.spawn_debris_particles);
    let carry = brk.is_none_or(|b| b.can_be_carried);
    let center = brk.map(|b| b.center_offset).unwrap_or([0.0, 0.5, 0.0]);

    if carry {
        elements.push((29, vec![1, 40, 0, 2, 40, 0, 43, 4, 1, 1, 0, 0, 0]));
    } else {
        elements.push((29, vec![0u8]));
    }

    elements.push((41, vec![1, 1, 0, 0]));
    elements.push((42, vec![2, 0x1E, 0, 0x23, 1, 0, 0, 0, 0, 0]));
    elements.push((45, vec![1, 1, 0, 0]));
    elements.push((46, endian.u32_to_bytes(mat_id).to_vec()));

    let mut blk70 = Vec::with_capacity(1 + debris_count * 2 + debris_count * 28);
    blk70.push(debris_count as u8);
    for i in 0..debris_count {
        blk70.push((40 + i) as u8);
        blk70.push((i * 28) as u8);
    }
    for _ in 0..debris_count {
        blk70.extend_from_slice(&[
            6, 0x28, 0, 0x29, 1, 0x2A, 2, 0x2B, 3, 0x2C, 7, 0x2D, 11, 0, 0, 0, 1, 0x14, 0, 0, 1, 1,
            0, 0, 1, 1, 0, 0,
        ]);
    }
    elements.push((70, blk70));

    elements.push((
        71,
        vec![
            13, 0x29, 0, 0x2A, 7, 0x2B, 8, 0x2C, 9, 0x2D, 10, 0x64, 11, 0x65, 24, 0x66, 37, 0x67,
            50, 0x68, 63, 0x69, 76, 0x6A, 89, 0x6B, 102, 1, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0x29,
            0, 0x2A, 1, 0x2C, 2, 0x30, 3, 0, 0, 0, 0, 4, 0x29, 0, 0x2A, 1, 0x2C, 2, 0x30, 3, 0, 0,
            0, 0, 4, 0x29, 0, 0x2A, 1, 0x2C, 2, 0x30, 3, 0, 0, 0, 0, 4, 0x29, 0, 0x2A, 1, 0x2C, 2,
            0x30, 3, 0, 0, 0, 0, 4, 0x29, 0, 0x2A, 1, 0x2C, 2, 0x30, 3, 0, 0, 0, 0, 4, 0x29, 0,
            0x2A, 1, 0x2C, 2, 0x30, 3, 0, 0, 0, 0, 4, 0x29, 0, 0x2A, 1, 0x2C, 2, 0x30, 3, 0, 0, 0,
            0, 4, 0x29, 0, 0x2A, 1, 0x2C, 2, 0x30, 3, 0, 0, 0, 0,
        ],
    ));

    let lua_text = updated_lua.map(|s| s.to_string()).unwrap_or_else(|| {
        format!(
            "local AliasName = GetAlias()\nCollapse(AliasName, \"{}\")",
            brk.map(|b| b.collapse_target_model.as_str()).unwrap_or("")
        )
    });

    let mut script_sub = Vec::new();
    for line in lua_text.lines() {
        let b = line.as_bytes();
        script_sub.extend_from_slice(&endian.u32_to_bytes(b.len() as u32));
        script_sub.extend_from_slice(b);
    }

    let mut script_container = Vec::new();
    script_container.extend_from_slice(&[3, 0x15, 0, 0x16, 0x54, 0x17, 0x58, 2, 0, 0, 0]);
    script_container.extend_from_slice(&script_sub);
    script_container.extend_from_slice(&[0xBF, 0, 0, 0]);
    script_container.extend_from_slice(b"\x1bLua\x50\x01\x04\x04\x04\x06\x08\x09\x09\x08\xB6\x09\x93\x68\xE7\xF5\x7D\x41\x01\0\0\0\0\0\0\0\0\0\0\0\x04\x07\0\0\0\x01\0\0\0\x01\0\0\0\x02\0\0\0\x02\0\0\0\x02\0\0\0\x02\0\0\0\x02\0\0\0\x01\0\0\0\x0A\0\0\0AliasName\0\x02\0\0\0\x06\0\0\0\0\0\0\0\x03\0\0\0\x04\x09\0\0\0GetAlias\0\x04\x09\0\0\0Collapse\0\x04\x14\0\0\0[Props_Sewers]OBJ\\6\0\0\0\0\0\x07\0\0\0\x05\0\0\0\x99\x80\0\0\x45\0\0\x01\0\0\0\x02\x81\0\0\x03\x59\x80\x01\x01\x1B\x80\0\0\0\0\0\0");

    let mut blk200 = Vec::new();
    blk200.extend_from_slice(&[
        0x83, 4, 0, 0, 0, 10, 0, 12, 4, 13, 5, 14, 0, 0, 0, 0x21, 1, 0, 0, 19, 0, 0, 0, 0x22, 1, 0,
        0, 22, 0, 0, 0, 0x23, 1, 0, 0, 23, 0, 0, 0, 0x24, 1, 0, 0,
    ]);
    blk200.extend_from_slice(&endian.f32_to_bytes(health));
    blk200.push(0);
    blk200.extend_from_slice(&script_container);
    elements.push((200, blk200));

    elements.push((201, vec![1, 0x16, 0, 1, 0x14, 0, cue_id as u8, 0, 0, 0]));
    elements.push((202, vec![if trigger { 1 } else { 0 }, 1, 0, 0]));
    elements.push((203, vec![if particles { 1 } else { 0 }, 1, 0, 0]));

    let mut blk300 = vec![1u8, 20, 0];
    let _ = endian.write_f32(&mut blk300, center[0]);
    let _ = endian.write_f32(&mut blk300, center[1]);
    let _ = endian.write_f32(&mut blk300, center[2]);
    elements.push((300, blk300));
    elements.push((301, vec![0u8]));

    elements
}
