//! Material instance system with parameter overrides.

use std::collections::HashMap;

use engine_core::AssetId;
use serde::{Deserialize, Serialize};

/// A material instance that references a base material and overrides specific parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaterialInstance {
    /// Base material name (builtin) or asset GUID.
    pub base_material: String,
    /// Overridden numeric parameters.
    #[serde(default)]
    pub parameter_overrides: HashMap<String, MaterialParam>,
    /// Overridden texture slots.
    #[serde(default)]
    pub texture_overrides: HashMap<String, AssetId>,
    /// Optional shader override.
    pub shader_override: Option<String>,
}

/// A material parameter value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MaterialParam {
    /// Float scalar.
    Float(f32),
    /// Vec2.
    Vec2([f32; 2]),
    /// Vec3.
    Vec3([f32; 3]),
    /// Vec4.
    Vec4([f32; 4]),
    /// Integer.
    Int(i32),
    /// Boolean.
    Bool(bool),
}

impl MaterialInstance {
    /// Creates a material instance referencing a base material.
    pub fn new(base_material: impl Into<String>) -> Self {
        Self {
            base_material: base_material.into(),
            parameter_overrides: HashMap::new(),
            texture_overrides: HashMap::new(),
            shader_override: None,
        }
    }

    /// Sets a float parameter override.
    pub fn set_float(&mut self, name: impl Into<String>, value: f32) {
        self.parameter_overrides
            .insert(name.into(), MaterialParam::Float(value));
    }

    /// Sets a color parameter override.
    pub fn set_color(&mut self, name: impl Into<String>, rgba: [f32; 4]) {
        self.parameter_overrides
            .insert(name.into(), MaterialParam::Vec4(rgba));
    }

    /// Sets a texture slot override.
    pub fn set_texture(&mut self, slot: impl Into<String>, asset: AssetId) {
        self.texture_overrides.insert(slot.into(), asset);
    }
}

impl Default for MaterialInstance {
    fn default() -> Self {
        Self::new("debug/default")
    }
}
