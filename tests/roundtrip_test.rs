use TREOVL::engine::analysis::graph::build_dependency_graph;
use TREOVL::engine::assets::attachment::{
    ItemAttachmentJson, ItemFlagsJson, ItemPhysicsConfigJson, ItemSocketConfigJson,
    export_attachment_to_json, import_attachment_from_json_with_endian,
};
use TREOVL::engine::assets::character::{
    CharacterActorJson, CharacterAttributesJson, export_character, export_character_to_json,
    import_character_from_json, import_character_from_json_with_externals,
};
use TREOVL::engine::assets::cptx::{
    CptxMapJson, UvRectJson, export_cptx_to_json, import_cptx_from_json_with_endian,
};
use TREOVL::engine::assets::dta::{
    DtaPackageJson, compress_dta_payload, export_dta_to_json, import_dta_from_json,
};
use TREOVL::engine::assets::font::{
    FontEngineMetadataJson, FontJson, GlyphMetricJson, export_font_to_json,
    import_font_from_json_with_endian,
};
use TREOVL::engine::assets::material::{
    MaterialBlock, MaterialEngineMetadataJson, MaterialJson, export_material_to_json,
    import_material_from_json_with_endian,
};
use TREOVL::engine::assets::object::{
    BoundingBoxJson, ObjectEngineMetadataJson, ObjectEntityJson, export_object,
    import_object_from_json_with_endian,
};
use TREOVL::engine::assets::parameter::{export_parameter_to_json, import_parameter_from_json};
use TREOVL::engine::assets::projectile::{export_projectile_to_json, import_projectile_from_json};
use TREOVL::engine::assets::vfx::{
    VfxEmitterJson, VfxEngineMetadataJson, VfxGraphJson, VfxPropertyBlockJson, export_vfx_to_json,
    import_vfx_from_json_with_endian,
};
use TREOVL::engine::assets::{build_chunk_from_elements, parse_chunk_elements};
use TREOVL::engine::common::{Endian, EntityHandleJson, parse_entity_handle};
use TREOVL::engine::container::footer::{
    MAGIC_FOOTER_1, MAGIC_FOOTER_2, calculate_triumph_crc32, check_footer,
};
use TREOVL::utils::gltf_builder::GltfBuilder;

#[cfg(feature = "gui")]
use TREOVL::utils::renderer::WgpuRenderer;

#[test]
fn test_triumph_crc32_and_footer() {
    let dummy_payload = b"OVERLORD_MODDING_STUDIO_TEST_PAYLOAD";
    let calc_crc = calculate_triumph_crc32(dummy_payload);

    let mut full_package = dummy_payload.to_vec();
    full_package.extend_from_slice(&MAGIC_FOOTER_1.to_le_bytes());
    full_package.extend_from_slice(&MAGIC_FOOTER_2.to_le_bytes());
    full_package.extend_from_slice(&calc_crc.to_le_bytes());
    full_package.extend_from_slice(&0x7C809B8Bu32.to_le_bytes());

    let footer = check_footer(&full_package).expect("Footer must be detected");
    assert_eq!(footer.original_crc, calc_crc);
    assert_eq!(footer.hash2, 0x7C809B8B);
}

#[test]
fn test_container_elements_roundtrip() {
    let original_elements = vec![
        (10u32, b"INDEX_DATA_BLOB".to_vec()),
        (20u32, b"TAG_STRING_BLOB".to_vec()),
        (33u32, b"BONES_CONTAINER_BLOB".to_vec()),
    ];

    let binary = build_chunk_from_elements(true, &original_elements);
    let (has_magic, parsed) =
        parse_chunk_elements(&binary).expect("Container parsing must succeed");

    assert!(has_magic);
    assert_eq!(parsed.len(), 3);
    assert_eq!(parsed[0].0, 10);
    assert_eq!(parsed[0].1, b"INDEX_DATA_BLOB");
    assert_eq!(parsed[1].0, 20);
    assert_eq!(parsed[2].0, 33);
}

#[test]
fn test_gltf_builder_validity() {
    let mut builder = GltfBuilder::new();
    let data = vec![0.0f32, 1.0f32, 2.0f32];
    let bytes: Vec<u8> = data.iter().flat_map(|f| f.to_le_bytes()).collect();

    let view = builder.add_buffer_view(&bytes, Some(34962));
    let _acc = builder.add_accessor(view, 1, 5126, "VEC3", None, None);
    builder.add_node(serde_json::json!({ "name": "TestNode" }));
    builder.add_scene(vec![0]);

    let glb = builder
        .build("TestGenerator")
        .expect("GLB export must succeed");

    assert!(glb.starts_with(b"glTF"));
    assert_eq!(u32::from_le_bytes(glb[4..8].try_into().unwrap()), 2);
}

