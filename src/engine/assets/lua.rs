use super::{build_typed_container, parse_typed_container};
use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

pub const LUA_MAGIC: &[u8; 4] = b"\x1bLua";
pub const LUA_VERSION_50: u8 = 0x50;

static TEMP_FILE_SEQ: AtomicU64 = AtomicU64::new(0);

/// RAII guard to guarantee temporary file cleanup across all execution paths.
struct TempFileGuard(PathBuf);

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Generates a process-unique, collision-resistant temporary file path.
fn create_unique_temp_file(prefix: &str, ext: &str) -> (PathBuf, TempFileGuard) {
    let pid = std::process::id();
    let seq = TEMP_FILE_SEQ.fetch_add(1, Ordering::Relaxed);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("{}_{}_{}_{}.{}", prefix, pid, time, seq, ext));
    let guard = TempFileGuard(path.clone());
    (path, guard)
}

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

/// Searches for the Lua 5.0 compiler (luac50.exe or luac.exe)
pub fn find_luac_executable() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("bin/luac50.exe"),
        PathBuf::from("bin/win32/luac50.exe"),
        PathBuf::from("bin/luac.exe"),
        PathBuf::from("luac50.exe"),
        PathBuf::from("luac.exe"),
    ];

    for c in &candidates {
        if c.exists() {
            return Some(c.clone());
        }
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        for c in &candidates {
            let p = parent.join(c);
            if p.exists() {
                return Some(p);
            }
        }
    }

    if Command::new("luac50").arg("-v").output().is_ok() {
        return Some(PathBuf::from("luac50"));
    }
    if Command::new("luac").arg("-v").output().is_ok() {
        return Some(PathBuf::from("luac"));
    }

    None
}

/// Searches for the Lua 5.0 decompiler (luadec50.exe or luadec.exe based on LuaDec 0.7 for Lua 5.0.2)
pub fn find_luadec_executable() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("bin/luadec50.exe"),
        PathBuf::from("bin/win32/luadec50.exe"),
        PathBuf::from("bin/luadec.exe"),
        PathBuf::from("luadec50.exe"),
        PathBuf::from("luadec.exe"),
    ];

    for c in &candidates {
        if c.exists() {
            return Some(c.clone());
        }
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        for c in &candidates {
            let p = parent.join(c);
            if p.exists() {
                return Some(p);
            }
        }
    }

    if Command::new("luadec50").output().is_ok() {
        return Some(PathBuf::from("luadec50"));
    }
    if Command::new("luadec").output().is_ok() {
        return Some(PathBuf::from("luadec"));
    }

    None
}

/// Compiles a Lua 5.0 source file (.lua) into Lua 5.0.2 bytecode using luac50.exe
pub fn compile_lua_script(source_path: &Path) -> Result<Vec<u8>> {
    let compiler = find_luac_executable().context(
        "Lua 5.0 compiler (luac50.exe) not found! Please place luac50.exe into the root folder or 'bin/' folder.",
    )?;

    let (temp_out, _guard) = create_unique_temp_file("ovl_lua_compile", "luac");

    let output = Command::new(&compiler)
        .arg("-o")
        .arg(&temp_out)
        .arg(source_path)
        .output()
        .with_context(|| format!("Failed to execute Lua compiler: {:?}", compiler))?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Lua 5.0 Compilation Error in {:?}:\n{}",
            source_path.file_name().unwrap_or_default(),
            err_msg.trim()
        );
    }

    let bytecode = fs::read(&temp_out)
        .with_context(|| format!("Failed to read compiled bytecode from {:?}", temp_out))?;

    validate_lua_50_header(&bytecode)?;
    Ok(bytecode)
}

