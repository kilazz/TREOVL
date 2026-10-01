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
}
