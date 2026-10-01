use anyhow::Result;
use byteorder::{LittleEndian, WriteBytesExt};
use serde_json::{Value, json};

pub struct GltfBuilder {
    bin_data: Vec<u8>,
    accessors: Vec<Value>,
    buffer_views: Vec<Value>,
    nodes: Vec<Value>,
    scenes: Vec<Value>,
    meshes: Vec<Value>,
    animations: Vec<Value>,
}

impl GltfBuilder {
    pub fn new() -> Self {
        Self {
            bin_data: Vec::new(),
            accessors: Vec::new(),
            buffer_views: Vec::new(),
            nodes: Vec::new(),
            scenes: Vec::new(),
            meshes: Vec::new(),
            animations: Vec::new(),
        }
    }

    fn align_buffer(&mut self) {
        while !self.bin_data.len().is_multiple_of(4) {
            self.bin_data.push(0);
        }
    }

    pub fn add_buffer_view(&mut self, data: &[u8], target: Option<u32>) -> usize {
        self.align_buffer();
        let byte_offset = self.bin_data.len();
        self.bin_data.extend_from_slice(data);

        let view_idx = self.buffer_views.len();
        let mut view = json!({
            "buffer": 0,
            "byteOffset": byte_offset,
            "byteLength": data.len(),
        });

        if let Some(t) = target {
            view["target"] = json!(t);
        }

        self.buffer_views.push(view);
        view_idx
    }

    pub fn add_accessor(
        &mut self,
        view_idx: usize,
        count: usize,
        comp_type: u32,
        acc_type: &str,
        min: Option<Vec<f32>>,
        max: Option<Vec<f32>>,
    ) -> usize {
        let acc_idx = self.accessors.len();
        let mut acc = json!({
            "bufferView": view_idx,
            "byteOffset": 0,
            "componentType": comp_type,
            "count": count,
            "type": acc_type
        });

        if let Some(m) = min {
            acc["min"] = json!(m);
        }
        if let Some(m) = max {
            acc["max"] = json!(m);
        }

        self.accessors.push(acc);
        acc_idx
    }

    pub fn add_mesh(&mut self, mesh: Value) -> usize {
        let idx = self.meshes.len();
        self.meshes.push(mesh);
        idx
    }

    pub fn add_node(&mut self, node: Value) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(node);
        idx
    }

    pub fn add_scene(&mut self, root_nodes: Vec<usize>) {
        self.scenes.push(json!({ "nodes": root_nodes }));
    }

    pub fn build(mut self, generator_name: &str) -> Result<Vec<u8>> {
        self.align_buffer();

        let mut gltf_json = json!({
            "asset": { "version": "2.0", "generator": generator_name },
            "scene": 0,
            "scenes": self.scenes,
            "nodes": self.nodes,
            "buffers": [{ "byteLength": self.bin_data.len() }],
            "bufferViews": self.buffer_views,
            "accessors": self.accessors
        });

        if !self.meshes.is_empty() {
            gltf_json["meshes"] = json!(self.meshes);
        }
        if !self.animations.is_empty() {
            gltf_json["animations"] = json!(self.animations);
        }

        let mut json_bytes = serde_json::to_vec(&gltf_json)?;
        while !json_bytes.len().is_multiple_of(4) {
            json_bytes.push(b' ');
        }

        let total_length = 12 + 8 + json_bytes.len() + 8 + self.bin_data.len();
        let mut glb = Vec::with_capacity(total_length);

        glb.extend_from_slice(b"glTF");
        glb.write_u32::<LittleEndian>(2)?;
        glb.write_u32::<LittleEndian>(total_length as u32)?;

        glb.write_u32::<LittleEndian>(json_bytes.len() as u32)?;
        glb.extend_from_slice(b"JSON");
        glb.extend_from_slice(&json_bytes);

        glb.write_u32::<LittleEndian>(self.bin_data.len() as u32)?;
        glb.extend_from_slice(b"BIN\0");
        glb.extend_from_slice(&self.bin_data);

        Ok(glb)
    }
}