#[test]
fn test_mesh_winding_roundtrip() {
    let original_indices = vec![0u32, 1, 2, 3, 4, 5];

    let mut exported = Vec::new();
    for tri in original_indices.chunks_exact(3) {
        exported.push(tri[0]);
        exported.push(tri[2]);
        exported.push(tri[1]);
    }

    assert_eq!(exported, vec![0, 2, 1, 3, 5, 4]);

    let mut restored = exported;
    for tri in restored.chunks_exact_mut(3) {
        tri.swap(1, 2);
    }

    assert_eq!(restored, original_indices);
}

#[test]
#[cfg(feature = "gui")]
fn test_wgpu_renderer_creation_safe() {
    let res = WgpuRenderer::new();
    match res {
        Ok(_) => println!("[+] GPU renderer initialized successfully."),
        Err(e) => println!("[*] Headless or driverless environment detected: {}", e),
    }
}

#[test]
fn test_entity_handle_parsing_and_reconstruction() {
    // 1. Minion Brown handle: 0x4D000008 (Domain 'M', UID 8)
    let h1 = parse_entity_handle(0x4D000008).expect("Should parse 'M' tag handle");
    assert_eq!(h1.domain_tag, "M");
    assert_eq!(h1.uid, 8);
    assert_eq!(h1.raw_hex, "0x4D000008");

    // 2. The Overlord player handle: 0x4D00004B (Domain 'M', UID 75)
    let h2 = parse_entity_handle(0x4D00004B).expect("Should parse 'M' tag handle");
    assert_eq!(h2.domain_tag, "M");
    assert_eq!(h2.uid, 75);

    // 3. WorshipPeasantA handle: 0x03000049 (Domain 'I', UID 3)
    let h3 = parse_entity_handle(0x03000049).expect("Should parse inverted 'I' handle");
    assert_eq!(h3.domain_tag, "I");
    assert_eq!(h3.uid, 3);
}

#[test]
fn test_character_pure_in_memory_roundtrip_le_and_be() {
    let character = CharacterActorJson {
        character_name: "Minion_Brown_PureTest".to_string(),
        resource_tag: Some("Minion_Test_Tag".to_string()),
        entity_handle: Some(EntityHandleJson {
            uid: 8,
            domain_tag: "M".to_string(),
            raw_hex: "0x4D000008".to_string(),
        }),
        is_baby: Some(false),
        combat_attributes: Some(CharacterAttributesJson {
            base_health: Some(42.5),
            move_speed_scale: Some(1.35),
            aggro_range: Some(18.0),
            ..Default::default()
        }),
        embedded_lua_script: Some("GetAlias()\nPrint(\"Test\")".to_string()),
        ai_behaviors: vec!["AttackState".to_string(), "FleeState".to_string()],
        ..Default::default()
    };

    let json_str = serde_json::to_string_pretty(&character).unwrap();

    // 1. Test Little Endian (PC)
    let bin_le = import_character_from_json_with_externals(&json_str, None, None, Endian::Little)
        .expect("LE Import must succeed");

    let extracted_le = export_character(&bin_le, "test_le").expect("LE Export must succeed");
    assert_eq!(
        extracted_le.character.character_name,
        "Minion_Brown_PureTest"
    );
    assert_eq!(
        extracted_le.character._engine_metadata.engine_class,
        "TREActor"
    );
    assert_eq!(
        extracted_le.character.entity_handle.as_ref().map(|h| h.uid),
        Some(8)
    );
    assert_eq!(
        extracted_le
            .character
            .entity_handle
            .as_ref()
            .map(|h| h.domain_tag.as_str()),
        Some("M")
    );

    // 2. Test Big Endian (Xbox 360 / PS3)
    let bin_be = import_character_from_json_with_externals(&json_str, None, None, Endian::Big)
        .expect("BE Import must succeed");
    assert!(!bin_be.is_empty());

    // 3. Test default function
    let bin_default = import_character_from_json(&json_str).expect("Default import must succeed");
    let extracted_default =
        export_character_to_json(&bin_default, "test").expect("Export to JSON must succeed");
    assert!(extracted_default.contains("Minion_Brown_PureTest"));
}

