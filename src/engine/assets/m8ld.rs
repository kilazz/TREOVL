use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const M8LD_MAGIC: &[u8; 4] = b"M8LD";

/// Official 7-byte repeating XOR key recovered directly from CD Projekt's O2Tools.exe (address 0x0045E7F8)
pub const M8LD_KEY: [u8; 7] = [0x4F, 0x4E, 0xE2, 0x99, 0xA5, 0x4D, 0x38];

/// Sidecar companion metadata stored in `<name>.meta.json` alongside `<name>.xml`
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct M8ldMetaJson {
    pub magic: String,
    pub crc_or_flags_hex: String,
    pub original_payload_size: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unmapped_binary_hex: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct M8ldEntryJson {
    pub name: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct M8ldPackageJson {
    pub magic: String,
    pub crc_or_flags_hex: String,
    pub language_id: String,
    pub raw_xml: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<M8ldEntryJson>,
}

/// Compatibility pass-through for pipeline
pub fn align_xml_token_stream(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().copied().filter(|&b| b != 0x00).collect()
}

/// Official CD Projekt stream decryption for Triumph Engine .8ld containers.
pub fn decrypt_triumph_8ld_stream(ciphertext: &[u8], seed: u32) -> Vec<u8> {
    let mut key_idx = (seed % 7) as usize;
    let mut plaintext = Vec::with_capacity(ciphertext.len());

    for &b in ciphertext {
        plaintext.push(b ^ M8LD_KEY[key_idx]);
        key_idx = if key_idx < 6 { key_idx + 1 } else { 0 };
    }

    plaintext
}

/// Official CD Projekt stream encryption for Triumph Engine .8ld containers.
pub fn encrypt_triumph_8ld_stream(plaintext: &[u8], seed: u32) -> Vec<u8> {
    let mut key_idx = (seed % 7) as usize;
    let mut ciphertext = Vec::with_capacity(plaintext.len());

    for &b in plaintext {
        ciphertext.push(b ^ M8LD_KEY[key_idx]);
        key_idx = if key_idx < 6 { key_idx + 1 } else { 0 };
    }

    ciphertext
}

/// Decompiles an .8ld binary container into clean, native XML text.
pub fn decompile_8ld_to_xml(data: &[u8]) -> Result<(u32, String)> {
    if data.len() < 5 {
        bail!("Data is too short for a valid .8ld container (minimum 5 bytes required)");
    }

    let (seed_byte, ciphertext) = if data.starts_with(M8LD_MAGIC) {
        // Offset 0..4: "M8LD"
        // Offset 4: 1-byte seed for the XOR key rotation
        // Offset 5..: XOR encrypted XML stream
        (data[4] as u32, &data[5..])
    } else {
        (0u32, data)
    };

    // If it's already an unencrypted XML file
    if let Ok(text) = std::str::from_utf8(ciphertext)
        && (text.contains("<?xml") || text.contains("<Workbook") || text.contains("<Language"))
    {
        return Ok((seed_byte, text.to_string()));
    }

    // Decrypt using official CD Projekt algorithm
    let decrypted_bytes = decrypt_triumph_8ld_stream(ciphertext, seed_byte);
    let xml_text = String::from_utf8_lossy(&decrypted_bytes).to_string();

    Ok((seed_byte, xml_text))
}

pub fn decompile_8ld_to_xml_file(data: &[u8]) -> Result<String> {
    let (_, xml_content) = decompile_8ld_to_xml(data)?;
    Ok(xml_content)
}

/// Compiles an XML string back into an .8ld binary container.
pub fn compile_xml_to_8ld(xml_text: &str, seed: u32) -> Vec<u8> {
    let seed_byte = (seed & 0xFF) as u8;
    let mut out = Vec::with_capacity(5 + xml_text.len());

    // 1. Header "M8LD" (4 bytes)
    out.extend_from_slice(M8LD_MAGIC);
    // 2. Seed byte (1 byte)
    out.push(seed_byte);
    // 3. Encrypted XML stream
    let encrypted = encrypt_triumph_8ld_stream(xml_text.as_bytes(), seed_byte as u32);
    out.extend_from_slice(&encrypted);

    out
}

pub fn compile_xml_to_8ld_file(xml_text: &str, seed_override: Option<u32>) -> Result<Vec<u8>> {
    let seed = seed_override.unwrap_or(0x91);
    Ok(compile_xml_to_8ld(xml_text, seed))
}

pub fn export_m8ld_to_json(data: &[u8]) -> Result<String> {
    let (seed, raw_xml) = decompile_8ld_to_xml(data)?;
    let (lang_id, entries) = parse_xml_entries(&raw_xml);

    let output = M8ldPackageJson {
        magic: "M8LD".to_string(),
        crc_or_flags_hex: format!("0x{:02X}", seed),
        language_id: lang_id,
        raw_xml,
        entries,
    };

    serde_json::to_string_pretty(&output).context("Failed to serialize M8LD JSON")
}

pub fn import_m8ld_from_json(json_or_xml_str: &str) -> Result<Vec<u8>> {
    let trimmed = json_or_xml_str.trim();

    // Direct XML passthrough
    if trimmed.starts_with("<?xml")
        || trimmed.starts_with("<Language")
        || trimmed.starts_with("<Workbook")
    {
        return Ok(compile_xml_to_8ld(trimmed, 0x91));
    }

    if let Ok(parsed) = serde_json::from_str::<M8ldPackageJson>(json_or_xml_str) {
        let seed = u32::from_str_radix(parsed.crc_or_flags_hex.trim_start_matches("0x"), 16)
            .unwrap_or(0x91);

        let xml_payload = if !parsed.raw_xml.trim().is_empty() {
            parsed.raw_xml
        } else {
            build_clean_xml(&parsed.language_id, &parsed.entries)
        };

        return Ok(compile_xml_to_8ld(&xml_payload, seed));
    }

    Ok(compile_xml_to_8ld(trimmed, 0x91))
}

fn parse_xml_entries(xml: &str) -> (String, Vec<M8ldEntryJson>) {
    let mut entries = Vec::new();
    let mut language_id = String::from("English");

    if let Some(pos) = xml.find("<Language")
        && let Some(id_start) = xml[pos..].find("id=\"")
    {
        let start = pos + id_start + 4;
        if let Some(end) = xml[start..].find('\"') {
            let id = &xml[start..start + end];
            if !id.trim().is_empty() {
                language_id = id.trim().to_string();
            }
        }
    }

    let mut current_name = String::new();

    for line in xml.lines() {
        let trimmed = line.trim();

        if let Some(pos) = trimmed.find("<Entry name=\"") {
            let start = pos + 13;
            if let Some(end) = trimmed[start..].find('\"') {
                current_name = trimmed[start..start + end].to_string();
            }
        } else if let Some(start_t) = trimmed.find("<Text>")
            && let Some(end_t) = trimmed.find("</Text>")
        {
            let text = &trimmed[start_t + 6..end_t];
            if !current_name.is_empty() {
                entries.push(M8ldEntryJson {
                    name: current_name.clone(),
                    text: text.to_string(),
                });
                current_name.clear();
            }
        }
    }

    (language_id, entries)
}

fn build_clean_xml(lang_id: &str, entries: &[M8ldEntryJson]) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str(&format!("<Language id=\"{}\">\n", lang_id));
    for entry in entries {
        xml.push_str(&format!("  <Entry name=\"{}\">\n", entry.name));
        xml.push_str(&format!("    <Text>{}</Text>\n", entry.text));
        xml.push_str("  </Entry>\n");
    }
    xml.push_str("</Language>\n");
    xml
}
