use anyhow::Result;
use std::fs;

use super::super::cache::{AssetSyncEntry, build_asset_filename, calculate_crc32};
use super::{AssetProcessor, ProjectWorkspace};
use crate::engine::assets::sniffer::{AssetKind, SniffedAsset};
use crate::engine::common::magic;

pub struct VfxProcessor;
impl AssetProcessor for VfxProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Vfx {
            return Ok(None);
        }

        let vfx_dir = workspace.assets_dir.join("vfx");
        fs::create_dir_all(&vfx_dir)?;

        let json_str = crate::engine::assets::vfx::export_vfx_to_json(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");

        fs::write(vfx_dir.join(&out_name), json_str.as_bytes())?;
        Ok(Some((
            format!("assets/vfx/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::Vfx,
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct EventProcessor;
impl AssetProcessor for EventProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Event
            && !data.starts_with(magic::EVENT)
            && !data.starts_with(b"\x83\x00\x00\x04")
        {
            return Ok(None);
        }

        let evt_dir = workspace.assets_dir.join("events");
        fs::create_dir_all(&evt_dir)?;

        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");
        let abs_path = evt_dir.join(&out_name);

        let json_str = crate::engine::assets::event::export_event_to_json(data)?;
        fs::write(&abs_path, json_str.as_bytes())?;

        Ok(Some((
            format!("assets/events/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::Event,
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct FaceFxProcessor;
impl AssetProcessor for FaceFxProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::FaceFx {
            return Ok(None);
        }

        let facefx_json_dir = workspace.assets_dir.join("facefx");
        fs::create_dir_all(&facefx_json_dir)?;

        let extracted = crate::engine::assets::facefx::export_facefx(data, stem)?;
        if !extracted.fxe_payload.is_empty() {
            let fxe_out_path = facefx_json_dir.join(&extracted.fxe_filename);
            fs::write(&fxe_out_path, &extracted.fxe_payload)?;
        }

        let json_str = serde_json::to_string_pretty(&extracted.metadata)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");
        fs::write(facefx_json_dir.join(&out_name), json_str.as_bytes())?;

        Ok(Some((
            format!("assets/facefx/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::FaceFx,
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct LuaProcessor;
impl AssetProcessor for LuaProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Lua {
            return Ok(None);
        }

        let scripts_dir = workspace.assets_dir.join("scripts");
        fs::create_dir_all(&scripts_dir)?;

        let bytecode = crate::engine::assets::lua::extract_lua_bytecode(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "luac");
        let lua_source_name = build_asset_filename(&sniffed.display_name, stem, "lua");

        fs::write(scripts_dir.join(&out_name), &bytecode)?;

        if let Ok(disasm) = crate::engine::assets::lua::disassemble_lua_bytecode(&bytecode) {
            let _ = fs::write(scripts_dir.join(format!("{}.lua.txt", out_name)), disasm);
        }

        let decompiled = crate::engine::assets::lua::decompile_lua_bytecode(&bytecode)
            .unwrap_or_else(|_| "-- Decompilation failed, edit via bytecode disassembly".into());

        let lua_path = scripts_dir.join(&lua_source_name);
        fs::write(&lua_path, decompiled.as_bytes())?;

        Ok(Some((
            format!("assets/scripts/{}", lua_source_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::Lua,
                vanilla_crc32: calculate_crc32(decompiled.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct XmlProcessor;
impl AssetProcessor for XmlProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Xml {
            return Ok(None);
        }

        let xml_dir = workspace.assets_dir.join("xml");
        fs::create_dir_all(&xml_dir)?;

        let out_name = build_asset_filename(&sniffed.display_name, stem, "xml");
        let abs_path = xml_dir.join(&out_name);

        let xml_payload = crate::engine::assets::xml::extract_xml_payload(data);
        fs::write(&abs_path, xml_payload)?;

        Ok(Some((
            format!("assets/xml/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::Xml,
                vanilla_crc32: calculate_crc32(xml_payload),
                is_modified: false,
            },
        )))
    }
}

pub struct M8ldProcessor;
impl AssetProcessor for M8ldProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::M8ldMap && !data.starts_with(b"M8LD") {
            return Ok(None);
        }

        let xml_dir = workspace.assets_dir.join("xml");
        fs::create_dir_all(&xml_dir)?;

        let (_crc, xml_content) = crate::engine::assets::m8ld::decompile_8ld_to_xml(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "xml");

        let xml_path = xml_dir.join(&out_name);
        fs::write(&xml_path, xml_content.as_bytes())?;

        Ok(Some((
            format!("assets/xml/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::M8ldMap,
                vanilla_crc32: calculate_crc32(xml_content.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct ParameterProcessor;
impl AssetProcessor for ParameterProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::Parameter && data.len() > 64 {
            return Ok(None);
        }

        let param_dir = workspace.assets_dir.join("parameters");
        fs::create_dir_all(&param_dir)?;

        let out_name = format!("{}.json", stem);
        let abs_path = param_dir.join(&out_name);

        let json_str = crate::engine::assets::parameter::export_parameter_to_json(data, stem)?;
        fs::write(&abs_path, json_str.as_bytes())?;

        Ok(Some((
            format!("assets/parameters/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::Parameter,
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct UiProcessor;
impl AssetProcessor for UiProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        if sniffed.kind != AssetKind::UI {
            return Ok(None);
        }

        let ui_dir = workspace.assets_dir.join("ui");
        fs::create_dir_all(&ui_dir)?;

        let json_str = crate::engine::assets::ui::export_ui_to_json(data)?;
        let out_name = build_asset_filename(&sniffed.display_name, stem, "json");

        fs::write(ui_dir.join(&out_name), json_str.as_bytes())?;
        Ok(Some((
            format!("assets/ui/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::UI,
                vanilla_crc32: calculate_crc32(json_str.as_bytes()),
                is_modified: false,
            },
        )))
    }
}

pub struct RawProcessor;
impl AssetProcessor for RawProcessor {
    fn process(
        &self,
        data: &[u8],
        stem: &str,
        _sniffed: &SniffedAsset,
        workspace: &ProjectWorkspace,
    ) -> Result<Option<(String, AssetSyncEntry)>> {
        let raw_dir = workspace.assets_dir.join("raw_chunks");
        fs::create_dir_all(&raw_dir)?;

        let out_name = format!("{}.bin", stem);
        let bin_path = raw_dir.join(&out_name);
        fs::write(&bin_path, data)?;

        Ok(Some((
            format!("assets/raw_chunks/{}", out_name),
            AssetSyncEntry {
                chunk_rel_path: format!("chunks/{}.bin", stem),
                asset_kind: AssetKind::Generic,
                vanilla_crc32: calculate_crc32(data),
                is_modified: false,
            },
        )))
    }
}
