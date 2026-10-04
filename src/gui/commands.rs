use crate::engine::assets::sniffer::AssetKind;
use std::path::PathBuf;

pub enum WorkerCommand {
    // Project & Archive Operations
    UnpackArchive {
        src: PathBuf,
        dst: PathBuf,
    },
    PackArchive {
        proj_dir: PathBuf,
    },
    LoadProject {
        proj_dir: PathBuf,
    },
    CleanRebuild {
        proj_dir: PathBuf,
    },
    RevertAsset {
        proj_dir: PathBuf,
        chunk_path: String,
    },
    CreatePatch {
        base_dir: PathBuf,
        mod_dir: PathBuf,
        out_file: PathBuf,
    },
    ApplyPatch {
        target_dir: PathBuf,
        patch_file: PathBuf,
    },

    // Interactive UI & Asynchronous Asset Inspection
    SelectAsset {
        filtered_index: i32,
        path: PathBuf,
        kind: AssetKind,
    },
    RotateMeshViewport {
        delta_yaw: f32,
        delta_pitch: f32,
    },
    FilterAssets {
        query: String,
        generation: u64,
    },

    // Direct 8LD & XML File Operations
    Decompile8ldDirect {
        src: PathBuf,
        dst: PathBuf,
    },
    Compile8ldDirect {
        src: PathBuf,
        dst: PathBuf,
    },
    Decompile8ldBatch {
        src_dir: PathBuf,
        dst_dir: PathBuf,
    },
    Compile8ldBatch {
        src_dir: PathBuf,
        dst_dir: PathBuf,
    },

    // Asset Export Operations
    ExportDds {
        chunk_path: PathBuf,
        out_path: PathBuf,
    },
    ExportWav {
        chunk_path: PathBuf,
        out_path: PathBuf,
    },
    ExportMesh {
        chunk_path: PathBuf,
        out_path: PathBuf,
        is_glb: bool,
    },
    ExportTerrain {
        chunk_path: PathBuf,
        out_path: PathBuf,
        is_glb: bool,
    },
    ExportCollisionGlb {
        chunk_path: PathBuf,
        out_path: PathBuf,
    },
    ExportLua {
        chunk_path: PathBuf,
        out_path: PathBuf,
    },
    ExportAnimGlb {
        chunk_path: PathBuf,
        out_path: PathBuf,
    },
    ExportAnimJson {
        chunk_path: PathBuf,
        out_path: PathBuf,
    },

    // Asset Import & Save Operations
    ImportDds {
        chunk_path: PathBuf,
        in_path: PathBuf,
    },
    ImportWav {
        chunk_path: PathBuf,
        in_path: PathBuf,
    },
    ImportMesh {
        chunk_path: PathBuf,
        in_path: PathBuf,
        is_glb: bool,
    },
    ImportCollisionGlb {
        chunk_path: PathBuf,
        in_path: PathBuf,
    },
    ImportLua {
        chunk_path: PathBuf,
        in_path: PathBuf,
    },
    SaveMaterial {
        chunk_path: PathBuf,
        json_data: String,
    },
    SaveUI {
        chunk_path: PathBuf,
        json_data: String,
    },
    SaveObject {
        chunk_path: PathBuf,
        json_data: String,
    },
    SaveTerrainPalette {
        chunk_path: PathBuf,
        json_data: String,
    },
    SaveEnvironment {
        chunk_path: PathBuf,
        json_data: String,
    },
    SaveM8ld {
        chunk_path: PathBuf,
        json_data: String,
    },
    SaveUiSprite {
        chunk_path: PathBuf,
        json_data: String,
    },
}
