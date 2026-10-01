use crate::engine::assets::animation::{
    export_animation_to_glb, export_animation_to_json, parse_animation_clip,
};
use crate::engine::assets::lua::{disassemble_lua_bytecode, inspect_lua_bytecode};
use crate::engine::assets::map::{export_level_to_glb, parse_omp_map};
use crate::engine::assets::mesh::{
    export_mesh_to_glb, export_mesh_to_obj, import_glb_to_mesh, import_obj_to_mesh,
};
use crate::engine::assets::shader::{ShaderType, export_shader};
use crate::engine::assets::terrain::export_terrain_to_glb;
use crate::engine::assets::texture::{export_to_dds, replace_texture_in_chunk};
use crate::engine::container::project::{pack_archive, unpack_archive};
use crate::engine::container::sync::{export_smart_assets, sync_assets_to_chunks};
use crate::utils::diff::{apply_patch, create_diff};
use std::path::Path;

pub fn print_help() {
    println!(
        "\
Overlord Modding Studio CLI
Usage: overlord_tool <command> [args...]

Commands:
  unpack <archive.prp> <out_dir>         Extract PRP/RPK archive into project folder + smart assets
  pack <project_dir> <out_archive.prp>   Sync assets and compile project back into game package
  sync <project_dir>                     Sync modified assets from 'assets/' into 'chunks/'
  export_assets <project_dir>            Re-generate all smart assets in 'assets/' from chunks
  export_glb <mesh.bin> <output.glb>     Export 3D mesh chunk to glTF 2.0 Binary (.glb)
  import_glb <mesh.bin> <input.glb>      Inject glTF 2.0 Binary (.glb) back into mesh chunk
  export_obj <mesh.bin> <output.obj>     Export 3D mesh chunk to legacy Wavefront OBJ
  import_obj <mesh.bin> <input.obj>      Inject legacy Wavefront OBJ back into mesh chunk
  export_terrain_glb <terr.bin> <out.glb> Export terrain heightmap to glTF 2.0 with vertex colors
  export_level_glb <level.omp> <out.glb> Export complete level scene with terrain and entity markers
  export_anim <anim.bin> <output.glb>    Export skeletal animation to glTF 2.0 timeline for Blender
  export_anim_json <anim.bin> <out.json> Dump animation keyframes and timestamps to readable JSON
  inspect_anim <anim.bin>                Inspect animation clip duration, frame rate, and bone tracks
  inspect_map <level.omp>                Inspect Overlord Map Package, entities, and spawn point
  inspect_lua <script.bin>               Verify Lua 5.0.2 bytecode and extract string constants
  disasm_lua <script.bin> [out.txt]      Disassemble Lua 5.0.2 bytecode into readable pseudocode
  export_dds <tex.bin> <output.dds>      Export texture chunk to DDS/TGA
  import_dds <tex.bin> <input.dds>       Inject DDS/TGA into texture chunk
  export_shader <shader.bin> <out_dir>   Export shader binary to HLSL or DXBC
  create_patch <base> <mod> <patch.json> Create non-destructive JSON diff patch
  apply_patch <target_dir> <patch.json>  Apply mod patch to project directory
  help                                   Print this message"
    );
}

