use anyhow::{Context, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use super::{build_chunk_from_elements, parse_chunk_elements};
use crate::engine::common::{read_length_prefixed_string, write_length_prefixed_string};
use crate::engine::container::footer::{
    FOOTER_SIZE, MAGIC_FOOTER_1, MAGIC_FOOTER_2, calculate_triumph_crc32, check_footer,
};
use crate::engine::container::header::{HEADER_SIZE, PrpHeader};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VoicePackageJson {
    pub _engine_metadata: VoicePackageMetadataJson,
    pub package_name: String,
    pub voice_bank_id: u32,
    pub base_voice_clb: String,
    pub voice_cues: Vec<VoiceCueJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct VoicePackageMetadataJson {
    pub magic: String,
    pub major_version: u16,
    pub minor_version: u16,
    pub file_id: u32,
    pub footer_crc_hex: String,
    pub footer_hash2_hex: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VoiceCueJson {
    pub cue_id: u32,
    pub sample_id: String,
    pub flags: String,
}

pub fn export_vpk_to_json(data: &[u8]) -> Result<String> {
    let (payload, header_opt, footer_opt) = if data.len() >= HEADER_SIZE + FOOTER_SIZE
        && (data.starts_with(b"RPK\0") || data.starts_with(b"PRP\0"))
    {
        let hdr = PrpHeader::read(data)?;
        let ftr = check_footer(data);
        let end = data.len().saturating_sub(FOOTER_SIZE);
        (&data[HEADER_SIZE..end], Some(hdr), ftr)
    } else {
        (data, None, None)
    };

    let (_, elements) =
        parse_chunk_elements(payload).context("Failed to parse Voice Package container table")?;

    let mut voice_bank_id = 0u32;
    let mut base_voice_clb = String::new();
    let mut package_name = String::from("Minion Voices");
    let mut voice_cues = Vec::new();

    for (id, chunk) in elements {
        match id {
            20 if chunk.len() >= 4 => {
                voice_bank_id = Cursor::new(&chunk).read_u32::<LittleEndian>().unwrap_or(0);
            }
            21 => {
                // Skips 4-byte prefix if present before string length
                let str_slice = if chunk.len() > 8 && chunk[0..4] == [1, 0, 0, 0] {
                    &chunk[4..]
                } else {
                    &chunk[..]
                };
                if let Some(s) = read_length_prefixed_string(str_slice) {
                    base_voice_clb = s;
                }
            }
            22 => {
                if let Some(s) = read_length_prefixed_string(&chunk) {
                    package_name = s;
                }
            }
            23..=100 => {
                if let Ok((_, sub_elems)) = parse_chunk_elements(&chunk) {
                    let mut sample_id = String::new();
                    let mut flags = String::from("01010000");

                    for (sid, sdata) in sub_elems {
                        if sid == 20 {
                            if let Some(s) = read_length_prefixed_string(&sdata) {
                                sample_id = s;
                            }
                        } else if sid == 21 || sid == 30 {
                            flags = hex::encode_upper(&sdata);
                        }
                    }

                    if !sample_id.is_empty() {
                        voice_cues.push(VoiceCueJson {
                            cue_id: id,
                            sample_id,
                            flags,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    voice_cues.sort_by_key(|c| c.cue_id);

    let metadata = VoicePackageMetadataJson {
        magic: header_opt
            .as_ref()
            .map(|h| h.magic.clone())
            .unwrap_or_else(|| "RPK".into()),
        major_version: header_opt.as_ref().map(|h| h.major_version).unwrap_or(6),
        minor_version: header_opt.as_ref().map(|h| h.minor_version).unwrap_or(70),
        file_id: header_opt
            .as_ref()
            .map(|h| h.file_id)
            .unwrap_or(voice_bank_id),
        footer_crc_hex: footer_opt
            .as_ref()
            .map(|f| format!("0x{:08X}", f.original_crc))
            .unwrap_or_default(),
        footer_hash2_hex: footer_opt
            .as_ref()
            .map(|f| format!("0x{:08X}", f.hash2))
            .unwrap_or_else(|| "0xFFFFFFFF".into()),
    };

    let vpk_json = VoicePackageJson {
        _engine_metadata: metadata,
        package_name,
        voice_bank_id,
        base_voice_clb,
        voice_cues,
    };

    serde_json::to_string_pretty(&vpk_json).context("Failed to serialize Voice Package JSON")
}

pub fn import_vpk_from_json(json_str: &str, baseline: &[u8]) -> Result<Vec<u8>> {
    let parsed: VoicePackageJson = serde_json::from_str(json_str)?;

    let mut root_elements = Vec::new();

    // ID 20: Bank ID
    root_elements.push((20, parsed.voice_bank_id.to_le_bytes().to_vec()));

    // ID 21: Base voice clb path with 4-byte prefix
    let mut clb_data = vec![1u8, 0, 0, 0];
    clb_data.extend_from_slice(&write_length_prefixed_string(&parsed.base_voice_clb));
    root_elements.push((21, clb_data));

    // ID 22: Package name
    root_elements.push((22, write_length_prefixed_string(&parsed.package_name)));

    // IDs 23+: Voice cues
    for cue in parsed.voice_cues {
        let flag_bytes = hex::decode(&cue.flags).unwrap_or_else(|_| vec![1, 1, 0, 0]);
        let sub_elements = vec![
            (20, write_length_prefixed_string(&cue.sample_id)),
            (21, flag_bytes.clone()),
            (30, flag_bytes),
        ];
        let sub_container = build_chunk_from_elements(false, &sub_elements);
        root_elements.push((cue.cue_id, sub_container));
    }

    root_elements.sort_by_key(|&(id, _)| id);
    let payload_bytes = build_chunk_from_elements(false, &root_elements);

    // If baseline has full PrpHeader (176 bytes), reconstruct full package with header and footer
    if baseline.len() >= HEADER_SIZE + FOOTER_SIZE
        && (baseline.starts_with(b"RPK\0") || baseline.starts_with(b"PRP\0"))
    {
        let base_hdr = PrpHeader::read(baseline)?;
        let header_bytes = base_hdr.write(payload_bytes.len() as u32)?;

        let mut final_package = Vec::with_capacity(HEADER_SIZE + payload_bytes.len() + FOOTER_SIZE);
        final_package.extend_from_slice(&header_bytes);
        final_package.extend_from_slice(&payload_bytes);

        let footer_crc = calculate_triumph_crc32(&final_package);
        let hash2 = u32::from_str_radix(
            parsed
                ._engine_metadata
                .footer_hash2_hex
                .trim_start_matches("0x"),
            16,
        )
        .unwrap_or(0xFFFFFFFF);

        final_package.write_u32::<LittleEndian>(MAGIC_FOOTER_1)?;
        final_package.write_u32::<LittleEndian>(MAGIC_FOOTER_2)?;
        final_package.write_u32::<LittleEndian>(footer_crc)?;
        final_package.write_u32::<LittleEndian>(hash2)?;

        return Ok(final_package);
    }

    Ok(payload_bytes)
}
