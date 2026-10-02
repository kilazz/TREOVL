use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;

use crate::engine::assets::animation::parse_animation_clip;
use crate::engine::assets::lua::{disassemble_lua_bytecode, inspect_lua_bytecode};
use crate::engine::assets::map::{assemble_level_scene_glb, export_level_to_glb, parse_omp_map};
use crate::engine::assets::shader::{ShaderType, export_shader};
use crate::engine::container::project::{pack_archive, unpack_archive};
use crate::engine::container::sync::{
    clean_rebuild_project, export_smart_assets, revert_single_asset, sync_assets_to_chunks,
};
use crate::engine::service;
use crate::utils::diff::{apply_patch, create_diff};

#[derive(Parser)]
#[command(
    name = "TREOVL",
    version = "1.0.0",
    about = "Triumph Engine Overlord Toolkit (TREOVL) — Modern Reverse Engineering & Modding Toolkit",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Extract PRP/RPK archive into a project folder and populate smart assets
    Unpack {
        /// Source .prp or .rpk archive path
        archive: PathBuf,
        /// Target project output directory
        out_dir: PathBuf,
    },
    /// Sync modified assets and pack the project directory back into an archive
    Pack {
        /// Project directory containing project.json
        project_dir: PathBuf,
        /// Output .prp archive file
        out_archive: PathBuf,
        /// Zlib compression level: 0 = Store (Fastest, No compression), 1 = Fast, 6 = Balanced, 9 = Maximum
        #[arg(short, long, default_value_t = 0)]
        compression: u32,
    },
    /// Sync modified files from 'assets/' into 'chunks/' using vanilla baseline
    Sync {
        /// Project directory containing project.json
        project_dir: PathBuf,
    },
    /// Clean rebuild all chunks from chunks_vanilla and freshly apply edits
    CleanRebuild {
        /// Project directory containing project.json
        project_dir: PathBuf,
    },
    /// Revert a specific chunk and its smart asset back to vanilla baseline
    Revert {
        /// Project directory containing project.json
        project_dir: PathBuf,
        /// Relative path or filename of the chunk (e.g. chunk_0001_id0x0.bin)
        chunk: String,
    },
    /// Re-export all smart editable assets into 'assets/' from raw chunks
    ExportAssets {
        /// Project directory containing chunks/
        project_dir: PathBuf,
    },
    /// Export 3D mesh chunk to glTF 2.0 Binary (.glb)
    ExportGlb {
        /// Input mesh chunk (.bin)
        mesh: PathBuf,
        /// Output .glb file
        output: PathBuf,
    },
    /// Inject glTF 2.0 Binary (.glb) back into a mesh chunk
    ImportGlb {
        /// Target mesh chunk (.bin)
        mesh: PathBuf,
        /// Input .glb file
        input: PathBuf,
    },
    /// Export 3D mesh chunk to Wavefront OBJ
    ExportObj {
        /// Input mesh chunk (.bin)
        mesh: PathBuf,
        /// Output .obj file
        output: PathBuf,
    },
    /// Inject Wavefront OBJ back into a mesh chunk
    ImportObj {
        /// Target mesh chunk (.bin)
        mesh: PathBuf,
        /// Input .obj file
        input: PathBuf,
    },
    /// Export terrain heightmap to glTF 2.0 with vertex colors
    ExportTerrainGlb {
        /// Input terrain chunk (.bin)
        terrain: PathBuf,
        /// Output .glb file
        output: PathBuf,
    },
    /// Export complete level scene with terrain and entity locators (.omp to .glb)
    ExportLevelGlb {
        /// Input level map (.omp)
        level: PathBuf,
        /// Output .glb file
        output: PathBuf,
    },
    /// Assemble level scene into .glb instancing real 3D models from assets/meshes
    AssembleLevel {
        /// Input level map (.omp)
        level: PathBuf,
        /// Extracted assets directory containing meshes/
        assets_dir: PathBuf,
        /// Output .glb file
        output: PathBuf,
    },
    /// Export skeletal animation to glTF 2.0 timeline for Blender
    ExportAnim {
        /// Input animation chunk (.bin)
        anim: PathBuf,
        /// Output .glb file
        output: PathBuf,
    },
    /// Dump animation keyframes and timestamps to readable JSON
    ExportAnimJson {
        /// Input animation chunk (.bin)
        anim: PathBuf,
        /// Output .json file
        output: PathBuf,
    },
    /// Inspect animation clip duration, frame rate, and bone tracks
    InspectAnim {
        /// Input animation chunk (.bin)
        anim: PathBuf,
    },
    /// Inspect Overlord Map Package, entities, and spawn point
    InspectMap {
        /// Input map package (.omp)
        level: PathBuf,
    },
    /// Verify Lua 5.0.2 bytecode and extract string constants
    InspectLua {
        /// Input Lua script chunk (.bin or .luac)
        script: PathBuf,
    },
    /// Disassemble Lua 5.0.2 bytecode into readable pseudocode
    DisasmLua {
        /// Input Lua script chunk (.bin or .luac)
        script: PathBuf,
        /// Optional output file path
        out: Option<PathBuf>,
    },
    /// Export texture chunk to DDS/TGA
    ExportDds {
        /// Input texture chunk (.bin)
        texture: PathBuf,
        /// Output image file (.dds or .tga)
        output: PathBuf,
    },
    /// Inject DDS/TGA into texture chunk
    ImportDds {
        /// Target texture chunk (.bin)
        texture: PathBuf,
        /// Input image file (.dds or .tga)
        input: PathBuf,
    },
    /// Export shader binary to HLSL or DXBC
    ExportShader {
        /// Input shader chunk (.bin)
        shader: PathBuf,
        /// Output directory
        out_dir: PathBuf,
    },
    /// Create a non-destructive mod patch (.json) comparing base and modded folders
    CreatePatch {
        /// Base/Vanilla project directory
        base: PathBuf,
        /// Modified project directory
        mod_dir: PathBuf,
        /// Output patch file (.json)
        patch: PathBuf,
    },
    /// Apply mod patch to target project directory
    ApplyPatch {
        /// Target project directory
        target_dir: PathBuf,
        /// Input patch file (.json)
        patch: PathBuf,
    },
}

