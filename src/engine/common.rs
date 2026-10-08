use anyhow::{Result, bail};
use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use crc32fast::Hasher;
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
pub mod magic {
    pub const TEX_3D: &[u8; 4] = b"\x3D\x00\x41\x00";
    pub const TEX_CUBEMAP: &[u8; 4] = b"\x99\x00\x41\x00";
    pub const TEX_INTERFACE: &[u8; 4] = b"\x98\x00\x41\x00";
    pub const TEX_MIPMAP: &[u8; 4] = b"\x24\x00\x41\x00";
    pub const AUDIO_WAV: &[u8; 4] = b"\x00\x00\xA1\x00";
    pub const MESH: &[u8; 4] = b"\x35\x00\x41\x00";
    pub const ANIM_CLIP: &[u8; 4] = b"\x05\x00\x41\x00";
    pub const ANIM_TRACK: &[u8; 4] = b"\x07\x00\x41\x00";
    pub const OBJECT: &[u8; 4] = b"\x4B\x00\x41\x00";
    pub const LUA: &[u8; 4] = b"\x1bLua";
    pub const EVENT: &[u8; 4] = b"\xB0\x00\x00\x04";
    pub const CONTAINER_MAGIC: &[u8; 3] = b"\x01\x01\x00";
}

#[allow(dead_code)]
pub mod chunk_id {
    pub const SUB_CONTAINER: u32 = 1;
    pub const INDEX_DATA: u32 = 10;
    pub const VERTEX_DATA: u32 = 11;
    pub const TAG_STRING: u32 = 20;
    pub const NAME_STRING: u32 = 21;
    pub const DATA_BLOB: u32 = 22;
    pub const FORMAT: u32 = 23;
    pub const WIDTH: u32 = 30;
    pub const HEIGHT: u32 = 31;
    pub const OBJECT_BONES: u32 = 33;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Endian {
    #[default]
    Little, // PC (Intel/AMD)
    Big, // Xbox 360 (PowerPC) / PlayStation 3 (Cell)
}

impl Endian {
    pub fn read_i16<R: std::io::Read>(self, reader: &mut R) -> Result<i16, std::io::Error> {
        match self {
            Endian::Little => reader.read_i16::<LittleEndian>(),
            Endian::Big => reader.read_i16::<BigEndian>(),
        }
    }

    pub fn read_u16<R: std::io::Read>(self, reader: &mut R) -> Result<u16, std::io::Error> {
        match self {
            Endian::Little => reader.read_u16::<LittleEndian>(),
            Endian::Big => reader.read_u16::<BigEndian>(),
        }
    }

    pub fn read_i32<R: std::io::Read>(self, reader: &mut R) -> Result<i32, std::io::Error> {
        match self {
            Endian::Little => reader.read_i32::<LittleEndian>(),
            Endian::Big => reader.read_i32::<BigEndian>(),
        }
    }

    pub fn read_u32<R: std::io::Read>(self, reader: &mut R) -> Result<u32, std::io::Error> {
        match self {
            Endian::Little => reader.read_u32::<LittleEndian>(),
            Endian::Big => reader.read_u32::<BigEndian>(),
        }
    }

    pub fn read_f32<R: std::io::Read>(self, reader: &mut R) -> Result<f32, std::io::Error> {
        match self {
            Endian::Little => reader.read_f32::<LittleEndian>(),
            Endian::Big => reader.read_f32::<BigEndian>(),
        }
    }

    pub fn f32_from_bytes(self, b: [u8; 4]) -> f32 {
        match self {
            Endian::Little => f32::from_le_bytes(b),
            Endian::Big => f32::from_be_bytes(b),
        }
    }

    pub fn u32_from_bytes(self, b: [u8; 4]) -> u32 {
        match self {
            Endian::Little => u32::from_le_bytes(b),
            Endian::Big => u32::from_be_bytes(b),
        }
    }