pub fn handle_cli(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let cmd = args[1].to_lowercase();
    match cmd.as_str() {
        "help" | "--help" | "-h" => print_help(),
        "unpack" => {
            if args.len() < 4 {
                println!("Usage: unpack <archive.prp> <out_dir>");
                return Ok(());
            }
            let (count, log) = unpack_archive(Path::new(&args[2]), Path::new(&args[3]))?;
            print!("{}", log);
            println!("[+] Extracted {} chunks.", count);
        }
        "pack" => {
            if args.len() < 4 {
                println!("Usage: pack <project_dir> <out.prp>");
                return Ok(());
            }
            let size = pack_archive(Path::new(&args[2]), Path::new(&args[3]))?;
            println!("[+] Archive packed successfully ({} bytes).", size);
        }
        "sync" => {
            if args.len() < 3 {
                println!("Usage: sync <project_dir>");
                return Ok(());
            }
            let count = sync_assets_to_chunks(Path::new(&args[2]))?;
            println!("[+] Synced {} modified assets into chunks.", count);
        }
        "export_assets" => {
            if args.len() < 3 {
                println!("Usage: export_assets <project_dir>");
                return Ok(());
            }
            let count = export_smart_assets(Path::new(&args[2]))?;
            println!("[+] Exported {} smart assets into 'assets/' folder.", count);
        }
        "export_glb" => {
            if args.len() < 4 {
                println!("Usage: export_glb <mesh.bin> <out.glb>");
                return Ok(());
            }
            let data = std::fs::read(&args[2])?;
            let (glb, stats) = export_mesh_to_glb(&data)?;
            std::fs::write(&args[3], glb)?;
            println!(
                "[+] glTF 2.0 Binary (.glb) exported ({} vertices, {} triangles, stride: {} bytes).",
                stats.vertex_count, stats.triangle_count, stats.stride
            );
        }
        "import_glb" => {
            if args.len() < 4 {
                println!("Usage: import_glb <mesh.bin> <in.glb>");
                return Ok(());
            }
            let chunk = std::fs::read(&args[2])?;
            let glb = std::fs::read(&args[3])?;
            let new_bin = import_glb_to_mesh(&chunk, &glb)?;
            std::fs::write(&args[2], new_bin)?;
            println!("[+] Mesh chunk successfully updated from .glb.");
        }
        "export_level_glb" => {
            if args.len() < 4 {
                println!("Usage: export_level_glb <level.omp> <out.glb>");
                return Ok(());
            }
            let data = std::fs::read(&args[2])?;
            let glb = export_level_to_glb(&data)?;
            std::fs::write(&args[3], glb)?;
            println!("[+] Complete level scene exported to .glb with terrain and entity locators.");
        }
        "disasm_lua" => {
            if args.len() < 3 {
                println!("Usage: disasm_lua <script.bin> [out.txt]");
                return Ok(());
            }
            let data = std::fs::read(&args[2])?;
            let disasm = disassemble_lua_bytecode(&data)?;
            if args.len() >= 4 {
                std::fs::write(&args[3], &disasm)?;
                println!("[+] Lua disassembly written to: {}", args[3]);
            } else {
                println!("{}", disasm);
            }
        }
        "inspect_anim" => {
            if args.len() < 3 {
                println!("Usage: inspect_anim <anim.bin>");
                return Ok(());
            }
            let data = std::fs::read(&args[2])?;
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
        "inspect_map" => {
            if args.len() < 3 {
                println!("Usage: inspect_map <level.omp>");
                return Ok(());
            }
            let data = std::fs::read(&args[2])?;
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
        "inspect_lua" => {
            if args.len() < 3 {
                println!("Usage: inspect_lua <script.bin>");
                return Ok(());
            }
            let data = std::fs::read(&args[2])?;
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
        "export_anim" => {
            if args.len() < 4 {
                println!("Usage: export_anim <anim.bin> <output.glb>");
                return Ok(());
            }
            let data = std::fs::read(&args[2])?;
            let glb = export_animation_to_glb(&data)?;
            std::fs::write(&args[3], glb)?;
            println!("[+] Animation exported to glTF 2.0 (.glb) with timeline channels.");
        }
        "export_anim_json" => {
            if args.len() < 4 {
                println!("Usage: export_anim_json <anim.bin> <out.json>");
                return Ok(());
            }
            let data = std::fs::read(&args[2])?;
            let json_str = export_animation_to_json(&data)?;
            std::fs::write(&args[3], json_str)?;
            println!("[+] Animation keyframes exported to JSON.");
        }
        "export_obj" => {
            if args.len() < 4 {
                println!("Usage: export_obj <mesh.bin> <out.obj>");
                return Ok(());
            }
            let data = std::fs::read(&args[2])?;
            let (obj, stats) = export_mesh_to_obj(&data)?;
            std::fs::write(&args[3], obj)?;
            println!(
                "[+] OBJ exported ({} vertices, {} triangles, stride: {} bytes).",
                stats.vertex_count, stats.triangle_count, stats.stride
            );
        }
        "import_obj" => {
            if args.len() < 4 {
                println!("Usage: import_obj <mesh.bin> <in.obj>");
                return Ok(());
            }
            let chunk = std::fs::read(&args[2])?;
            let obj = std::fs::read_to_string(&args[3])?;
            let new_bin = import_obj_to_mesh(&chunk, &obj)?;
            std::fs::write(&args[2], new_bin)?;
            println!("[+] Mesh chunk successfully updated from OBJ.");
        }
        "export_terrain_glb" => {
            if args.len() < 4 {
                println!("Usage: export_terrain_glb <terr.bin> <out.glb>");
                return Ok(());
            }
            let data = std::fs::read(&args[2])?;
            let (glb, v_count, tri_count) = export_terrain_to_glb(&data)?;
            std::fs::write(&args[3], glb)?;
            println!(
                "[+] Terrain exported to .glb with Vertex Colors ({} vertices, {} triangles).",
                v_count, tri_count
            );
        }
        "export_dds" => {
            if args.len() < 4 {
                println!("Usage: export_dds <tex.bin> <out.dds/tga>");
                return Ok(());
            }
            let data = std::fs::read(&args[2])?;
            let dds = export_to_dds(&data)?;
            std::fs::write(&args[3], dds)?;
            println!("[+] Image exported.");
        }
        "import_dds" => {
            if args.len() < 4 {
                println!("Usage: import_dds <tex.bin> <in.dds>");
                return Ok(());
            }
            let chunk = std::fs::read(&args[2])?;
            let dds = std::fs::read(&args[3])?;
            let new_bin = replace_texture_in_chunk(&chunk, &dds)?;
            std::fs::write(&args[2], new_bin)?;
            println!("[+] Texture chunk successfully updated from DDS.");
        }
        "export_shader" => {
            if args.len() < 4 {
                println!("Usage: export_shader <shader.bin> <out_dir>");
                return Ok(());
            }
            let data = std::fs::read(&args[2])?;
            let (payload, s_type, name) = export_shader(&data)?;
            let ext = match s_type {
                ShaderType::InternalHLSL => "hlsl",
                _ => "dxbc",
            };
            let out_file = Path::new(&args[3]).join(format!("{}.{}", name, ext));
            std::fs::write(&out_file, payload)?;
            println!("[+] Shader successfully exported to: {:?}", out_file);
        }
        "create_patch" => {
            if args.len() < 5 {
                println!("Usage: create_patch <base> <mod> <patch.json>");
                return Ok(());
            }
            let count = create_diff(
                Path::new(&args[2]),
                Path::new(&args[3]),
                Path::new(&args[4]),
            )?;
            println!("[+] Patch created: {} items tracked.", count);
        }
        "apply_patch" => {
            if args.len() < 4 {
                println!("Usage: apply_patch <target> <patch.json>");
                return Ok(());
            }
            let count = apply_patch(Path::new(&args[2]), Path::new(&args[3]))?;
            println!("[+] Patch applied: {} files updated.", count);
        }
        unknown => {
            eprintln!("[!] Unknown CLI command: '{}'", unknown);
            print_help();
        }
    }
    Ok(())
}
