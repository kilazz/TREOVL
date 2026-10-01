use super::parse_typed_container;
use anyhow::{Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;

#[derive(Debug, PartialEq)]
pub enum ShaderType {
    VertexDXBC,
    PixelDXBC,
    InternalHLSL,
}

pub fn export_shader(chunk_data: &[u8]) -> Result<(Vec<u8>, ShaderType, String)> {
    let (type_id, elements) = parse_typed_container(chunk_data)?;

    let shader_type = match type_id {
        0x00410002 => ShaderType::VertexDXBC,
        0x00410003 => ShaderType::PixelDXBC,
        0x004100BB => ShaderType::InternalHLSL,
        _ => bail!("Not a valid shader container"),
    };

    let mut largest_chunk = Vec::new();
    let mut shader_name = String::from("unknown_shader");

    for (_, chunk) in elements {
        if chunk.len() > largest_chunk.len() {
            largest_chunk = chunk.clone();
        }

        if chunk.len() >= 4 {
            let mut cur = Cursor::new(&chunk[0..4]);
            let len = cur.read_u32::<LittleEndian>()? as usize;

            if len == chunk.len() - 4
                && let Ok(s) = std::str::from_utf8(&chunk[4..])
            {
                let clean = s.trim_matches(char::from(0));
                if !clean.starts_with("[SHADERS]") && clean.len() > 2 {
                    shader_name = clean.to_string();
                }
            }
        }
    }

    if largest_chunk.is_empty() {
        bail!("Shader payload not found in container");
    }

    if shader_type == ShaderType::InternalHLSL && largest_chunk.len() > 4 {
        largest_chunk = largest_chunk[4..].to_vec();
        while largest_chunk.last() == Some(&0) {
            largest_chunk.pop();
        }
    }

    Ok((largest_chunk, shader_type, shader_name))
}
