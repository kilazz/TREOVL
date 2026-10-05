struct SceneUniform {
    view_proj: mat4x4<f32>,
    params: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> scene: SceneUniform;

// =========================================================================
// 1. 3D MESH SHADER PIPELINE
// =========================================================================

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tex_coords: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
};

@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = scene.view_proj * vec4<f32>(model.position, 1.0);
    out.world_normal = model.normal;
    out.tex_coords = model.tex_coords;
    return out;
}

@group(1) @binding(0)
var t_diffuse: texture_2d<f32>;
@group(1) @binding(1)
var s_diffuse: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    let mode = u32(scene.params.x);

    // Mode 2: Unlit (Pure Albedo / Texture inspect)
    if (mode == 2u) {
        return vec4<f32>(tex_color.rgb, 1.0);
    }

    let norm = normalize(in.world_normal);

    // 3-Point Double-Sided Studio Lighting Rig
    let key_dir = normalize(vec3<f32>(0.5, 0.85, 0.65));
    let fill_dir = normalize(vec3<f32>(-0.6, 0.35, -0.5));
    let back_dir = normalize(vec3<f32>(0.0, -0.8, -0.6));

    // abs() ensures two-sided illumination on thin/double-sided geometry
    let key_diff = abs(dot(norm, key_dir)) * 0.75;
    let fill_diff = abs(dot(norm, fill_dir)) * 0.35;
    let back_diff = abs(dot(norm, back_dir)) * 0.15;

    // Mode 1: Bright Fill Boost
    var ambient = 0.35;
    if (mode == 1u) {
        ambient = 0.65;
    }

    let lighting = ambient + key_diff + fill_diff + back_diff;
    return vec4<f32>(tex_color.rgb * lighting, 1.0);
}

// =========================================================================
// 2. 3D GROUND GRID SHADER PIPELINE
// =========================================================================

struct GridVertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct GridVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_grid(model: GridVertexInput) -> GridVertexOutput {
    var out: GridVertexOutput;
    out.clip_position = scene.view_proj * vec4<f32>(model.position, 1.0);
    out.color = model.color;
    return out;
}

@fragment
fn fs_grid(in: GridVertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
