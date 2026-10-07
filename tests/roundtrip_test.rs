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
use TREOVL::engine::assets::vfx::{
    VfxEmitterJson, VfxEngineMetadataJson, VfxGraphJson, VfxPropertyBlockJson, export_vfx_to_json,
    import_vfx_from_json_with_endian,
};
use TREOVL::engine::assets::{build_chunk_from_elements, parse_chunk_elements};
use TREOVL::engine::common::Endian;
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
fn test_character_pure_in_memory_roundtrip_le_and_be() {
    let character = CharacterActorJson {
        _engine_metadata: Default::default(),
        character_name: "Minion_Brown_PureTest".to_string(),
        resource_tag: Some("Minion_Test_Tag".to_string()),
        is_baby: Some(false),
        model_binding: None,
        breakable_config: None,
        collapse_target_model: None,
        actor_flags: None,
        combat_attributes: Some(CharacterAttributesJson {
            base_health: Some(42.5),
            move_speed_scale: Some(1.35),
            aggro_range: Some(18.0),
            ..Default::default()
        }),
        timing_parameters: None,
        knockback_parameters: None,
        state_and_rewards: None,
        morph_parameters: None,
        equipment: None,
        facefx_actor: None,
        embedded_facefx_file: None,
        lifeforce_color: None,
        lua_script_file: None,
        embedded_lua_script: Some("GetAlias()\nPrint(\"Test\")".to_string()),
        animation_states: Vec::new(),
        ai_behaviors: vec!["AttackState".to_string(), "FleeState".to_string()],
        transformations: Vec::new(),
        effect_receptors: Vec::new(),
        minion_grapple_bones: Vec::new(),
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
        extracted_le
            .character
            .combat_attributes
            .as_ref()
            .and_then(|a| a.base_health),
        Some(42.5)
    );
    assert_eq!(
        extracted_le
            .character
            .combat_attributes
            .as_ref()
            .and_then(|a| a.move_speed_scale),
        Some(1.35)
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
            type_id_hex: "0041004B".to_string(),
            class_name: Some("TREModelResource".to_string()),
            raw_fallbacks: Vec::new(),
        },
        group_tag: Some("Props_Group".to_string()),
        entity_name: Some("Barrel_Explosive".to_string()),
        scale: Some([1.5, 1.5, 2.0]),
        bounding_box: Some(BoundingBoxJson {
            center: [0.0, 1.0, 0.0],
            half_extents: [0.5, 1.0, 0.5],
            orientation_matrix: None,
        }),
        default_animation: None,
        physics_state: None,
        ragdoll_bone_groups: Vec::new(),
        mesh_bindings: Vec::new(),
        stand_model: None,
        placed_object: None,
        placement_offset: None,
        placement_config: None,
        bones: Vec::new(),
        attachments: Vec::new(),
        has_sentinel_terminator: true,
    };

    let json_str = serde_json::to_string_pretty(&object).unwrap();

    // Little Endian
    let bin_le = import_object_from_json_with_endian(&json_str, Endian::Little)
        .expect("Object import LE must succeed");
    let extracted_le = export_object(&bin_le).expect("Object export LE must succeed");

    assert_eq!(
        extracted_le.entity.entity_name.as_deref(),
        Some("Barrel_Explosive")
    );
    assert_eq!(extracted_le.entity.scale, Some([1.5, 1.5, 2.0]));
    assert!(extracted_le.entity.has_sentinel_terminator);

    // Big Endian
    let bin_be = import_object_from_json_with_endian(&json_str, Endian::Big)
        .expect("Object import BE must succeed");
    assert!(!bin_be.is_empty());
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
                char_code: 65, // 'A'
                character: "A".to_string(),
                width: 14.0,
                height: 18.0,
                uv_min: [0.1, 0.2],
                uv_max: [0.2, 0.3],
            },
            GlyphMetricJson {
                char_code: 66, // 'B'
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
    // A point light candidate record is 32 bytes:
    // x, y, z (f32), radius (f32), r, g, b (f32), intensity (f32)
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
