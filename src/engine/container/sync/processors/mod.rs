pub mod entities;
pub mod environment;
pub mod logic;
pub mod media;

use anyhow::Result;
use std::path::{Path, PathBuf};

use super::cache::AssetSyncEntry;
use crate::engine::assets::sniffer::SniffedAsset;

pub struct ProjectWorkspace<'a> {
    pub base_dir: &'a Path,
    pub chunks_dir: PathBuf,
    pub vanilla_chunks_dir: PathBuf,
    pub assets_dir: PathBuf,
}

impl<'a> ProjectWorkspace<'a> {
    pub fn new(base_dir: &'a Path) -> Self {
        Self {
            base_dir,
            chunks_dir: base_dir.join("chunks"),
            vanilla_chunks_dir: base_dir.join("chunks_vanilla"),
            assets_dir: base_dir.join("assets"),
        }
    }
}

pub trait AssetProcessor: Sync + Send {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>>;
}

pub use logic::RawProcessor;

pub fn get_standard_processors() -> Vec<Box<dyn AssetProcessor>> {
    vec![
        Box::new(entities::CharacterProcessor),
        Box::new(entities::AttachmentProcessor),
        Box::new(entities::ObjectProcessor),
        Box::new(media::TextureProcessor),
        Box::new(media::AudioProcessor),
        Box::new(media::MaterialProcessor),
        Box::new(media::MeshProcessor),
        Box::new(media::AnimationProcessor),
        Box::new(media::FontProcessor),
        Box::new(environment::TerrainPaletteProcessor),
        Box::new(environment::CollisionProcessor),
        Box::new(environment::DtaProcessor),
        Box::new(environment::VoicePackageProcessor),
        Box::new(logic::VfxProcessor),
        Box::new(logic::EventProcessor),
        Box::new(logic::FaceFxProcessor),
        Box::new(logic::LuaProcessor),
        Box::new(logic::XmlProcessor),
        Box::new(logic::M8ldProcessor),
        Box::new(logic::ParameterProcessor),
        Box::new(logic::UiProcessor),
    ]
}