    pub fn i32_from_bytes(self, b: [u8; 4]) -> i32 {
        match self {
            Endian::Little => i32::from_le_bytes(b),
            Endian::Big => i32::from_be_bytes(b),
        }
    }

    pub fn u16_from_bytes(self, b: [u8; 2]) -> u16 {
        match self {
            Endian::Little => u16::from_le_bytes(b),
            Endian::Big => u16::from_be_bytes(b),
        }
    }

    pub fn i16_from_bytes(self, b: [u8; 2]) -> i16 {
        match self {
            Endian::Little => i16::from_le_bytes(b),
            Endian::Big => i16::from_be_bytes(b),
        }
    }

    pub fn write_u16(self, cur: &mut impl std::io::Write, val: u16) -> Result<(), std::io::Error> {
        match self {
            Endian::Little => byteorder::WriteBytesExt::write_u16::<LittleEndian>(cur, val),
            Endian::Big => byteorder::WriteBytesExt::write_u16::<BigEndian>(cur, val),
        }
    }

    pub fn write_u32(self, cur: &mut impl std::io::Write, val: u32) -> Result<(), std::io::Error> {
        match self {
            Endian::Little => byteorder::WriteBytesExt::write_u32::<LittleEndian>(cur, val),
            Endian::Big => byteorder::WriteBytesExt::write_u32::<BigEndian>(cur, val),
        }
    }

    pub fn write_f32(self, cur: &mut impl std::io::Write, val: f32) -> Result<(), std::io::Error> {
        match self {
            Endian::Little => byteorder::WriteBytesExt::write_f32::<LittleEndian>(cur, val),
            Endian::Big => byteorder::WriteBytesExt::write_f32::<BigEndian>(cur, val),
        }
    }

    pub fn u32_to_bytes(self, val: u32) -> [u8; 4] {
        match self {
            Endian::Little => val.to_le_bytes(),
            Endian::Big => val.to_be_bytes(),
        }
    }

    pub fn i32_to_bytes(self, val: i32) -> [u8; 4] {
        match self {
            Endian::Little => val.to_le_bytes(),
            Endian::Big => val.to_be_bytes(),
        }
    }

    pub fn f32_to_bytes(self, val: f32) -> [u8; 4] {
        match self {
            Endian::Little => val.to_le_bytes(),
            Endian::Big => val.to_be_bytes(),
        }
    }

    pub fn u16_to_bytes(self, val: u16) -> [u8; 2] {
        match self {
            Endian::Little => val.to_le_bytes(),
            Endian::Big => val.to_be_bytes(),
        }
    }

    pub fn i16_to_bytes(self, val: i16) -> [u8; 2] {
        match self {
            Endian::Little => val.to_le_bytes(),
            Endian::Big => val.to_be_bytes(),
        }
    }

