use anyhow::Result;

pub fn extract_xml_payload(data: &[u8]) -> &[u8] {
    if let Some(pos) = data.windows(5).position(|w| w == b"<?xml") {
        &data[pos..]
    } else {
        data
    }
}

pub fn import_xml_payload(baseline_chunk: &[u8], asset_bytes: &[u8]) -> Result<Vec<u8>> {
    if let Some(pos) = baseline_chunk.windows(5).position(|w| w == b"<?xml") {
        let mut out = baseline_chunk[..pos].to_vec();
        out.extend_from_slice(asset_bytes);
        Ok(out)
    } else if baseline_chunk.len() > 4 {
        let mut buf = Vec::with_capacity(4 + asset_bytes.len());
        buf.extend_from_slice(&(asset_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(asset_bytes);
        Ok(buf)
    } else {
        Ok(asset_bytes.to_vec())
    }
}