pub fn validate_lua_50_header(bytecode: &[u8]) -> Result<()> {
    if bytecode.len() < 22 {
        bail!("Bytecode is too short to be valid Lua 5.0.");
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
    let mut raw = &data[pos..pos + len];
    if raw.ends_with(b"\x00") {
        raw = &raw[..raw.len() - 1];
    }
    let s = String::from_utf8_lossy(raw).to_string();
    cur.set_position((pos + len) as u64);
    Ok(Some(s))
}

pub fn decompile_lua_bytecode(bytecode: &[u8]) -> Result<String> {
    validate_lua_50_header(bytecode)?;

    // 1. Attempt decompilation through external LuaDec 5.0 if installed
    if let Some(luadec) = find_luadec_executable() {
        let (temp_in, _guard) = create_unique_temp_file("ovl_lua_decomp", "luac");
        if fs::write(&temp_in, bytecode).is_ok() {
            let output = Command::new(&luadec).arg(&temp_in).output();
            if let Ok(out) = output
                && out.status.success()
            {
                let code = String::from_utf8_lossy(&out.stdout).to_string();
                if !code.trim().is_empty() {
                    return Ok(code);
                }
            }
        }
    }

    // 2. Built-in AST-less pseudocode generator with warning header
    let mut cur = Cursor::new(&bytecode[22..]);
    let mut out = String::new();
    out.push_str("-- [WARNING: Built-in AST-less pseudocode generator]\n");
    out.push_str(
        "-- [Branch instructions (JMP/IF) are marked with comments and will NOT recompile directly]\n",
    );
    out.push_str(
        "-- [To enable full reversible decompilation, place 'luadec50.exe' into the 'bin/' folder]\n\n",
    );
    decompile_scope(&mut cur, &mut out, 0)?;
    Ok(out)
}

fn decompile_scope(cur: &mut Cursor<&[u8]>, out: &mut String, level: usize) -> Result<()> {
    let indent = "    ".repeat(level);
    let source = read_lua_string(cur)?.unwrap_or_default();
    let _line = cur.read_u32::<LittleEndian>()?;
    let _nups = cur.read_u8()?;
    let num_params = cur.read_u8()?;
    let _is_vararg = cur.read_u8()?;
    let _max_stack = cur.read_u8()?;

    // 1. Line info table
    let num_lines = cur.read_u32::<LittleEndian>()? as usize;
    for _ in 0..num_lines {
        let _ = cur.read_u32::<LittleEndian>()?;
    }

    // 2. Local variables debug table
    let num_locvars = cur.read_u32::<LittleEndian>()? as usize;
    let mut locvars = Vec::with_capacity(num_locvars);
    for _ in 0..num_locvars {
        let varname = read_lua_string(cur)?.unwrap_or_default();
        let startpc = cur.read_u32::<LittleEndian>()?;
        let endpc = cur.read_u32::<LittleEndian>()?;
        locvars.push((varname, startpc, endpc));
    }

    // 3. Upvalues table
    let num_upvalues = cur.read_u32::<LittleEndian>()? as usize;
    for _ in 0..num_upvalues {
        let _ = read_lua_string(cur)?;
    }

    // 4. Constants table
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

    // 5. Nested Function Prototypes
    let num_nested = cur.read_u32::<LittleEndian>()? as usize;
    let mut nested_functions = Vec::new();
    for _ in 0..num_nested {
        let mut nested_out = String::new();
        decompile_scope(cur, &mut nested_out, level + 1)?;
        nested_functions.push(nested_out);
    }

    // 6. Bytecode instructions
    let num_code = cur.read_u32::<LittleEndian>()? as usize;
    let mut code_instructions = Vec::with_capacity(num_code);
    for _ in 0..num_code {
        code_instructions.push(cur.read_u32::<LittleEndian>()?);
    }

    let fn_name = if source.is_empty() || source == "anonymous" {
        if level == 0 {
            "main".to_string()
        } else {
            format!("func_level_{}", level)
        }
    } else {
        source.clone()
    };

    let params: Vec<String> = (0..num_params)
        .map(|i| {
            locvars
                .get(i as usize)
                .map(|(name, _, _)| name.clone())
                .unwrap_or_else(|| format!("arg_{}", i))
        })
        .collect();

    if level > 0 || num_params > 0 {
        out.push_str(&format!(
            "{indent}function {}({})\n",
            fn_name,
            params.join(", ")
        ));
    }

    let mut registers: HashMap<usize, String> = HashMap::new();
    for (i, p) in params.iter().enumerate() {
        registers.insert(i, p.clone());
    }

    for (pc, &inst) in code_instructions.iter().enumerate() {
        let opcode = (inst & 0x3F) as usize;
        let c = ((inst >> 6) & 0x1FF) as usize;
        let b = ((inst >> 15) & 0x1FF) as usize;
        let a = ((inst >> 24) & 0xFF) as usize;
        let bx = ((inst >> 6) & 0x3FFFF) as usize;
        let sbx = (bx as i32) - 131071;

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
                    Some(LuaConstant::Nil) => "nil".into(),
                    None => format!("K[{}]", bx),
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
                out.push_str(&format!("{indent}{} = {}\n", gname, val));
            }
            "GETTABLE" => {
                let table = registers
                    .get(&b)
                    .cloned()
                    .unwrap_or_else(|| format!("r{}", b));
                let key = registers
                    .get(&c)
                    .cloned()
                    .unwrap_or_else(|| format!("r{}", c));
                registers.insert(a, format!("{}[{}]", table, key));
            }
            "SETTABLE" => {
                let table = registers
                    .get(&a)
                    .cloned()
                    .unwrap_or_else(|| format!("r{}", a));
                let key = registers
                    .get(&b)
                    .cloned()
                    .unwrap_or_else(|| format!("r{}", b));
                let val = registers
                    .get(&c)
                    .cloned()
                    .unwrap_or_else(|| format!("r{}", c));
                out.push_str(&format!("{indent}{}[{}] = {}\n", table, key, val));
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

                let num_results = c.saturating_sub(1);
                let call_str = format!("{}({})", func, call_args.join(", "));

                if let Some((vname, _, _)) = locvars.iter().find(|(_, start, _)| {
                    let s = *start as usize;
                    pc + 1 == s || pc == s
                }) {
                    out.push_str(&format!("{indent}local {} = {}\n", vname, call_str));
                    registers.insert(a, vname.clone());
                } else if num_results == 1 || c == 2 {
                    registers.insert(a, call_str);
                } else {
                    out.push_str(&format!("{indent}{}\n", call_str));
                }
            }
            "RETURN" => {
                let count = b.saturating_sub(1);
                if count > 0 && level > 0 {
                    let ret_val = registers.get(&a).cloned().unwrap_or_else(|| "nil".into());
                    out.push_str(&format!("{indent}return {}\n", ret_val));
                }
            }
            "JMP" => {
                let target = (pc as i32) + sbx + 2;
                out.push_str(&format!(
                    "{indent}-- [Branch] goto instruction {}\n",
                    target
                ));
            }
            _ => {}
        }
    }

    if level > 0 || num_params > 0 {
        out.push_str(&format!("{indent}end\n\n"));
    }

    for nested in nested_functions {
        out.push_str(&nested);
    }

    Ok(())
}

