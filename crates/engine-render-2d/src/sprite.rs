//! 2D sprite component data.

use engine_core::AssetId;
use engine_ecs::MaterialRef;
use serde::{Deserialize, Serialize};

/// Texture region for a sprite.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpriteRegion {
    /// X offset in texture.
    pub x: f32,
    /// Y offset in texture.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

/// Serializable 2D sprite component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpriteComponentData {
    /// Texture asset GUID.
    pub texture: Option<AssetId>,
    /// Optional texture region.
    pub region: Option<SpriteRegion>,
    /// Tint color (RGBA).
    #[serde(default = "default_color")]
    pub color: [f32; 4],
    /// Flip horizontally.
    #[serde(default)]
    pub flip_h: bool,
    /// Flip vertically.
    #[serde(default)]
    pub flip_v: bool,
    /// Draw order within layer.
    #[serde(default)]
    pub order_in_layer: i32,
    /// Sorting layer name.
    #[serde(default = "default_layer")]
    pub layer: String,
    /// Optional material override.
    pub material_override: Option<MaterialRef>,
    /// Whether to center the sprite.
    #[serde(default)]
    pub centered: bool,
}

fn default_color() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

fn default_layer() -> String {
    "Default".to_string()
}

impl Default for SpriteComponentData {
    fn default() -> Self {
        Self {
            texture: None,
            region: None,
            color: default_color(),
            flip_h: false,
            flip_v: false,
            order_in_layer: 0,
            layer: default_layer(),
            material_override: None,
            centered: true,
        }
    }
}
