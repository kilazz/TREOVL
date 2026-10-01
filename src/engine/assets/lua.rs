use super::{build_chunk_from_elements, parse_chunk_elements};
use anyhow::{Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Cursor;

pub const LUA_MAGIC: &[u8; 4] = b"\x1bLua";
pub const LUA_VERSION_50: u8 = 0x50;

#[derive(Debug, Serialize, Deserialize)]
pub struct LuaInfo {
    pub is_valid_50: bool,
    pub script_name: Option<String>,
    pub bytecode_len: usize,
    pub string_constants: Vec<String>,
}

pub const OP_NAMES: [&str; 35] = [
    "MOVE",
    "LOADK",
    "LOADBOOL",
    "LOADNIL",
    "GETUPVAL",
    "GETGLOBAL",
    "GETTABLE",
    "SETGLOBAL",
    "SETUPVAL",
    "SETTABLE",
    "NEWTABLE",
    "SELF",
    "ADD",
    "SUB",
    "MUL",
    "DIV",
    "POW",
    "UNM",
    "NOT",
    "CONCAT",
    "JMP",
    "EQ",
    "LT",
    "LE",
    "TEST",
    "CALL",
    "TAILCALL",
    "RETURN",
    "FORLOOP",
    "TFORLOOP",
    "TFORPREP",
    "SETLIST",
    "SETLISTO",
    "CLOSE",
    "CLOSURE",
];

#[derive(Debug, Clone)]
pub enum LuaConstant {
    Nil,
    Bool(bool),
    Number(f64),
    String(String),
}

pub fn validate_lua_50_header(bytecode: &[u8]) -> Result<()> {
    if bytecode.len() < 12 {
        bail!("Bytecode is too short to be valid Lua.");
    }
    if &bytecode[0..4] != LUA_MAGIC {
        bail!("Missing '\\x1bLua' signature. Engine requires compiled bytecode.");
    }
    if bytecode[4] != LUA_VERSION_50 {
        bail!(
            "Incompatible Lua version (0x{:02X}). Overlord requires Lua 5.0.2 (0x50).",
            bytecode[4]
        );
    }
    if bytecode[6] != 1 || bytecode[7] != 4 || bytecode[8] != 4 || bytecode[11] != 8 {
        bail!("Lua VM type sizes mismatch. Must be 32-bit Little-Endian with double Number.");
    }
    Ok(())
}

fn read_lua_string(cur: &mut Cursor<&[u8]>) -> Result<Option<String>> {
    let len = cur.read_u32::<LittleEndian>()? as usize;
    if len == 0 {
        return Ok(None);
    }
    let pos = cur.position() as usize;
    let data = cur.get_ref();
    if pos + len > data.len() {
        bail!("Unexpected EOF while reading string");
    }
    let s = String::from_utf8_lossy(&data[pos..pos + len])
        .trim_matches(char::from(0))
        .to_string();
    cur.set_position((pos + len) as u64);
    Ok(Some(s))
}

// -------------------------------------------------------------
// HIGH-LEVEL PSEUDOCODE DECOMPILER
// -------------------------------------------------------------

pub fn decompile_lua_bytecode(bytecode: &[u8]) -> Result<String> {
    validate_lua_50_header(bytecode)?;

    let mut cur = Cursor::new(&bytecode[12..]);
    let mut out = String::new();

    out.push_str("-- ========================================================\n");
    out.push_str("-- Decompiled Lua 5.0.2 High-Level Pseudocode\n");
    out.push_str("-- ========================================================\n\n");

    decompile_scope(&mut cur, &mut out, 0)?;
    Ok(out)
}

fn decompile_scope(cur: &mut Cursor<&[u8]>, out: &mut String, level: usize) -> Result<()> {
    let indent = "    ".repeat(level);

    let source = read_lua_string(cur)?.unwrap_or_else(|| "anonymous".into());
    let _line = cur.read_u32::<LittleEndian>()?;
    let num_params = cur.read_u8()?;
    let _is_vararg = cur.read_u8()?;
    let _max_stack = cur.read_u8()?;

    let params: Vec<String> = (0..num_params).map(|i| format!("arg_{}", i)).collect();
    out.push_str(&format!(
        "{indent}function {} ({})\n",
        source,
        params.join(", ")
    ));

    let num_lines = cur.read_u32::<LittleEndian>()? as usize;
    for _ in 0..num_lines {
        let _ = cur.read_u32::<LittleEndian>()?;
    }

    let num_constants = cur.read_u32::<LittleEndian>()? as usize;
    let mut constants = Vec::with_capacity(num_constants);
    for _ in 0..num_constants {
        let k_type = cur.read_u8()?;
        let c = match k_type {
            0 => LuaConstant::Nil,
            1 => LuaConstant::Bool(cur.read_u8()? != 0),
            3 => LuaConstant::Number(cur.read_f64::<LittleEndian>()?),
            4 => LuaConstant::String(read_lua_string(cur)?.unwrap_or_default()),
            _ => bail!("Unknown constant tag: {}", k_type),
        };
        constants.push(c);
    }

    let mut registers: HashMap<usize, String> = HashMap::new();
    for (i, p) in params.iter().enumerate() {
        registers.insert(i, p.clone());
    }

    let get_rk = |val: usize, ksts: &[LuaConstant], regs: &HashMap<usize, String>| -> String {
        if (val & 256) != 0 {
            let k_idx = val & 255;
            match ksts.get(k_idx) {
                Some(LuaConstant::String(s)) => format!("\"{}\"", s),
                Some(LuaConstant::Number(n)) => format!("{}", n),
                Some(LuaConstant::Bool(b)) => format!("{}", b),
                Some(LuaConstant::Nil) => "nil".into(),
                None => format!("K[{}]", k_idx),
            }
        } else {
            regs.get(&val)
                .cloned()
                .unwrap_or_else(|| format!("r{}", val))
        }
    };

    let num_code = cur.read_u32::<LittleEndian>()? as usize;
    for _ in 0..num_code {
        let inst = cur.read_u32::<LittleEndian>()?;
        let opcode = (inst & 0x3F) as usize;
        let a = ((inst >> 6) & 0xFF) as usize;
        let b = ((inst >> 14) & 0x1FF) as usize;
        let c = ((inst >> 23) & 0x1FF) as usize;
        let bx = ((inst >> 14) & 0x3FFFF) as usize;

        let op_name = OP_NAMES.get(opcode).copied().unwrap_or("UNKNOWN");

        match op_name {
            "MOVE" => {
                let val = registers
                    .get(&b)
                    .cloned()
                    .unwrap_or_else(|| format!("r{}", b));
                registers.insert(a, val);
            }
            "LOADK" => {
                let val_str = match constants.get(bx) {
                    Some(LuaConstant::String(s)) => format!("\"{}\"", s),
                    Some(LuaConstant::Number(n)) => format!("{}", n),
                    Some(LuaConstant::Bool(b)) => format!("{}", b),
                    _ => "nil".into(),
                };
                registers.insert(a, val_str);
            }
            "LOADBOOL" => {
                registers.insert(a, (b != 0).to_string());
            }
            "LOADNIL" => {
                for r in a..=b {
                    registers.insert(r, "nil".into());
                }
            }
            "GETGLOBAL" => {
                let gname = match constants.get(bx) {
                    Some(LuaConstant::String(s)) => s.clone(),
                    _ => format!("G_{}", bx),
                };
                registers.insert(a, gname);
            }
            "SETGLOBAL" => {
                let gname = match constants.get(bx) {
                    Some(LuaConstant::String(s)) => s.clone(),
                    _ => format!("G_{}", bx),
                };
                let val = registers.get(&a).cloned().unwrap_or_else(|| "nil".into());
                out.push_str(&format!("{indent}    {} = {}\n", gname, val));
            }
            "GETTABLE" => {
                let table = registers
                    .get(&b)
                    .cloned()
                    .unwrap_or_else(|| format!("r{}", b));
                let key = get_rk(c, &constants, &registers);
                registers.insert(a, format!("{}[{}]", table, key));
            }
            "SETTABLE" => {
                let table = registers
                    .get(&a)
                    .cloned()
                    .unwrap_or_else(|| format!("r{}", a));
                let key = get_rk(b, &constants, &registers);
                let val = get_rk(c, &constants, &registers);
                out.push_str(&format!("{indent}    {}[{}] = {}\n", table, key, val));
            }
            "ADD" => {
                let op1 = get_rk(b, &constants, &registers);
                let op2 = get_rk(c, &constants, &registers);
                registers.insert(a, format!("({} + {})", op1, op2));
            }
            "SUB" => {
                let op1 = get_rk(b, &constants, &registers);
                let op2 = get_rk(c, &constants, &registers);
                registers.insert(a, format!("({} - {})", op1, op2));
            }
            "MUL" => {
                let op1 = get_rk(b, &constants, &registers);
                let op2 = get_rk(c, &constants, &registers);
                registers.insert(a, format!("({} * {})", op1, op2));
            }
            "DIV" => {
                let op1 = get_rk(b, &constants, &registers);
                let op2 = get_rk(c, &constants, &registers);
                registers.insert(a, format!("({} / {})", op1, op2));
            }
            "CONCAT" => {
                let mut parts = Vec::new();
                for r in b..=c {
                    parts.push(
                        registers
                            .get(&r)
                            .cloned()
                            .unwrap_or_else(|| format!("r{}", r)),
                    );
                }
                registers.insert(a, parts.join(" .. "));
            }
            "CALL" => {
                let func = registers
                    .get(&a)
                    .cloned()
                    .unwrap_or_else(|| format!("func_{}", a));
                let num_args = b.saturating_sub(1);
                let mut call_args = Vec::new();
                for arg_i in 1..=num_args {
                    let reg_idx = a + arg_i;
                    let arg_val = registers
                        .get(&reg_idx)
                        .cloned()
                        .unwrap_or_else(|| format!("r{}", reg_idx));
                    call_args.push(arg_val);
                }
                out.push_str(&format!("{indent}    {}({})\n", func, call_args.join(", ")));
            }
            "RETURN" => {
                let count = b.saturating_sub(1);
                if count == 0 {
                    out.push_str(&format!("{indent}    return\n"));
                } else {
                    let ret_val = registers.get(&a).cloned().unwrap_or_else(|| "nil".into());
                    out.push_str(&format!("{indent}    return {}\n", ret_val));
                }
            }
            _ => {}
        }
    }

    out.push_str(&format!("{indent}end\n\n"));

    let num_nested = cur.read_u32::<LittleEndian>()? as usize;
    for _ in 0..num_nested {
        decompile_scope(cur, out, level + 1)?;
    }

    Ok(())
}

// -------------------------------------------------------------
// LOW-LEVEL DISASSEMBLER
// -------------------------------------------------------------

pub fn disassemble_lua_bytecode(bytecode: &[u8]) -> Result<String> {
    validate_lua_50_header(bytecode)?;

    let mut cur = Cursor::new(&bytecode[12..]);
    let mut out = String::new();

    out.push_str("; ========================================================\n");
    out.push_str("; Disassembled Lua 5.0.2 Bytecode (Overlord Engine)\n");

    disassemble_function_prototype(&mut cur, &mut out, 0)?;

    out.push_str("; ========================================================\n");
    Ok(out)
}

fn disassemble_function_prototype(
    cur: &mut Cursor<&[u8]>,
    out: &mut String,
    level: usize,
) -> Result<()> {
    let indent = "  ".repeat(level);

    let source = read_lua_string(cur)?.unwrap_or_else(|| "N/A".into());
    let line_defined = cur.read_u32::<LittleEndian>()?;
    let num_params = cur.read_u8()?;
    let is_vararg = cur.read_u8()?;
    let max_stack = cur.read_u8()?;

    out.push_str(&format!(
        "\n{indent}; Source: {}\n{indent}; Line: {} | Params: {} | Vararg: {} | MaxStack: {}\n",
        source, line_defined, num_params, is_vararg, max_stack
    ));

    let num_lines = cur.read_u32::<LittleEndian>()? as usize;
    for _ in 0..num_lines {
        let _ = cur.read_u32::<LittleEndian>()?;
    }

    let num_constants = cur.read_u32::<LittleEndian>()? as usize;
    let mut constants = Vec::with_capacity(num_constants);

    out.push_str(&format!("{indent}.constants ({})\n", num_constants));
    for i in 0..num_constants {
        let k_type = cur.read_u8()?;
        let c = match k_type {
            0 => LuaConstant::Nil,
            1 => {
                let b = cur.read_u8()?;
                LuaConstant::Bool(b != 0)
            }
            3 => {
                let num = cur.read_f64::<LittleEndian>()?;
                LuaConstant::Number(num)
            }
            4 => {
                let s = read_lua_string(cur)?.unwrap_or_default();
                LuaConstant::String(s)
            }
            _ => bail!("Unknown constant type tag: {}", k_type),
        };

        match &c {
            LuaConstant::Nil => out.push_str(&format!("{indent}  [{}] nil\n", i)),
            LuaConstant::Bool(b) => out.push_str(&format!("{indent}  [{}] {}\n", i, b)),
            LuaConstant::Number(n) => out.push_str(&format!("{indent}  [{}] {}\n", i, n)),
            LuaConstant::String(s) => out.push_str(&format!("{indent}  [{}] \"{}\"\n", i, s)),
        }
        constants.push(c);
    }

    let num_code = cur.read_u32::<LittleEndian>()? as usize;
    out.push_str(&format!("{indent}.code ({} instructions)\n", num_code));

    let fmt_rk = |val: usize, ksts: &[LuaConstant]| -> String {
        if (val & 256) != 0 {
            let k_idx = val & 255;
            if let Some(k) = ksts.get(k_idx) {
                match k {
                    LuaConstant::String(s) => format!("K(\"{}\")", s),
                    LuaConstant::Number(n) => format!("K({})", n),
                    LuaConstant::Bool(b) => format!("K({})", b),
                    LuaConstant::Nil => "K(nil)".into(),
                }
            } else {
                format!("K[{}]", k_idx)
            }
        } else {
            format!("R{}", val)
        }
    };

    for pc in 0..num_code {
        let inst = cur.read_u32::<LittleEndian>()?;

        let opcode = (inst & 0x3F) as usize;
        let a = ((inst >> 6) & 0xFF) as usize;
        let b = ((inst >> 14) & 0x1FF) as usize;
        let c = ((inst >> 23) & 0x1FF) as usize;
        let bx = ((inst >> 14) & 0x3FFFF) as usize;
        let sbx = (bx as i32) - 131071;

        let op_name = OP_NAMES.get(opcode).copied().unwrap_or("OP_UNKNOWN");

        let disassembly = match op_name {
            "MOVE" => format!("R{} := R{}", a, b),
            "LOADK" => {
                let k_str = if let Some(k) = constants.get(bx) {
                    match k {
                        LuaConstant::String(s) => format!("\"{}\"", s),
                        LuaConstant::Number(n) => format!("{}", n),
                        LuaConstant::Bool(v) => format!("{}", v),
                        LuaConstant::Nil => "nil".into(),
                    }
                } else {
                    format!("K[{}]", bx)
                };
                format!("R{} := {}", a, k_str)
            }
            "LOADBOOL" => format!("R{} := {}", a, b != 0),
            "LOADNIL" => format!("R{}..R{} := nil", a, b),
            "GETGLOBAL" => {
                let gname = constants
                    .get(bx)
                    .and_then(|k| match k {
                        LuaConstant::String(s) => Some(s.as_str()),
                        _ => None,
                    })
                    .unwrap_or("?");
                format!("R{} := _G[\"{}\"]", a, gname)
            }
            "SETGLOBAL" => {
                let gname = constants
                    .get(bx)
                    .and_then(|k| match k {
                        LuaConstant::String(s) => Some(s.as_str()),
                        _ => None,
                    })
                    .unwrap_or("?");
                format!("_G[\"{}\"] := R{}", gname, a)
            }
            "GETTABLE" => format!("R{} := R{}[{}]", a, b, fmt_rk(c, &constants)),
            "SETTABLE" => format!(
                "R{}[{}] := {}",
                a,
                fmt_rk(b, &constants),
                fmt_rk(c, &constants)
            ),
            "NEWTABLE" => format!("R{} := {{}} (size {}, {})", a, b, c),
            "ADD" => format!(
                "R{} := {} + {}",
                a,
                fmt_rk(b, &constants),
                fmt_rk(c, &constants)
            ),
            "SUB" => format!(
                "R{} := {} - {}",
                a,
                fmt_rk(b, &constants),
                fmt_rk(c, &constants)
            ),
            "MUL" => format!(
                "R{} := {} * {}",
                a,
                fmt_rk(b, &constants),
                fmt_rk(c, &constants)
            ),
            "DIV" => format!(
                "R{} := {} / {}",
                a,
                fmt_rk(b, &constants),
                fmt_rk(c, &constants)
            ),
            "CALL" => format!(
                "CALL R{} (args: {}, returns: {})",
                a,
                b.saturating_sub(1),
                c.saturating_sub(1)
            ),
            "RETURN" => format!("RETURN R{} (count: {})", a, b.saturating_sub(1)),
            "JMP" => format!("JMP -> line {}", (pc as i32 + sbx + 2)),
            "EQ" => format!(
                "if ({} == {}) != {} then PC++",
                fmt_rk(b, &constants),
                fmt_rk(c, &constants),
                a
            ),
            _ => format!("R{} B:{} C:{} Bx:{}", a, b, c, bx),
        };

        out.push_str(&format!(
            "{indent}  [{:03}] {:<10} ; {}\n",
            pc + 1,
            op_name,
            disassembly
        ));
    }

    let num_nested = cur.read_u32::<LittleEndian>()? as usize;
    for i in 0..num_nested {
        out.push_str(&format!(
            "{indent}; --- Nested Closure Prototype [{}] ---\n",
            i
        ));
        disassemble_function_prototype(cur, out, level + 1)?;
    }

    Ok(())
}

pub fn inspect_lua_bytecode(bytecode: &[u8]) -> Result<LuaInfo> {
    validate_lua_50_header(bytecode)?;

    let mut script_name = None;
    let mut string_constants = Vec::new();

    if bytecode.len() >= 16 {
        let name_len = u32::from_le_bytes(bytecode[12..16].try_into()?) as usize;
        if name_len > 0
            && 16 + name_len <= bytecode.len()
            && let Ok(s) = std::str::from_utf8(&bytecode[16..16 + name_len])
        {
            script_name = Some(s.trim_matches(char::from(0)).to_string());
        }
    }

    let mut i = 12;
    while i + 8 <= bytecode.len() {
        let len = u32::from_le_bytes(bytecode[i..i + 4].try_into()?) as usize;
        if (3..=100).contains(&len)
            && i + 4 + len <= bytecode.len()
            && let slice = &bytecode[i + 4..i + 4 + len]
            && slice.iter().all(|&b| (0x20..=0x7E).contains(&b) || b == 0)
            && let Ok(s) = std::str::from_utf8(slice)
        {
            let clean = s.trim_matches(char::from(0)).trim();
            if clean.len() >= 3 && !string_constants.contains(&clean.to_string()) {
                string_constants.push(clean.to_string());
            }
        }
        i += 1;
    }

    Ok(LuaInfo {
        is_valid_50: true,
        script_name,
        bytecode_len: bytecode.len(),
        string_constants,
    })
}

pub fn extract_lua_bytecode(chunk_data: &[u8]) -> Result<Vec<u8>> {
    if chunk_data.starts_with(LUA_MAGIC) {
        validate_lua_50_header(chunk_data)?;
        return Ok(chunk_data.to_vec());
    }

    let (_, elements) = parse_chunk_elements(chunk_data)?;

    if let Some((_, bytecode)) = elements.iter().find(|(id, _)| *id == 23) {
        validate_lua_50_header(bytecode)?;
        return Ok(bytecode.clone());
    }

    if let Some(pos) = chunk_data.windows(4).position(|w| w == LUA_MAGIC) {
        let raw = &chunk_data[pos..];
        validate_lua_50_header(raw)?;
        return Ok(raw.to_vec());
    }

    bail!("No valid Lua 5.0.2 bytecode found in this chunk.")
}

pub fn replace_lua_bytecode(chunk_data: &[u8], new_bytecode: &[u8]) -> Result<Vec<u8>> {
    validate_lua_50_header(new_bytecode)?;

    if chunk_data.starts_with(LUA_MAGIC) {
        return Ok(new_bytecode.to_vec());
    }

    let (has_magic, mut elements) = parse_chunk_elements(chunk_data)?;
    let mut found_bytecode = false;
    let new_len = new_bytecode.len() as u32;

    for (id, data) in elements.iter_mut() {
        if *id == 22 {
            *data = new_len.to_le_bytes().to_vec();
        } else if *id == 23 {
            *data = new_bytecode.to_vec();
            found_bytecode = true;
        }
    }

    if !found_bytecode {
        elements.push((22, new_len.to_le_bytes().to_vec()));
        elements.push((23, new_bytecode.to_vec()));
    }

    Ok(build_chunk_from_elements(has_magic, &elements))
}
