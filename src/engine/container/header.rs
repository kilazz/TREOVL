use anyhow::{Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};

pub const HEADER_SIZE: usize = 176;

#[derive(Debug, Serialize, Deserialize)]
pub struct PrpHeader {
    pub magic: String,
    pub major_version: u16,
    pub minor_version: u16,
    pub file_id: u32,
    pub data_size: u32,
    pub pack_name: String,
}

impl PrpHeader {
    pub fn read(data: &[u8]) -> Result<Self> {
        if data.len() < HEADER_SIZE {
            bail!(
                "Package file is too small ({} bytes) to contain a valid header",
                data.len()
            );
        }

        let mut cur = Cursor::new(&data[..HEADER_SIZE]);
        let mut magic_bytes = [0u8; 4];
        cur.read_exact(&mut magic_bytes)?;

        let magic = String::from_utf8_lossy(&magic_bytes)
            .trim_matches(char::from(0))
            .to_string();
        let major_version = cur.read_u16::<LittleEndian>()?;
        let minor_version = cur.read_u16::<LittleEndian>()?;
        let file_id = cur.read_u32::<LittleEndian>()?;
        let data_size = cur.read_u32::<LittleEndian>()?;

        let mut name_bytes = [0u8; 160];
        cur.read_exact(&mut name_bytes)?;
        let pack_name = String::from_utf8_lossy(&name_bytes)
            .trim_matches(char::from(0))
            .to_string();

        Ok(Self {
            magic,
            major_version,
            minor_version,
            file_id,
            data_size,
            pack_name,
        })
    }
}