    pub fn write_length_prefixed_string(self, s: &str) -> Vec<u8> {
        let bytes = s.as_bytes();
        let mut out = Vec::with_capacity(4 + bytes.len());
        let _ = self.write_u32(&mut out, bytes.len() as u32);
        out.extend_from_slice(bytes);
        out
    }
}

pub fn detect_endianness(header_bytes: &[u8]) -> Endian {
    if header_bytes.len() < 16 {
        return Endian::Little;
    }

    let size_le = u32::from_le_bytes(header_bytes[12..16].try_into().unwrap_or_default());
    if size_le > 0x7FFF_FFFF {
        Endian::Big
    } else {
        Endian::Little
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ContainerTableEntry {
    pub id: u32,
    pub offset: usize,
    pub is_large: bool,
}

#[derive(Debug)]
pub struct ParsedContainerTable {
    pub has_magic: bool,
    pub data_start: usize,
    pub entries: Vec<ContainerTableEntry>,
    pub endian: Endian,
}

pub fn parse_raw_container_table(
    data: &[u8],
    pos: usize,
    allow_magic: bool,
) -> Result<ParsedContainerTable> {
    let mut endian = Endian::Little;

    let mut temp_pos = pos;
    if allow_magic
        && data.len() >= temp_pos + 3
        && &data[temp_pos..temp_pos + 3] == magic::CONTAINER_MAGIC
    {
        temp_pos += 3;
    }

    if data.len() > temp_pos + 4 {
        let control_byte = data[temp_pos];
        if (control_byte & 0x80) != 0 {
            let l_le = u32::from_le_bytes(
                data[temp_pos + 1..temp_pos + 5]
                    .try_into()
                    .unwrap_or_default(),
            );
            let l_be = u32::from_be_bytes(
                data[temp_pos + 1..temp_pos + 5]
                    .try_into()
                    .unwrap_or_default(),
            );
            if l_le > 10_000 && l_be < 10_000 {
                endian = Endian::Big;
            }
        }
    }

    parse_raw_container_table_with_endian(data, pos, allow_magic, endian)
}

pub fn parse_raw_container_table_with_endian(
    data: &[u8],
    mut pos: usize,
    allow_magic: bool,
    endian: Endian,
) -> Result<ParsedContainerTable> {
    let mut has_magic = false;
    if allow_magic && data.len() >= pos + 3 && &data[pos..pos + 3] == magic::CONTAINER_MAGIC {
        has_magic = true;
        pos += 3;
    }

    if pos >= data.len() {
        bail!("Container header truncated");
    }

    let control_byte = data[pos];
    let has_large = (control_byte & 0x80) != 0;
    let small_count = (control_byte & 0x7F) as usize;
    pos += 1;

    let mut large_count = 0;
    if has_large {
        if pos + 4 > data.len() {
            bail!("Corrupted container header: truncated large count");
        }
        let mut cur = std::io::Cursor::new(&data[pos..pos + 4]);
        large_count = endian.read_u32(&mut cur)? as usize;
        pos += 4;
    }

    let total_entries = small_count + large_count;
    if total_entries == 0 || total_entries > 4096 {
        bail!("Invalid entry count in container table: {}", total_entries);
    }

    let table_size = (small_count * 2) + (large_count * 8);
    let data_start = pos + table_size;
    if data_start > data.len() {
        bail!("Table offsets exceed data bounds");
    }

    let mut cur = std::io::Cursor::new(&data[pos..data_start]);
    let mut entries = Vec::with_capacity(total_entries);

    for _ in 0..small_count {
        let id = cur.read_u8()? as u32;
        let offset = cur.read_u8()? as usize;
        entries.push(ContainerTableEntry {
            id,
            offset,
            is_large: false,
        });
    }

    for _ in 0..large_count {
        let id = endian.read_u32(&mut cur)?;
        let offset = endian.read_u32(&mut cur)? as usize;
        entries.push(ContainerTableEntry {
            id,
            offset,
            is_large: true,
        });
    }

    entries.sort_by_key(|e| e.offset);

    if entries.is_empty() || entries[0].offset != 0 {
        bail!("First table offset must be 0");
    }

    Ok(ParsedContainerTable {
        has_magic,
        data_start,
        entries,
        endian,
    })
}

pub fn extract_slices_from_table<'a>(
    data: &'a [u8],
    table: &ParsedContainerTable,
) -> Vec<(u32, bool, &'a [u8])> {
    let mut elements = Vec::with_capacity(table.entries.len());
    for i in 0..table.entries.len() {
        let entry = &table.entries[i];
        let start = table.data_start + entry.offset;
        let end = if i + 1 < table.entries.len() {
            table.data_start + table.entries[i + 1].offset
        } else {
            data.len()
        };

        if start <= data.len() && end <= data.len() && start <= end {
            elements.push((entry.id, entry.is_large, &data[start..end]));
        }
    }
    elements
}

pub fn extract_elements_from_table(
    data: &[u8],
    table: &ParsedContainerTable,
) -> Vec<(u32, Vec<u8>)> {
    extract_slices_from_table(data, table)
        .into_iter()
        .map(|(id, _, slice)| (id, slice.to_vec()))
        .collect()
}

pub fn serialize_container_payload<'a, I>(elements: I) -> Vec<u8>
where
    I: IntoIterator<Item = (u32, bool, &'a [u8])>,
{
    serialize_container_payload_with_endian(elements, Endian::Little)
}

pub fn serialize_container_payload_with_endian<'a, I>(elements: I, endian: Endian) -> Vec<u8>
where
    I: IntoIterator<Item = (u32, bool, &'a [u8])>,
{
    let mut small_entries = Vec::new();
    let mut large_entries = Vec::new();
    let mut current_offset = 0usize;
    let mut data_segment = Vec::new();

    for (id, force_large, data) in elements {
        if !force_large && id <= 255 && current_offset <= 255 {
            small_entries.push((id as u8, current_offset as u8));
        } else {
            large_entries.push((id, current_offset as u32));
        }
        data_segment.extend_from_slice(data);
        current_offset += data.len();
    }

    let has_large = !large_entries.is_empty();
    let mut control_byte = (small_entries.len() & 0x7F) as u8;
    if has_large {
        control_byte |= 0x80;
    }

    let mut out = Vec::with_capacity(
        1 + 4 + small_entries.len() * 2 + large_entries.len() * 8 + data_segment.len(),
    );
    out.push(control_byte);

    if has_large {
        let _ = endian.write_u32(&mut out, large_entries.len() as u32);
    }
    for (id, offset) in small_entries {
        out.push(id);
        out.push(offset);
    }
    for (id, offset) in large_entries {
        let _ = endian.write_u32(&mut out, id);
        let _ = endian.write_u32(&mut out, offset);
    }
    out.extend_from_slice(&data_segment);
    out
}

pub fn read_length_prefixed_string(data: &[u8]) -> Option<String> {
    if data.len() < 5 {
        return None;
    }
    let len = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
    if len == 0 || len > 2048 || len > data.len() - 4 {
        return None;
    }
    let slice = &data[4..4 + len];
    let clean = slice.strip_suffix(&[0]).unwrap_or(slice);

    if !clean
        .iter()
        .all(|&b| (0x20..=0x7E).contains(&b) || b == b'\t' || b == b'\r' || b == b'\n')
    {
        return None;
    }

    std::str::from_utf8(clean)
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn write_length_prefixed_string(s: &str) -> Vec<u8> {
    write_length_prefixed_string_with_endian(s, Endian::Little)
}

pub fn write_length_prefixed_string_with_endian(s: &str, endian: Endian) -> Vec<u8> {
    endian.write_length_prefixed_string(s)
}

pub fn calculate_crc32(data: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

pub fn calculate_triumph_crc32(data: &[u8]) -> u32 {
    !calculate_crc32(data)
}

#[inline]
pub fn parse_f32_safe(chunk: &[u8]) -> Option<f32> {
    if chunk.len() >= 4 {
        let val = f32::from_le_bytes(chunk[0..4].try_into().unwrap_or_default());
        if val.is_finite() && !val.is_subnormal() && (1e-4..=500_000.0).contains(&val.abs()) {
            return Some(val);
        }
    }
    None
}

/// A safe, robust abstraction for sequential binary chunk decoding
/// Replaces repetitive cursor setup, manual slicing, and bounds checking.
pub struct ChunkReader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    pub endian: Endian,
}

impl<'a> ChunkReader<'a> {
    pub fn new(data: &'a [u8], endian: Endian) -> Self {
        Self {
            data,
            pos: 0,
            endian,
        }
    }

    pub fn is_eof(&self) -> bool {
        self.pos >= self.data.len()
    }

    pub fn skip(&mut self, len: usize) {
        self.pos += len;
    }

    pub fn read_u8(&mut self) -> Result<u8> {
        if self.pos >= self.data.len() {
            bail!("EOF reached");
        }
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    pub fn read_u16(&mut self) -> Result<u16> {
        if self.pos + 2 > self.data.len() {
            bail!("EOF reached");
        }
        let v = self
            .endian
            .u16_from_bytes(self.data[self.pos..self.pos + 2].try_into().unwrap());
        self.pos += 2;
        Ok(v)
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        if self.pos + 4 > self.data.len() {
            bail!("EOF reached");
        }
        let v = self
            .endian
            .u32_from_bytes(self.data[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }

    pub fn read_f32(&mut self) -> Result<f32> {
        if self.pos + 4 > self.data.len() {
            bail!("EOF reached");
        }
        let v = self
            .endian
            .f32_from_bytes(self.data[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }

    pub fn read_f32_safe(&mut self) -> Option<f32> {
        if self.pos + 4 > self.data.len() {
            return None;
        }
        let val = self
            .endian
            .f32_from_bytes(self.data[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        if val.is_finite() && !val.is_subnormal() && (1e-4..=500_000.0).contains(&val.abs()) {
            Some(val)
        } else {
            None
        }
    }

    pub fn read_string(&mut self) -> Option<String> {
        let len = self.read_u32().ok()? as usize;
        if len == 0 || len > 2048 || self.pos + len > self.data.len() {
            return None;
        }
        let slice = &self.data[self.pos..self.pos + len];
        self.pos += len;

        let clean = slice.strip_suffix(&[0]).unwrap_or(slice);
        if clean
            .iter()
            .all(|&b| (0x20..=0x7E).contains(&b) || b == b'\t' || b == b'\r' || b == b'\n')
        {
            std::str::from_utf8(clean)
                .ok()
                .map(|s| s.trim().to_string())
        } else {
            None
        }
    }
}

/// Represents a Triumph Engine Map Entity UID / Scene Graph Instance Handle.
/// In Triumph Engine packages, Chunk 22 stores a packed 32-bit handle where:
/// - Bits 0..23: The sequential instance UID within the level map.
/// - Bits 24..31: The Domain / Layer prefix ('M' = 0x4D for Main Map, 0x01..0x3F for sub-layers/groups).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct EntityHandleJson {
    pub uid: u32,
    pub domain_tag: String,
    pub raw_hex: String,
}

pub fn parse_entity_handle(raw_u32: u32) -> Option<EntityHandleJson> {
    let high_byte = ((raw_u32 >> 24) & 0xFF) as u8;
    let uid = raw_u32 & 0x00FF_FFFF;

    // Entity handles have a realistic map UID count (< 65,536) and a non-zero domain/layer prefix.
    // This avoids false positives for raw boolean masks like 0x00C00001 or 0x21400000.
    if uid > 0 && uid <= 0x0000_FFFF && high_byte > 0 {
        let domain_tag = if high_byte.is_ascii_alphanumeric() {
            (high_byte as char).to_string()
        } else {
            format!("L{:02X}", high_byte)
        };

        return Some(EntityHandleJson {
            uid,
            domain_tag,
            raw_hex: format!("0x{:08X}", raw_u32),
        });
    }

    None
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ItemSocketConfigJson {
    pub mount_point: String,
    pub primary_slot: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_slot: Option<u32>,
}

pub fn parse_socket_data(chunk: &[u8]) -> ItemSocketConfigJson {
    if chunk.len() >= 13 && chunk.starts_with(&[1, 40, 0, 2, 40, 0, 43, 4]) {
        ItemSocketConfigJson {
            mount_point: "Right_Hand_Carry".into(),
            primary_slot: 40,
            secondary_slot: Some(43),
        }
    } else {
        ItemSocketConfigJson {
            mount_point: "Standard_Grip".into(),
            primary_slot: 40,
            secondary_slot: None,
        }
    }
}

pub fn build_socket_data(socket: &ItemSocketConfigJson) -> Vec<u8> {
    if socket.secondary_slot.is_some() {
        vec![1, 40, 0, 2, 40, 0, 43, 4, 1, 1, 0, 0, 0]
    } else {
        vec![1, 40, 0, 1, 40, 0, 1, 1, 0, 0]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ObjectTypeId {
    ModelResource = 0x0041004B,
    PlacementObject = 0x00464621,
    SoundMarker = 0x00464661,
    PushableWheel1 = 0x00464665,
    PushableWheel2 = 0x00464669,
    LogicMarker = 0x00462103,
    PointLight = 0x00462107,
    MinionGate = 0x00464181,
    UpgradePortal = 0x00464681,
    DoorController,
    LightMarker,
    Mechanism,
    Unknown(u32),
}

impl From<u32> for ObjectTypeId {
    fn from(val: u32) -> Self {
        match val {
            0x0041004B => Self::ModelResource,
            0x00464621 => Self::PlacementObject,
            0x00464661 => Self::SoundMarker,
            0x00464665 => Self::PushableWheel1,
            0x00464669 => Self::PushableWheel2,
            0x00462103 => Self::LogicMarker,
            0x00462107 => Self::PointLight,
            0x00464181 => Self::MinionGate,
            0x00464681 => Self::UpgradePortal,
            _ if (val >> 8) == 0x004650 => Self::DoorController,
            _ if (val >> 8) == 0x004621 => Self::LightMarker,
            _ if (val >> 8) == 0x004646 => Self::Mechanism,
            _ => Self::Unknown(val),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ObjectChunkId {
    GroupTag = 20,
    EntityName = 21,
    Flags = 22,
    Enabled = 23,
    DisplayName = 25,
    GroupCount26 = 26,
    StanceId = 28,
    CanBeCarried = 29,
    MeshBindings = 30,
    StandModel = 31,
    Scale = 32,
    Bones = 33,
    BoundingBox = 34,
    RagdollBones = 35,
    DefaultAnimation = 36,
    PhysicsState = 37,
    InteractionActions41 = 41,
    PlacedObject = 42,
    TriggerActive = 43,
    Unknown44 = 44,
    SecondaryFlags = 45,
    MaterialId = 46,
    LogicEventLink = 50,
    DoorStateCount = 55,
    Attachments60 = 60,
    PointLightParams = 70,
    DoorDefaultState = 71,
    InteractionActions72 = 72,
    InteractionActions73 = 73,
    InteractionActions74 = 74,
    InteractionActions75 = 75,
    Attachments86 = 86,
    OpenCollision = 100,
    ClosedCollision = 101,
    Attachments128 = 128,
    PlacementOffset = 300,
    Padding301 = 301,
    Terminator = 19,
    AttachmentSlots = 1,
    Unknown(u32),
}

impl From<u32> for ObjectChunkId {
    fn from(val: u32) -> Self {
        match val {
            20 => Self::GroupTag,
            21 => Self::EntityName,
            22 => Self::Flags,
            23 => Self::Enabled,
            25 => Self::DisplayName,
            26 => Self::GroupCount26,
            28 => Self::StanceId,
            29 => Self::CanBeCarried,
            30 => Self::MeshBindings,
            31 => Self::StandModel,
            32 => Self::Scale,
            33 => Self::Bones,
            34 => Self::BoundingBox,
            35 => Self::RagdollBones,
            36 => Self::DefaultAnimation,
            37 => Self::PhysicsState,
            41 => Self::InteractionActions41,
            42 => Self::PlacedObject,
            43 => Self::TriggerActive,
            44 => Self::Unknown44,
            45 => Self::SecondaryFlags,
            46 => Self::MaterialId,
            50 => Self::LogicEventLink,
            55 => Self::DoorStateCount,
            60 => Self::Attachments60,
            70 => Self::PointLightParams,
            71 => Self::DoorDefaultState,
            72 => Self::InteractionActions72,
            73 => Self::InteractionActions73,
            74 => Self::InteractionActions74,
            75 => Self::InteractionActions75,
            86 => Self::Attachments86,
            100 => Self::OpenCollision,
            101 => Self::ClosedCollision,
            128 => Self::Attachments128,
            300 => Self::PlacementOffset,
            301 => Self::Padding301,
            19 => Self::Terminator,
            1 => Self::AttachmentSlots,
            _ => Self::Unknown(val),
        }
    }
}
