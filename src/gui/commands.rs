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
    ZoomMeshViewport {
        delta_zoom: f32,
    },
    SetViewportFov {
        fov_degrees: f32,
    },
    SetViewportLighting {
        mode: u32,
    },
    SetViewportUpAxis {
        mode: u32,
    },
    ResetViewportCamera,
    ToggleCompositeView,
    ToggleSkinning, // Allows debugging rig independently from mesh deformation
    FilterAssets {
        query: String,
        generation: u64,
    },

    // Real-Time Viewport Animation Playback Commands
    SelectAnimation {
        clip_index: i32,
    },
    SetAnimationTime {
        time_seconds: f32,
    },
    TickAnimationPlayback {
        delta_seconds: f32,
    },

    // Standalone Direct File Operations (Global Toolbox)
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

    DirectVpkToJson {
        src: PathBuf,
        dst: PathBuf,
    },
    DirectJsonToVpk {
        src_json: PathBuf,
        baseline_vpk: PathBuf,
        dst: PathBuf,
    },

    DirectDtaToJson {
        src: PathBuf,
        dst: PathBuf,
    },
    DirectJsonToDta {
        src_json: PathBuf,
        baseline_dta: PathBuf,
        dst: PathBuf,
    },
    DirectEnvToJson {
        src: PathBuf,
        dst: PathBuf,
    },
    DirectJsonToEnv {
        src_json: PathBuf,
        baseline_env: PathBuf,
        dst: PathBuf,
    },

    DirectMeshExport {
        src: PathBuf,
        dst: PathBuf,
        is_glb: bool,
    },
    DirectMeshImport {
        chunk_target: PathBuf,
        model_src: PathBuf,
        is_glb: bool,
    },
    DirectAssembleLevel {
        omp_path: PathBuf,
        assets_dir: PathBuf,
        dst: PathBuf,
    },
    DirectTerrainExport {
        src: PathBuf,
        dst: PathBuf,
        is_glb: bool,
    },

    DirectCollisionExport {
        src: PathBuf,
        dst: PathBuf,
    },
    DirectCollisionImport {
        chunk_target: PathBuf,
        glb_src: PathBuf,
    },
    DirectFontToJson {
        src: PathBuf,
        dst: PathBuf,
    },
    DirectJsonToFont {
        src_json: PathBuf,
        dst: PathBuf,
    },

    DirectTextureExport {
        src: PathBuf,
        dst: PathBuf,
    },
    DirectTextureImport {
        chunk_target: PathBuf,
        img_src: PathBuf,
    },
    DirectAudioExport {
        src: PathBuf,
        dst: PathBuf,
    },
    DirectAudioImport {
        chunk_target: PathBuf,
        wav_src: PathBuf,
    },

    // Contextual Asset Export Operations
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

    // Contextual Asset Import & Save Operations
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
    SaveDta {
        chunk_path: PathBuf,
        json_data: String,
    },
    SaveVpk {
        chunk_path: PathBuf,
        json_data: String,
    },
}
