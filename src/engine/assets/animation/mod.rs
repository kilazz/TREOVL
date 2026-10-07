pub mod bone;
pub mod clip;
pub mod diagnostics;
pub mod gltf_export;
pub mod kinematics;

// Unified re-exports to maintain 100% backward compatibility across the codebase
pub use bone::{ObjectBone, RawObjectBone, parse_object_bone_container};
pub use clip::{
    AnimationClip, BoneTrack, KeyframeRotation, KeyframeTranslation, parse_animation_clip,
};
pub use diagnostics::run_deep_skeleton_diagnostics;
pub use gltf_export::{
    export_animation_to_glb, export_animation_to_json, export_skeleton_to_glb, gltf_basis_quat,
};
pub use kinematics::compute_skinning_matrices;
