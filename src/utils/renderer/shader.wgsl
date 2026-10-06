struct SceneUniform {
    view_proj: mat4x4<f32>,
    params: vec4<f32>, // x: lighting_mode, y: is_skinned, z: up_axis, w: padding
};

struct BonesUniform {
    matrices: array<mat4x4<f32>, 128>,
};

@group(0) @binding(0)
var<uniform> scene: SceneUniform;

@group(0) @binding(1)
var<uniform> bones: BonesUniform;

// =========================================================================
// 1. 3D MESH SHADER PIPELINE (WITH GPU SKELETAL SKINNING)
// =========================================================================

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tex_coords: vec2<f32>,
    @location(3) joints: vec4<u32>,
    @location(4) weights: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
};

@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    var local_pos = vec4<f32>(model.position, 1.0);
    var local_norm = model.normal;

    // GPU Vertex Skinning: linear blend skinning across 4 bone weights
    let is_skinned = scene.params.y > 0.5;
    if (is_skinned && (model.weights.x + model.weights.y + model.weights.z + model.weights.w) > 0.001) {
        let bone_m = model.weights.x * bones.matrices[model.joints.x]
                   + model.weights.y * bones.matrices[model.joints.y]
                   + model.weights.z * bones.matrices[model.joints.z]
                   + model.weights.w * bones.matrices[model.joints.w];

        local_pos = bone_m * local_pos;
        local_norm = (bone_m * vec4<f32>(local_norm, 0.0)).xyz;
    }

    // Global Viewport Orientation Converter:
    // Mode 0 (Default / Ground): [x, y, z] (Already synchronized with skeleton)
    // Mode 1 (Pitch Up +90°): [x, -z, y]
    // Mode 2 (Pitch Down -90°): [x, z, -y]
    // Mode 3 (Invert 180°): [x, -y, -z]
    let up_axis = u32(scene.params.z);
    var world_pos = local_pos.xyz;
    var world_norm = local_norm;

    if (up_axis == 1u) {
        world_pos = vec3<f32>(local_pos.x, -local_pos.z, local_pos.y);
        world_norm = vec3<f32>(local_norm.x, -local_norm.z, local_norm.y);
    } else if (up_axis == 2u) {
        world_pos = vec3<f32>(local_pos.x, local_pos.z, -local_pos.y);
        world_norm = vec3<f32>(local_norm.x, local_norm.z, -local_norm.y);
    } else if (up_axis == 3u) {
        world_pos = vec3<f32>(local_pos.x, -local_pos.y, -local_pos.z);
        world_norm = vec3<f32>(local_norm.x, -local_norm.y, -local_norm.z);
    }

    out.clip_position = scene.view_proj * vec4<f32>(world_pos, 1.0);
    out.world_normal = world_norm;
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
// 2. 3D GROUND GRID & DEBUG LINES SHADER PIPELINE
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
