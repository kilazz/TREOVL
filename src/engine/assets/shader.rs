use super::parse_typed_container;
use crate::engine::common::read_length_prefixed_string;
use anyhow::{Result, bail};

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

        if let Some(s) = read_length_prefixed_string(&chunk)
            && !s.starts_with("[SHADERS]")
            && s.len() > 2
        {
            shader_name = s;
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
