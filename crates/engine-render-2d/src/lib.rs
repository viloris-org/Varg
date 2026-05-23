#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! 2D rendering pipeline: sprites, tilemaps, 2D lights, camera, and extraction.

pub mod camera2d;
pub mod light2d;
pub mod render_world_2d;
pub mod sprite;
pub mod tilemap;

pub use camera2d::{Anchor2D, Camera2DComponentData, ParallaxLayer};
pub use light2d::{Light2DComponentData, Light2DKind, Occluder2DComponentData};
pub use render_world_2d::{
    RenderCamera2D, RenderLight2D, RenderOccluder2D, RenderSprite, RenderTileMap, RenderWorld2D,
};
pub use sprite::{SpriteComponentData, SpriteRegion};
pub use tilemap::{TileData, TileMapComponentData};