pub fn disassemble_lua_bytecode(bytecode: &[u8]) -> Result<String> {
    validate_lua_50_header(bytecode)?;
    let mut cur = Cursor::new(&bytecode[22..]);
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
    let nups = cur.read_u8()?;
    let num_params = cur.read_u8()?;
    let is_vararg = cur.read_u8()?;
    let max_stack = cur.read_u8()?;

    out.push_str(&format!(
        "\n{indent}; Source: {}\n{indent}; Line: {} | Nups: {} | Params: {} | Vararg: {} | MaxStack: {}\n",
        source, line_defined, nups, num_params, is_vararg, max_stack
    ));

    // 1. Lines
    let num_lines = cur.read_u32::<LittleEndian>()? as usize;
    let mut lines = Vec::with_capacity(num_lines);
    for _ in 0..num_lines {
        lines.push(cur.read_u32::<LittleEndian>()?);
    }

    // 2. LocVars
    let num_locvars = cur.read_u32::<LittleEndian>()? as usize;
    out.push_str(&format!("{indent}.locals ({})\n", num_locvars));
    for i in 0..num_locvars {
        let vname = read_lua_string(cur)?.unwrap_or_default();
        let spc = cur.read_u32::<LittleEndian>()?;
        let epc = cur.read_u32::<LittleEndian>()?;
        out.push_str(&format!(
            "{indent}  [{}] \"{}\" (pc {}..{})\n",
            i, vname, spc, epc
        ));
    }

    // 3. Upvalues
    let num_upvalues = cur.read_u32::<LittleEndian>()? as usize;
    for _ in 0..num_upvalues {
        let _ = read_lua_string(cur)?;
    }

    // 4. Constants
    let num_constants = cur.read_u32::<LittleEndian>()? as usize;
    let mut constants = Vec::with_capacity(num_constants);
    out.push_str(&format!("{indent}.constants ({})\n", num_constants));
    for i in 0..num_constants {
        let k_type = cur.read_u8()?;
        let c = match k_type {
            0 => LuaConstant::Nil,
            1 => LuaConstant::Bool(cur.read_u8()? != 0),
            3 => LuaConstant::Number(cur.read_f64::<LittleEndian>()?),
            4 => LuaConstant::String(read_lua_string(cur)?.unwrap_or_default()),
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

    // 5. Nested Prototypes
    let num_nested = cur.read_u32::<LittleEndian>()? as usize;

    // 6. Code Instructions
    let num_code = cur.read_u32::<LittleEndian>()? as usize;
    out.push_str(&format!("{indent}.code ({} instructions)\n", num_code));

    for pc in 0..num_code {
        let inst = cur.read_u32::<LittleEndian>()?;
        let opcode = (inst & 0x3F) as usize;
        let c = ((inst >> 6) & 0x1FF) as usize;
        let b = ((inst >> 15) & 0x1FF) as usize;
        let a = ((inst >> 24) & 0xFF) as usize;
        let bx = ((inst >> 6) & 0x3FFFF) as usize;
        let sbx = (bx as i32) - 131071;

        let op_name = OP_NAMES.get(opcode).copied().unwrap_or("OP_UNKNOWN");
        let line = lines.get(pc).copied().unwrap_or(0);

        let mut comment = String::new();
        if matches!(op_name, "LOADK" | "GETGLOBAL" | "SETGLOBAL") && bx < constants.len() {
            if let Some(k) = constants.get(bx) {
                match k {
                    LuaConstant::String(s) => comment = format!("; \"{}\"", s),
                    LuaConstant::Number(n) => comment = format!("; {}", n),
                    LuaConstant::Bool(v) => comment = format!("; {}", v),
                    LuaConstant::Nil => comment = "; nil".into(),
                }
            }
        } else if matches!(op_name, "JMP" | "FORLOOP" | "TFORLOOP") {
            comment = format!("; to pc={}", (pc as i32 + sbx + 2));
        }

        out.push_str(&format!(
            "{indent}  [{:3}] {:3}: {:<10} A={:<3} B={:<3} C={:<3} Bx={:<5} {}\n",
            line, pc, op_name, a, b, c, bx, comment
        ));
    }

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
    let mut cur = Cursor::new(&bytecode[22..]);
    let script_name = read_lua_string(&mut cur).unwrap_or(None);

    let mut string_constants = Vec::new();
    let mut i = 22;
    while i + 8 <= bytecode.len() {
        let len = u32::from_le_bytes(bytecode[i..i + 4].try_into().unwrap_or_default()) as usize;
        if (3..=120).contains(&len)
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

    if let Ok((_, elements)) = parse_typed_container(chunk_data)
        && let Some((_, bytecode)) = elements.iter().find(|(id, _)| *id == 23)
        && validate_lua_50_header(bytecode).is_ok()
    {
        return Ok(bytecode.clone());
    }

    if let Some(pos) = chunk_data.windows(4).position(|w| w == LUA_MAGIC) {
        if pos >= 4 {
            let declared_len =
                u32::from_le_bytes(chunk_data[pos - 4..pos].try_into().unwrap_or_default())
                    as usize;
            if declared_len > 12 && pos + declared_len <= chunk_data.len() {
                let bytecode = &chunk_data[pos..pos + declared_len];
                if validate_lua_50_header(bytecode).is_ok() {
                    return Ok(bytecode.to_vec());
                }
            }
        }

        let bytecode = &chunk_data[pos..];
        if validate_lua_50_header(bytecode).is_ok() {
            return Ok(bytecode.to_vec());
        }
    }

    bail!("No valid Lua 5.0.2 bytecode found in this chunk.")
}

pub fn replace_lua_bytecode(chunk_data: &[u8], new_bytecode: &[u8]) -> Result<Vec<u8>> {
    validate_lua_50_header(new_bytecode)?;

    if chunk_data.starts_with(LUA_MAGIC) {
        return Ok(new_bytecode.to_vec());
    }

    if let Some(pos) = chunk_data.windows(4).position(|w| w == LUA_MAGIC) {
        if pos >= 4 {
            let declared_len =
                u32::from_le_bytes(chunk_data[pos - 4..pos].try_into().unwrap_or_default())
                    as usize;

            if pos + declared_len <= chunk_data.len() {
                let mut new_chunk = chunk_data[..pos - 4].to_vec();
                new_chunk.extend_from_slice(&(new_bytecode.len() as u32).to_le_bytes());
                new_chunk.extend_from_slice(new_bytecode);
                new_chunk.extend_from_slice(&chunk_data[pos + declared_len..]);
                return Ok(new_chunk);
            }
        }

        let mut new_chunk = chunk_data[..pos].to_vec();
        new_chunk.extend_from_slice(new_bytecode);
        return Ok(new_chunk);
    }

    let new_len = new_bytecode.len() as u32;
    if let Ok((type_id, mut elements)) = parse_typed_container(chunk_data) {
        let mut found_bytecode = false;
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
            elements.sort_by_key(|e| e.0);
        }
        return Ok(build_typed_container(type_id, &elements));
    }

    bail!("Could not find Lua bytecode to replace");
}
