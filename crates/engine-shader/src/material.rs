//! Standard PBR material type.

use engine_core::{AssetId, EngineError, EngineResult};
use serde::{Deserialize, Serialize};

/// Alpha blending mode.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum AlphaMode {
    /// Fully opaque.
    #[default]
    Opaque,
    /// Alpha blending.
    Blend,
    /// Alpha testing with cutoff.
    Mask,
    /// Additive blending.
    Add,
    /// Multiply blending.
    Multiply,
}

/// Standard PBR material with metallic-roughness workflow.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StandardMaterial3D {
    /// Base color (RGBA).
    pub base_color: [f32; 4],
    /// Metallic factor (0.0 = dielectric, 1.0 = metal).
    pub metallic: f32,
    /// Roughness factor (0.0 = smooth, 1.0 = rough).
    pub roughness: f32,
    /// Emissive color (RGB).
    pub emissive: [f32; 3],
    /// Ambient occlusion factor.
    pub ambient_occlusion: f32,
    /// Normal map intensity scale.
    pub normal_scale: f32,
    /// Alpha blending mode.
    pub alpha_mode: AlphaMode,
    /// Alpha test cutoff threshold (for Mask mode).
    pub alpha_cutoff: f32,
    /// Base color texture asset GUID.
    pub base_color_texture: Option<AssetId>,
    /// Normal map texture asset GUID.
    pub normal_texture: Option<AssetId>,
    /// Metallic-roughness texture asset GUID.
    pub metallic_roughness_texture: Option<AssetId>,
    /// Ambient occlusion texture asset GUID.
    pub occlusion_texture: Option<AssetId>,
    /// Emissive texture asset GUID.
    pub emissive_texture: Option<AssetId>,
    /// Clearcoat factor.
    pub clearcoat: f32,
    /// Clearcoat roughness factor.
    pub clearcoat_roughness: f32,
    /// Subsurface scattering factor.
    pub subsurface: f32,
}

impl Default for StandardMaterial3D {
    fn default() -> Self {
        Self {
            base_color: [1.0, 1.0, 1.0, 1.0],
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0, 0.0, 0.0],
            ambient_occlusion: 1.0,
            normal_scale: 1.0,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            base_color_texture: None,
            normal_texture: None,
            metallic_roughness_texture: None,
            occlusion_texture: None,
            emissive_texture: None,
            clearcoat: 0.0,
            clearcoat_roughness: 0.0,
            subsurface: 0.0,
        }
    }
}

impl StandardMaterial3D {
    /// Serializes to JSON.
    pub fn to_json(&self) -> EngineResult<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| EngineError::other(format!("StandardMaterial3D serialization failed: {e}")))
    }

    /// Deserializes from JSON.
    pub fn from_json(input: &str) -> EngineResult<Self> {
        serde_json::from_str(input)
            .map_err(|e| EngineError::other(format!("StandardMaterial3D parse failed: {e}")))
    }

    /// Serializes to JSON bytes.
    pub fn to_binary(&self) -> EngineResult<Vec<u8>> {
        serde_json::to_vec(self)
            .map_err(|e| EngineError::other(format!("StandardMaterial3D binary serialization failed: {e}")))
    }

    /// Deserializes from JSON bytes.
    pub fn from_binary(bytes: &[u8]) -> EngineResult<Self> {
        serde_json::from_slice(bytes)
            .map_err(|e| EngineError::other(format!("StandardMaterial3D binary parse failed: {e}")))
    }
}
