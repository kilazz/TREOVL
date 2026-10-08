use anyhow::Result;
use std::path::Path;

use crate::engine::assets::sniffer::{AssetKind, SniffedAsset};
use crate::engine::common::Endian;

/// A unified interface for all Triumph Engine asset formats (Open/Closed Principle).
/// Implement this trait for new formats to automatically integrate them into the
/// sniffer, exporter, and binary compiler without touching core engine logic.
pub trait AssetHandler: Send + Sync {
    /// Returns the asset type ID.
    fn kind(&self) -> AssetKind;

    /// The human-readable name for the UI.
    fn display_name(&self) -> &'static str;

    /// Emoji icon for the UI list.
    fn icon(&self) -> &'static str;

    /// Probe heuristic: does this binary chunk belong to this format?
    fn probe(&self, data: &[u8], filename_hint: &str) -> Option<SniffedAsset>;

    /// Decodes binary chunk data into JSON / GLB / WAV / DDS.
    fn export(&self, chunk_data: &[u8], stem: &str) -> Result<Vec<u8>>;

    /// Compiles edited JSON / GLB / WAV back into a proper binary chunk.
    fn import(
        &self,
        baseline_chunk: &[u8],
        decoded_payload: &[u8],
        endian: Endian,
        project_dir: Option<&Path>,
    ) -> Result<Vec<u8>>;
}

/// Global registry of asset handlers.
/// As you migrate formats to this new trait system, register them here.
pub fn get_asset_registry() -> Vec<Box<dyn AssetHandler>> {
    vec![
        // Future additions:
        // Box::new(crate::engine::assets::character::CharacterHandler),
        // Box::new(crate::engine::assets::world::MapHandler),
        // Box::new(crate::engine::assets::media::BinkVideoHandler),
    ]
}