#[test]
fn test_object_pure_in_memory_roundtrip() {
    let object = ObjectEntityJson {
        _engine_metadata: ObjectEngineMetadataJson {
            type_id_hex: "00464621".to_string(),
            class_name: Some("TREPlacementObject".to_string()),
            raw_fallbacks: Vec::new(),
        },
        group_tag: Some("Props_Group".to_string()),
        entity_name: Some("Big Bag of Gold".to_string()),
        entity_handle: Some(EntityHandleJson {
            uid: 83,
            domain_tag: "M".to_string(),
            raw_hex: "0x4D000053".to_string(),
        }),
        scale: Some([1.5, 1.5, 2.0]),
        bounding_box: Some(BoundingBoxJson {
            center: [0.0, 1.0, 0.0],
            half_extents: [0.5, 1.0, 0.5],
            orientation_matrix: None,
        }),
        has_sentinel_terminator: true,
        ..Default::default()
    };

    let json_str = serde_json::to_string_pretty(&object).unwrap();

    let bin_le = import_object_from_json_with_endian(&json_str, Endian::Little)
        .expect("Object import LE must succeed");
    let extracted_le = export_object(&bin_le).expect("Object export LE must succeed");

    assert_eq!(
        extracted_le.entity.entity_name.as_deref(),
        Some("Big Bag of Gold")
    );
    assert_eq!(
        extracted_le.entity.entity_handle.as_ref().map(|h| h.uid),
        Some(83)
    );
    assert!(extracted_le.entity.has_sentinel_terminator);
}

#[test]
fn test_material_pure_in_memory_roundtrip() {
    let mat = MaterialJson {
        _engine_metadata: MaterialEngineMetadataJson {
            type_id_hex: "0041060A".to_string(),
            engine_generation: "Overlord 1".to_string(),
        },
        material_name: "Standard Material".to_string(),
        blocks: vec![
            MaterialBlock {
                id: 30,
                role: Some("Diffuse Texture".to_string()),
                btype: "texture_link".to_string(),
                value: None,
                float_value: None,
                uint_value: None,
                ptr: Some("[TEXTURES]\\Minion_D.dds".to_string()),
                name: Some("Minion_D".to_string()),
            },
            MaterialBlock {
                id: 40,
                role: Some("Alpha Cutoff Threshold".to_string()),
                btype: "float".to_string(),
                value: None,
                float_value: Some(0.33),
                uint_value: None,
                ptr: None,
                name: None,
            },
        ],
    };

    let json_str = serde_json::to_string(&mat).unwrap();

    let bin = import_material_from_json_with_endian(&json_str, Endian::Little)
        .expect("Material import must succeed");
    let re_json = export_material_to_json(&bin).expect("Material export must succeed");

    let parsed_back: MaterialJson = serde_json::from_str(&re_json).unwrap();
    assert_eq!(parsed_back._engine_metadata.type_id_hex, "0041060A");
    assert_eq!(parsed_back.blocks.len(), 2);
}

#[test]
fn test_attachment_pure_in_memory_roundtrip() {
    let attachment = ItemAttachmentJson {
        _engine_metadata: Default::default(),
        item_name: "Steel_Sword".to_string(),
        internal_model_slot: "[Weapons]\\Sword_01".to_string(),
        mesh_package: "Sword_Package".to_string(),
        submesh_name: "Blade".to_string(),
        sound_bank: "Sword_SFX".to_string(),
        drop_sound: None,
        impact_sound: None,
        hold_offset: [0.1, -0.05, 0.4],
        flags: ItemFlagsJson {
            is_pickable: true,
            cast_shadows: true,
            drop_physics: false,
            raw_mask_hex: "0x21400000".to_string(),
        },
        socket: ItemSocketConfigJson {
            mount_point: "Right_Hand_Carry".to_string(),
            primary_slot: 40,
            secondary_slot: Some(43),
        },
        physics: ItemPhysicsConfigJson {
            category_id: 1200,
            world_collision: true,
            is_buoyant: false,
            damage_on_throw: true,
        },
        equipment_config: None,
        weapon_config: None,
        breakable_config: None,
    };

    let json_str = serde_json::to_string(&attachment).unwrap();

    let bin = import_attachment_from_json_with_endian(&json_str, Endian::Little)
        .expect("Attachment import must succeed");
    let re_json = export_attachment_to_json(&bin).expect("Attachment export must succeed");

    let parsed_back: ItemAttachmentJson = serde_json::from_str(&re_json).unwrap();
    assert_eq!(parsed_back.item_name, "Steel_Sword");
    assert_eq!(parsed_back.physics.category_id, 1200);
    assert_eq!(parsed_back.hold_offset, [0.1, -0.05, 0.4]);
}

