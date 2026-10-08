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
// 3D MESH SHADER PIPELINE (NORMAL MAPPING & SKELETAL SKINNING)
// =========================================================================

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) tex_coords: vec2<f32>,
    @location(4) joints: vec4<u32>,
    @location(5) weights: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) world_tangent: vec4<f32>,
    @location(2) tex_coords: vec2<f32>,
    @location(3) world_pos: vec3<f32>,
};

@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    var local_pos = vec4<f32>(model.position, 1.0);
    var local_norm = model.normal;
    var local_tangent = model.tangent;

    let is_skinned = scene.params.y > 0.5;
    let weight_sum = model.weights.x + model.weights.y + model.weights.z + model.weights.w;

    // GPU Hardware Skeletal Skinning
    if (is_skinned && weight_sum > 0.001) {
        let j_x = min(model.joints.x, 127u);
        let j_y = min(model.joints.y, 127u);
        let j_z = min(model.joints.z, 127u);
        let j_w = min(model.joints.w, 127u);

        let bone_m = model.weights.x * bones.matrices[j_x]
                   + model.weights.y * bones.matrices[j_y]
                   + model.weights.z * bones.matrices[j_z]
                   + model.weights.w * bones.matrices[j_w];

        local_pos = bone_m * local_pos;
        local_norm = (bone_m * vec4<f32>(local_norm, 0.0)).xyz;
        local_tangent = vec4<f32>((bone_m * vec4<f32>(local_tangent.xyz, 0.0)).xyz, local_tangent.w);
    }

    // Default base coordinates: Native Direct3D 9 Y-up mapping
    let base_world_pos = vec3<f32>(local_pos.x, -local_pos.z, local_pos.y);
    let base_world_norm = vec3<f32>(local_norm.x, -local_norm.z, local_norm.y);
    let base_world_tan = vec4<f32>(local_tangent.x, -local_tangent.z, local_tangent.y, local_tangent.w);

    let up_axis = u32(scene.params.z);
    var world_pos = base_world_pos;
    var world_norm = base_world_norm;
    var world_tan = base_world_tan;

    if (up_axis == 1u) {
        // Pitch Up (+90°)
        world_pos = vec3<f32>(base_world_pos.x, -base_world_pos.z, base_world_pos.y);
        world_norm = vec3<f32>(base_world_norm.x, -base_world_norm.z, base_world_norm.y);
        world_tan = vec4<f32>(base_world_tan.x, -base_world_tan.z, base_world_tan.y, base_world_tan.w);
    } else if (up_axis == 2u) {
        // Pitch Down (-90°)
        world_pos = vec3<f32>(base_world_pos.x, base_world_pos.z, -base_world_pos.y);
        world_norm = vec3<f32>(base_world_norm.x, base_world_norm.z, -base_world_norm.y);
        world_tan = vec4<f32>(base_world_tan.x, base_world_tan.z, -base_world_tan.y, base_world_tan.w);
    } else if (up_axis == 3u) {
        // Inverted (180°)
        world_pos = vec3<f32>(base_world_pos.x, -base_world_pos.y, -base_world_pos.z);
        world_norm = vec3<f32>(base_world_norm.x, -base_world_norm.y, -base_world_norm.z);
        world_tan = vec4<f32>(base_world_tan.x, -base_world_tan.y, -base_world_tan.z, base_world_tan.w);
    } else if (up_axis == 4u) {
        // Roll Left (+90°)
        world_pos = vec3<f32>(-base_world_pos.y, base_world_pos.x, base_world_pos.z);
        world_norm = vec3<f32>(-base_world_norm.y, base_world_norm.x, base_world_norm.z);
        world_tan = vec4<f32>(-base_world_tan.y, base_world_tan.x, base_world_tan.z, base_world_tan.w);
    } else if (up_axis == 5u) {
        // Roll Right (-90°)
        world_pos = vec3<f32>(base_world_pos.y, -base_world_pos.x, base_world_pos.z);
        world_norm = vec3<f32>(base_world_norm.y, -base_world_norm.x, base_world_norm.z);
        world_tan = vec4<f32>(base_world_tan.y, -base_world_tan.x, base_world_tan.z, base_world_tan.w);
    }

    out.clip_position = scene.view_proj * vec4<f32>(world_pos, 1.0);
    out.world_normal = world_norm;
    out.world_tangent = world_tan;
    out.tex_coords = model.tex_coords;
    out.world_pos = world_pos;
    return out;
}

@group(1) @binding(0)
var t_diffuse: texture_2d<f32>;
@group(1) @binding(1)
var s_diffuse: sampler;
@group(1) @binding(2)
var t_normal: texture_2d<f32>;
@group(1) @binding(3)
var s_normal: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    let mode = u32(scene.params.x);

    // Mode 2: Unlit
    if (mode == 2u) {
        return vec4<f32>(tex_color.rgb, 1.0);
    }

    // TBN (Tangent, Bitangent, Normal) Basis Matrix Calculation
    let N = normalize(in.world_normal);
    let T_raw = normalize(in.world_tangent.xyz);
    let T = normalize(T_raw - dot(T_raw, N) * N);
    let B = cross(N, T) * in.world_tangent.w;
    let TBN = mat3x3<f32>(T, B, N);

    // Unpack Tangent Space Normal Vector
    let norm_map_raw = textureSample(t_normal, s_normal, in.tex_coords).rgb;
    let local_normal = normalize(norm_map_raw * 2.0 - vec3<f32>(1.0));
    let norm = normalize(TBN * local_normal);

    // 3-Point Studio Lighting
    let key_dir = normalize(vec3<f32>(0.5, 0.85, 0.65));
    let fill_dir = normalize(vec3<f32>(-0.6, 0.35, -0.5));
    let back_dir = normalize(vec3<f32>(0.0, -0.8, -0.6));

    let key_diff = max(dot(norm, key_dir), 0.0) * 0.75;
    let fill_diff = max(dot(norm, fill_dir), 0.0) * 0.35;
    let back_diff = max(dot(norm, back_dir), 0.0) * 0.15;

    // Specular Highlight (Blinn-Phong)
    let view_dir = normalize(-in.world_pos);
    let half_vec = normalize(key_dir + view_dir);
    let spec = pow(max(dot(norm, half_vec), 0.0), 24.0) * 0.25;

    var ambient = 0.35;
    if (mode == 1u) {
        ambient = 0.65;
    }

    let lighting = ambient + key_diff + fill_diff + back_diff;
    return vec4<f32>(tex_color.rgb * lighting + vec3<f32>(spec), 1.0);
}

// =========================================================================
// 3D GROUND GRID & DEBUG LINES SHADER PIPELINE
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

// =========================================================================
// HARDWARE-ACCELERATED AXIS GIZMO SHADER PIPELINE (SCREEN NDC OVERLAY)
// =========================================================================

@vertex
fn vs_gizmo(model: GridVertexInput) -> GridVertexOutput {
    var out: GridVertexOutput;
    out.clip_position = vec4<f32>(model.position.xy, 0.0, 1.0);
    out.color = model.color;
    return out;
}

@fragment
fn fs_gizmo(in: GridVertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
