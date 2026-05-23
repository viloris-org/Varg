//! Tile map component data.

use engine_core::AssetId;
use engine_ecs::MaterialRef;
use serde::{Deserialize, Serialize};

/// Single tile data.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TileData {
    /// Tile index in the tileset.
    pub index: u32,
    /// Flip horizontally.
    #[serde(default)]
    pub flip_h: bool,
    /// Flip vertically.
    #[serde(default)]
    pub flip_v: bool,
}

impl Default for TileData {
    fn default() -> Self {
        Self {
            index: 0,
            flip_h: false,
            flip_v: false,
        }
    }
}

/// Serializable tile map component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TileMapComponentData {
    /// Tileset texture asset GUID.
    pub tileset: Option<AssetId>,
    /// Tile size in pixels.
    #[serde(default = "default_tile_size")]
    pub tile_size: u32,
    /// Map dimensions in tiles (width, height).
    #[serde(default)]
    pub map_size: (u32, u32),
    /// Tile data.
    #[serde(default)]
    pub tiles: Vec<TileData>,
    /// Sorting layer name.
    #[serde(default = "default_layer")]
    pub layer: String,
    /// Optional material override.
    pub material_override: Option<MaterialRef>,
}

fn default_tile_size() -> u32 {
    16
}

fn default_layer() -> String {
    "Default".to_string()
}

impl Default for TileMapComponentData {
    fn default() -> Self {
        Self {
            tileset: None,
            tile_size: default_tile_size(),
            map_size: (1, 1),
            tiles: Vec::new(),
            layer: default_layer(),
            material_override: None,
        }
    }
}