#[test]
fn test_vfx_pure_in_memory_roundtrip() {
    let vfx = VfxGraphJson {
        _engine_metadata: VfxEngineMetadataJson {
            type_id: "0x00730002".to_string(),
            unmapped_components: Vec::new(),
        },
        group_path: Some("FX_Spells".to_string()),
        vfx_name: Some("Fireball_Burst".to_string()),
        trigger_flags: Some("0x16004007".to_string()),
        enabled: Some(true),
        emitters: vec![VfxEmitterJson {
            id: 1,
            type_id: "0x00730004".to_string(),
            name: "Fire_Sparks".to_string(),
            emitter_type: Some("BillboardSprite".to_string()),
            tag: Some("Sparks".to_string()),
            delay_seconds: Some(0.1),
            lifetime: Some(1.5),
            spawn_rate: Some(45.0),
            speed: Some(12.0),
            size: Some(0.8),
            color_rgba: Some("#FF5500FF".to_string()),
            properties: vec![
                VfxPropertyBlockJson {
                    id: 30,
                    role: Some("Emission Rate".to_string()),
                    ptype: "float".to_string(),
                    string_val: None,
                    float_val: Some(45.0),
                    int_val: None,
                    uint_val: None,
                    color: None,
                    target_emitter_id: None,
                    vector3_val: None,
                    vector4_val: None,
                    hex_val: None,
                },
                VfxPropertyBlockJson {
                    id: 21,
                    role: Some("Spawn Position Offset".to_string()),
                    ptype: "vector3".to_string(),
                    string_val: None,
                    float_val: None,
                    int_val: None,
                    uint_val: None,
                    color: None,
                    target_emitter_id: None,
                    vector3_val: Some([0.0, 1.0, 0.0]),
                    vector4_val: None,
                    hex_val: None,
                },
            ],
            sub_emitters: Vec::new(),
        }],
    };

    let json_str = serde_json::to_string_pretty(&vfx).unwrap();

    // 1. Test Little Endian (PC)
    let bin_le = import_vfx_from_json_with_endian(&json_str, Endian::Little)
        .expect("VFX LE import must succeed");
    let re_json_le = export_vfx_to_json(&bin_le).expect("VFX LE export must succeed");
    let parsed_le: VfxGraphJson = serde_json::from_str(&re_json_le).unwrap();

    assert_eq!(parsed_le.vfx_name.as_deref(), Some("Fireball_Burst"));
    assert_eq!(parsed_le.group_path.as_deref(), Some("FX_Spells"));
    assert_eq!(parsed_le.emitters.len(), 1);
    assert_eq!(parsed_le.emitters[0].name, "Fire_Sparks");

    // 2. Test Big Endian (Xbox 360 / PS3)
    let bin_be = import_vfx_from_json_with_endian(&json_str, Endian::Big)
        .expect("VFX BE import must succeed");
    assert!(!bin_be.is_empty());
}

#[test]
fn test_font_pure_in_memory_roundtrip() {
    let font = FontJson {
        _engine_metadata: FontEngineMetadataJson {
            type_id_hex: "00410072".to_string(),
            texture_link: Some("[FONTS]\\Hud_Font.dds".to_string()),
            raw_blocks: Vec::new(),
        },
        font_name: "Hud_Font_Bold".to_string(),
        font_size: 18.0,
        line_height: 22.0,
        glyphs: vec![
            GlyphMetricJson {
                char_code: 65,
                character: "A".to_string(),
                width: 14.0,
                height: 18.0,
                uv_min: [0.1, 0.2],
                uv_max: [0.2, 0.3],
            },
            GlyphMetricJson {
                char_code: 66,
                character: "B".to_string(),
                width: 13.5,
                height: 18.0,
                uv_min: [0.2, 0.2],
                uv_max: [0.3, 0.3],
            },
        ],
    };

    let json_str = serde_json::to_string_pretty(&font).unwrap();

    // 1. Test Little Endian (PC)
    let bin_le = import_font_from_json_with_endian(&json_str, &[], Endian::Little)
        .expect("Font LE import must succeed");
    let re_json_le = export_font_to_json(&bin_le).expect("Font LE export must succeed");
    let parsed_le: FontJson = serde_json::from_str(&re_json_le).unwrap();

    assert_eq!(parsed_le.font_name, "Hud_Font_Bold");
    assert_eq!(parsed_le.font_size, 18.0);
    assert_eq!(parsed_le.line_height, 22.0);
    assert_eq!(parsed_le.glyphs.len(), 2);
    assert_eq!(parsed_le.glyphs[0].char_code, 65);
    assert_eq!(parsed_le.glyphs[0].width, 14.0);

    // 2. Test Big Endian (Xbox 360 / PS3)
    let bin_be = import_font_from_json_with_endian(&json_str, &[], Endian::Big)
        .expect("Font BE import must succeed");
    assert!(!bin_be.is_empty());
}

