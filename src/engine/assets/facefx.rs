use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use super::parse_typed_container;
use crate::engine::common::read_length_prefixed_string;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FaceFxJson {
    pub type_id_hex: String,
    pub resource_path: String,
    pub actor_name: String,
    pub fxe_filename: String,
    pub fxe_size: usize,
}

pub fn export_facefx_to_json(chunk_data: &[u8], assets_dir: &Path, stem: &str) -> Result<String> {
    if chunk_data.len() < 5 {
        bail!("Chunk data too short to be a FaceFX container");
    }

    let (type_id, elements) =
        parse_typed_container(chunk_data).context("Failed to parse FaceFX typed container")?;

    let mut resource_path = String::new();
    let mut actor_name = String::new();
    let mut fxe_filename = String::new();
    let mut fxe_payload = Vec::new();

    for (id, chunk) in elements {
        match id {
            20 => {
                if let Some(s) = read_length_prefixed_string(&chunk) {
                    resource_path = s;
                }
            }
            21 => {
                if let Some(s) = read_length_prefixed_string(&chunk) {
                    actor_name = s;
                }
            }
            40 => {
                if let Some(s) = read_length_prefixed_string(&chunk) {
                    fxe_filename = s;
                }
            }
            41 | 30 | 22 => {
                if let Some(pos) = chunk.windows(4).position(|w| w == b"FACE") {
                    fxe_payload = chunk[pos..].to_vec();
                } else if chunk.len() > 100 {
                    fxe_payload = chunk;
                }
            }
            _ => {}
        }
    }

    if fxe_filename.is_empty() {
        fxe_filename = if !actor_name.is_empty() {
            format!("{}.fxe", actor_name)
        } else {
            format!("{}.fxe", stem)
        };
    }

    if !fxe_payload.is_empty() {
        let facefx_dir = assets_dir.join("facefx");
        fs::create_dir_all(&facefx_dir)?;
        let fxe_out_path = facefx_dir.join(&fxe_filename);
        fs::write(&fxe_out_path, &fxe_payload)?;
    }

    let fx_json = FaceFxJson {
        type_id_hex: format!("{:08X}", type_id),
        resource_path,
        actor_name,
        fxe_filename,
        fxe_size: fxe_payload.len(),
    };

    serde_json::to_string_pretty(&fx_json).map_err(|e| anyhow::anyhow!(e))
}
