use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, WriteBytesExt};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;

use super::builder::build_node;
use super::footer::{
    FOOTER_SIZE, MAGIC_FOOTER_1, MAGIC_FOOTER_2, calculate_triumph_crc32, check_footer,
};
use super::header::{HEADER_SIZE, PrpHeader};
use super::node::{PrpNode, parse_node};
use super::sync::{export_smart_assets, sync_assets_to_chunks};

#[derive(Serialize, Deserialize, Debug)]
pub struct ProjectManifest {
    pub header: PrpHeader,
    pub has_footer: bool,
    pub footer_hash2: u32,
    pub root: PrpNode,
}

pub fn unpack_archive(archive_path: &Path, output_dir: &Path) -> Result<(usize, String)> {
    let data = fs::read(archive_path)
        .with_context(|| format!("Failed to read archive: {:?}", archive_path))?;

    let header = PrpHeader::read(&data)?;
    let footer_opt = check_footer(&data);
    let has_footer = footer_opt.is_some();
    let footer_hash2 = footer_opt.as_ref().map(|f| f.hash2).unwrap_or(0x7C809B8B);

    let mut log = format!(
        "Magic: '{}' | Version: {}.{} | Package: '{}'\n",
        header.magic, header.major_version, header.minor_version, header.pack_name
    );

    if let Some(ref footer) = footer_opt {
        let crc_payload = &data[..data.len() - FOOTER_SIZE];
        let calc_crc = calculate_triumph_crc32(crc_payload);
        log.push_str(&format!(
            "Footer OK: Original CRC = 0x{:08X} | Calculated CRC = 0x{:08X}\n",
            footer.original_crc, calc_crc
        ));
    }

    // Create both working chunks directory and read-only vanilla baseline directory
    fs::create_dir_all(output_dir.join("chunks"))?;
    fs::create_dir_all(output_dir.join("chunks_vanilla"))?;

    let payload_end = if has_footer {
        data.len() - FOOTER_SIZE
    } else {
        data.len()
    };
    let payload = &data[HEADER_SIZE..payload_end];

    let mut chunk_counter = 0;
    let root_node = parse_node(payload, 0, true, true, &mut chunk_counter, output_dir);

    let manifest = ProjectManifest {
        header,
        has_footer,
        footer_hash2,
        root: root_node,
    };

    let manifest_path = output_dir.join("project.json");
    let file = fs::File::create(&manifest_path)?;
    serde_json::to_writer_pretty(file, &manifest)?;

    match export_smart_assets(output_dir) {
        Ok(count) => {
            log.push_str(&format!(
                "[+] Smart workspace created: {} editable assets exported to 'assets/'.\n",
                count
            ));
        }
        Err(err) => {
            log.push_str(&format!(
                "[!] Warning: Smart asset export encountered an issue: {}\n",
                err
            ));
        }
    }

    Ok((chunk_counter as usize, log))
}

pub fn pack_archive(project_dir: &Path, output_archive: &Path) -> Result<usize> {
    let manifest_path = project_dir.join("project.json");
    if !manifest_path.exists() {
        bail!(
            "Project manifest 'project.json' not found in {:?}",
            project_dir
        );
    }

    // Always inject modifications from assets/ into pure vanilla baseline chunks
    if let Ok(synced) = sync_assets_to_chunks(project_dir)
        && synced > 0
    {
        println!(
            "[*] Synced {} modified assets from 'assets/' into 'chunks/' using vanilla baseline.",
            synced
        );
    }

    let manifest_str = fs::read_to_string(&manifest_path)?;
    let manifest: ProjectManifest = serde_json::from_str(&manifest_str)?;

    let payload = build_node(&manifest.root, project_dir)?;

    let mut header_bytes = vec![0u8; HEADER_SIZE];
    let mut cur = Cursor::new(&mut header_bytes);

    let mut magic = manifest.header.magic.into_bytes();
    magic.resize(4, 0);
    cur.write_all(&magic)?;
    cur.write_u16::<LittleEndian>(manifest.header.major_version)?;
    cur.write_u16::<LittleEndian>(manifest.header.minor_version)?;
    cur.write_u32::<LittleEndian>(manifest.header.file_id)?;
    cur.write_u32::<LittleEndian>(payload.len() as u32)?;

    let mut pack_name = manifest.header.pack_name.into_bytes();
    pack_name.resize(160, 0);
    cur.write_all(&pack_name)?;

    let mut final_binary = header_bytes;
    final_binary.extend(payload);

    if manifest.has_footer {
        let crc = calculate_triumph_crc32(&final_binary);
        let mut footer = Vec::with_capacity(FOOTER_SIZE);
        footer.write_u32::<LittleEndian>(MAGIC_FOOTER_1)?;
        footer.write_u32::<LittleEndian>(MAGIC_FOOTER_2)?;
        footer.write_u32::<LittleEndian>(crc)?;
        footer.write_u32::<LittleEndian>(manifest.footer_hash2)?;
        final_binary.extend(footer);
    }

    fs::write(output_archive, &final_binary)?;
    Ok(final_binary.len())
}