#[test]
fn test_cptx_pure_in_memory_roundtrip() {
    let mut baseline = Vec::new();
    baseline.extend_from_slice(b"CPTX");
    baseline.extend_from_slice(&[0u8; 32]); // Space for 2 UV rects

    let cptx = CptxMapJson {
        magic: "CPTX".to_string(),
        uv_rects: vec![
            UvRectJson {
                id: 0,
                u_min: 0.0,
                v_min: 0.0,
                u_max: 0.5,
                v_max: 0.5,
            },
            UvRectJson {
                id: 1,
                u_min: 0.5,
                v_min: 0.5,
                u_max: 1.0,
                v_max: 1.0,
            },
        ],
        unmapped_sequences: Vec::new(),
    };

    let json_str = serde_json::to_string_pretty(&cptx).unwrap();

    // 1. Test Little Endian (PC)
    let bin_le = import_cptx_from_json_with_endian(&json_str, &baseline, Endian::Little)
        .expect("CPTX LE import must succeed");
    let re_json_le = export_cptx_to_json(&bin_le).expect("CPTX LE export must succeed");
    let parsed_le: CptxMapJson = serde_json::from_str(&re_json_le).unwrap();

    assert_eq!(parsed_le.uv_rects.len(), 2);
    assert_eq!(parsed_le.uv_rects[0].u_min, 0.0);
    assert_eq!(parsed_le.uv_rects[0].u_max, 0.5);
    assert_eq!(parsed_le.uv_rects[1].u_min, 0.5);
    assert_eq!(parsed_le.uv_rects[1].u_max, 1.0);

    // 2. Test Big Endian (Xbox 360 / PS3)
    let bin_be = import_cptx_from_json_with_endian(&json_str, &baseline, Endian::Big)
        .expect("CPTX BE import must succeed");
    assert_eq!(&bin_be[0..4], b"CPTX");
}

#[test]
fn test_dta_pure_in_memory_roundtrip() {
    let mut raw_light_bytes = Vec::new();
    for val in [
        12.5f32, -4.0, 30.0, // position XYZ
        8.0,  // radius
        1.0, 0.8, 0.4, // color RGB
        2.5, // intensity
    ] {
        raw_light_bytes.extend_from_slice(&val.to_le_bytes());
    }

    let dta_binary =
        compress_dta_payload(&raw_light_bytes, None).expect("DTA payload compression must succeed");

    let json_str =
        export_dta_to_json(&dta_binary, "test_light_dta").expect("DTA export to JSON must succeed");
    let parsed: DtaPackageJson = serde_json::from_str(&json_str).unwrap();

    assert_eq!(parsed.lights.len(), 1);
    let light = &parsed.lights[0];
    assert_eq!(light.position, [12.5, -4.0, 30.0]);
    assert_eq!(light.radius, 8.0);
    assert_eq!(light.color_rgba, [1.0, 0.8, 0.4, 1.0]);
    assert_eq!(light.intensity, 2.5);

    // Re-import with modified light properties
    let mut modified = parsed;
    modified.lights[0].intensity = 4.0;
    let mod_json_str = serde_json::to_string_pretty(&modified).unwrap();

    let re_bin = import_dta_from_json(&mod_json_str, &dta_binary)
        .expect("DTA import from JSON must succeed");
    let re_exported_json = export_dta_to_json(&re_bin, "test_light_dta").unwrap();
    let re_parsed: DtaPackageJson = serde_json::from_str(&re_exported_json).unwrap();

    assert_eq!(re_parsed.lights.len(), 1);
    assert_eq!(re_parsed.lights[0].intensity, 4.0);
}