pub fn handle_cli() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Unpack { archive, out_dir } => {
            let (count, log) = unpack_archive(&archive, &out_dir)?;
            print!("{}", log);
            println!(
                "[+] Extracted {} chunks (vanilla baseline preserved).",
                count
            );
        }
        Commands::Pack {
            project_dir,
            out_archive,
            compression,
        } => {
            let size = pack_archive(&project_dir, &out_archive, compression)?;
            println!(
                "[+] Archive packed successfully ({} bytes, compression level: {}).",
                size, compression
            );
        }
        Commands::Sync { project_dir } => {
            let count = sync_assets_to_chunks(&project_dir)?;
            println!(
                "[+] Synced {} modified assets into chunks using vanilla baseline.",
                count
            );
        }
        Commands::CleanRebuild { project_dir } => {
            let count = clean_rebuild_project(&project_dir)?;
            println!("[+] Clean rebuild completed: {} assets re-synced.", count);
        }
        Commands::Revert { project_dir, chunk } => {
            revert_single_asset(&project_dir, &chunk)?;
            println!("[+] Reverted {:?} to pristine vanilla baseline.", chunk);
        }
        Commands::ExportAssets { project_dir } => {
            let count = export_smart_assets(&project_dir)?;
            println!("[+] Exported {} smart assets into 'assets/' folder.", count);
        }
        Commands::ExportGlb { mesh, output } => {
            let stats = service::export_mesh(&mesh, &output, true)?;
            println!(
                "[+] glTF 2.0 Binary (.glb) exported ({} vertices, {} triangles, skinned: {}).",
                stats.vertex_count, stats.triangle_count, stats.is_skinned
            );
        }
        Commands::ImportGlb { mesh, input } => {
            service::import_mesh(&mesh, &input, true)?;
            println!("[+] Mesh chunk successfully updated from .glb (skinning preserved).");
        }
        Commands::ExportObj { mesh, output } => {
            let stats = service::export_mesh(&mesh, &output, false)?;
            println!(
                "[+] OBJ exported ({} vertices, {} triangles, stride: {} bytes).",
                stats.vertex_count, stats.triangle_count, stats.stride
            );
        }
        Commands::ImportObj { mesh, input } => {
            service::import_mesh(&mesh, &input, false)?;
            println!("[+] Mesh chunk successfully updated from OBJ.");
        }
        Commands::ExportTerrainGlb { terrain, output } => {
            let (v_count, tri_count) = service::export_terrain(&terrain, &output, true)?;
            println!(
                "[+] Terrain exported to .glb with Vertex Colors ({} vertices, {} triangles).",
                v_count, tri_count
            );
        }
        Commands::ExportLevelGlb { level, output } => {
            let data = fs::read(&level)?;
            let glb = export_level_to_glb(&data)?;
            fs::write(&output, glb)?;
            println!("[+] Complete level scene exported to .glb with terrain and entity locators.");
        }
        Commands::AssembleLevel {
            level,
            assets_dir,
            output,
        } => {
            let data = fs::read(&level).with_context(|| format!("Failed to read {:?}", level))?;
            let glb = assemble_level_scene_glb(&data, &assets_dir)?;
            fs::write(&output, glb)?;
            println!(
                "[+] Full level scene assembled with real meshes into: {:?}",
                output
            );
        }
        Commands::ExportAnim { anim, output } => {
            service::export_anim_glb(&anim, &output)?;
            println!("[+] Animation exported to glTF 2.0 (.glb) with timeline channels.");
        }
        Commands::ExportAnimJson { anim, output } => {
            service::export_anim_json(&anim, &output)?;
            println!("[+] Animation keyframes exported to JSON.");
        }
        Commands::InspectAnim { anim } => {
            let data = fs::read(&anim)?;
            let clip = parse_animation_clip(&data)?;
            println!("--- ANIMATION CLIP INFO ---");
            println!("Clip Name:    {}", clip.name);
            println!("Target Rig:   {}", clip.target_rig);
            println!("Frame Rate:   {:.1} FPS", clip.frame_rate);
            println!("Duration:     {:.3} seconds", clip.duration_seconds);
            println!("Bone Tracks:  {} animated bones", clip.bone_tracks.len());
            for track in clip.bone_tracks.iter().take(10) {
                println!(
                    "  - {:<22} (pos keys: {}, rot keys: {})",
                    track.bone_name,
                    track.translations.len(),
                    track.rotations.len()
                );
            }
        }
        Commands::InspectMap { level } => {
            let data = fs::read(&level)?;
            let info = parse_omp_map(&data)?;
            println!("--- OVERLORD MAP INFO ---");
            println!("Map Title:    {}", info.map_name);
            println!("Entity Count: {} objects placed", info.entity_count);
            if let Some(pos) = info.player_spawn {
                println!(
                    "Player Start: X={:.2}, Y={:.2}, Z={:.2}",
                    pos.x, pos.y, pos.z
                );
            }
        }
        Commands::InspectLua { script } => {
            let data = fs::read(&script)?;
            let info = inspect_lua_bytecode(&data)?;
            println!("--- LUA 5.0.2 BYTECODE INFO ---");
            println!("VM Status:    Valid PUC-Rio 5.0.2 (32-bit LE)");
            println!(
                "Script Name:  {}",
                info.script_name.unwrap_or_else(|| "N/A".into())
            );
            println!("Bytecode Len: {} bytes", info.bytecode_len);
            println!("Strings ({} found):", info.string_constants.len());
            for s in info.string_constants.iter().take(20) {
                println!("  - \"{}\"", s);
            }
        }
        Commands::DisasmLua { script, out } => {
            let data = fs::read(&script)?;
            let disasm = disassemble_lua_bytecode(&data)?;
            if let Some(out_path) = out {
                fs::write(&out_path, &disasm)?;
                println!("[+] Lua disassembly written to: {:?}", out_path);
            } else {
                println!("{}", disasm);
            }
        }
        Commands::ExportDds { texture, output } => {
            service::export_texture(&texture, &output)?;
            println!("[+] Image exported to {:?}", output);
        }
        Commands::ImportDds { texture, input } => {
            service::import_texture(&texture, &input)?;
            println!("[+] Texture chunk successfully updated from DDS.");
        }
        Commands::ExportShader { shader, out_dir } => {
            let data = fs::read(&shader)?;
            let (payload, s_type, name) = export_shader(&data)?;
            let ext = match s_type {
                ShaderType::InternalHLSL => "hlsl",
                _ => "dxbc",
            };
            let out_file = out_dir.join(format!("{}.{}", name, ext));
            fs::write(&out_file, payload)?;
            println!("[+] Shader successfully exported to: {:?}", out_file);
        }
        Commands::CreatePatch {
            base,
            mod_dir,
            patch,
        } => {
            let count = create_diff(&base, &mod_dir, &patch)?;
            println!("[+] Patch created: {} items tracked.", count);
        }
        Commands::ApplyPatch { target_dir, patch } => {
            let count = apply_patch(&target_dir, &patch)?;
            println!("[+] Patch applied: {} files updated.", count);
        }
    }

    Ok(())
}
