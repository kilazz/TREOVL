use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Write};

use crate::engine::common::{Endian, detect_endianness};

pub const HEADER_SIZE: usize = 176;

#[derive(Debug, Serialize, Deserialize)]
pub struct PrpHeader {
    pub magic: String,
    pub major_version: u16,
    pub minor_version: u16,
    pub file_id: u32,
    pub data_size: u32,
    pub pack_name: String,
    #[serde(default)]
    pub endian: Endian,
}

impl PrpHeader {
    pub fn read(data: &[u8]) -> Result<Self> {
        if data.len() < HEADER_SIZE {
            bail!(
                "Package file is too small ({} bytes) to contain a valid header",
                data.len()
            );
        }

        let endian = detect_endianness(data);
        let mut cur = Cursor::new(&data[..HEADER_SIZE]);
        let mut magic_bytes = [0u8; 4];
        cur.read_exact(&mut magic_bytes)?;

        let magic = String::from_utf8_lossy(&magic_bytes)
            .trim_matches(char::from(0))
            .to_string();

        let major_version = endian.read_u16(&mut cur)?;
        let minor_version = endian.read_u16(&mut cur)?;
        let file_id = endian.read_u32(&mut cur)?;
        let data_size = endian.read_u32(&mut cur)?;

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
            endian,
        })
    }

    pub fn write(&self, payload_size: u32) -> Result<Vec<u8>> {
        let mut header_bytes = vec![0u8; HEADER_SIZE];
        let mut cur = Cursor::new(&mut header_bytes);

        let mut magic = self.magic.clone().into_bytes();
        magic.resize(4, 0);
        cur.write_all(&magic)?;
        self.endian.write_u16(&mut cur, self.major_version)?;
        self.endian.write_u16(&mut cur, self.minor_version)?;
        self.endian.write_u32(&mut cur, self.file_id)?;
        self.endian.write_u32(&mut cur, payload_size)?;

        let mut pack_name = self.pack_name.clone().into_bytes();
        pack_name.resize(160, 0);
        cur.write_all(&pack_name)?;

        Ok(header_bytes)
    }
}