#[test]
fn test_projectile_pure_in_memory_roundtrip() {
    let proj_json_raw = serde_json::json!({
        "_engine_metadata": {
            "type_id_hex": "00463006",
            "engine_class": "TREProjectile",
            "unmapped_raw_blocks": []
        },
        "projectile_name": "Fireball_Test",
        "entity_handle": {
            "uid": 28,
            "domain_tag": "M",
            "raw_hex": "0x4D00001C"
        },
        "is_enabled": true,
        "physics": {
            "flight_speed": 40.0,
            "damage_scale": 2.5
        }
    });

    let json_str = serde_json::to_string_pretty(&proj_json_raw).unwrap();
    let bin = import_projectile_from_json(&json_str, Endian::Little)
        .expect("Projectile import must succeed");
    let re_exported_json =
        export_projectile_to_json(&bin, "test_proj").expect("Projectile export must succeed");

    assert!(re_exported_json.contains("Fireball_Test"));
    assert!(re_exported_json.contains("0x4D00001C"));
}

#[test]
fn test_logic_marker_environment_cross_linking() {
    let temp_dir = tempfile::tempdir().unwrap();
    let assets_dir = temp_dir.path().join("assets");
    let obj_dir = assets_dir.join("objects");
    let env_dir = assets_dir.join("environments");

    std::fs::create_dir_all(&obj_dir).unwrap();
    std::fs::create_dir_all(&env_dir).unwrap();

    // 1. Write an environment with flags_hex "0300004D"
    let env_json = serde_json::json!({
        "_engine_metadata": {
            "is_typed_container": true,
            "type_id_hex": "04000083"
        },
        "profile_name": "Endscene7DOOM",
        "flags_hex": "0300004D",
        "fog_near": 20.0,
        "fog_far": 40.0,
        "fog_color": "#377479",
        "ambient_color": "#0D1015",
        "sun_direction": [0.0, 1.0, 0.0],
        "sun_color": "#FFFFFF",
        "fill_direction": [0.0, -1.0, 0.0],
        "fill_color": "#000000",
        "fill_range": 10.0,
        "gamma": 2.2,
        "exposure": 0.3,
        "bloom_threshold": 0.05,
        "bloom_intensity": 1.5,
        "water_tint": "#FFFFFF",
        "far_clip": 64.0,
        "flow_vector": [0.0, 0.0]
    });
    std::fs::write(
        env_dir.join("Endscene7DOOM_chunk_0023_id0x3.json"),
        env_json.to_string(),
    )
    .unwrap();

    // 2. Write a logic marker object with raw_hex_id "0300004D"
    let marker_json = serde_json::json!({
        "_engine_metadata": {
            "type_id_hex": "00462103",
            "class_name": "TRELogicMarker",
            "raw_fallbacks": []
        },
        "entity_name": "DOOM",
        "logic_event_link": {
            "target_event": "Endscene7DOOM",
            "raw_hex_id": "0300004D"
        }
    });
    std::fs::write(
        obj_dir.join("DOOM_chunk_1900_id0x0.json"),
        marker_json.to_string(),
    )
    .unwrap();

    // 3. Build graph and assert bidirectional cross-links
    let graph = build_dependency_graph(&assets_dir).expect("Graph building must succeed");

    let links_from_doom = graph.find_links_for_asset("DOOM_chunk_1900_id0x0");
    assert!(
        links_from_doom
            .iter()
            .any(|l| l.target_name.contains("Endscene7DOOM"))
    );

    let links_from_env = graph.find_links_for_asset("Endscene7DOOM_chunk_0023_id0x3");
    assert!(
        links_from_env
            .iter()
            .any(|l| l.target_name.contains("DOOM"))
    );
}

#[test]
fn test_chunk_1881_jester_slot_descriptor_roundtrip() {
    let raw_hex = "03140015091E0D0500000033393434300101000001010001000057000004020A000B0A060000006A657374657200000000";
    let binary = hex::decode(raw_hex).unwrap();
    assert_eq!(binary.len(), 49);

    // 1. Export binary to JSON
    let json_str = export_parameter_to_json(&binary, "chunk_1881").unwrap();
    assert!(json_str.contains("asset_slot_descriptor"));
    assert!(json_str.contains("39440"));
    assert!(json_str.contains("jester"));

    // 2. Import JSON back to binary
    let rebuilt = import_parameter_from_json(json_str.as_bytes(), &binary).unwrap();

    // 3. Must match bit-for-bit with the original 49 bytes!
    assert_eq!(hex::encode_upper(&rebuilt), raw_hex);
}
