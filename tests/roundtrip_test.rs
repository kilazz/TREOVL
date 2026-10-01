use OVLTool::engine::assets::{build_chunk_from_elements, parse_chunk_elements};
use OVLTool::engine::container::footer::{
    MAGIC_FOOTER_1, MAGIC_FOOTER_2, calculate_triumph_crc32, check_footer,
};
use OVLTool::utils::gltf_builder::GltfBuilder;

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

    // Check glTF binary header magic "glTF" and version 2
    assert!(glb.starts_with(b"glTF"));
    assert_eq!(u32::from_le_bytes(glb[4..8].try_into().unwrap()), 2);
}
