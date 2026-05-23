//! 2D render world extraction from scene.

use engine_core::math::Transform;
use engine_core::EntityId;

/// Camera data extracted for 2D rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderCamera2D {
    /// Source scene object ID.
    pub object: EntityId,
    /// World transform.
    pub transform: Transform,
    /// Camera zoom.
    pub zoom: f32,
}

/// Sprite data extracted for 2D rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderSprite {
    /// Source scene object ID.
    pub object: EntityId,
    /// World transform.
    pub transform: Transform,
    /// Tint color (RGBA).
    pub color: [f32; 4],
    /// Texture identifier.
    pub texture: String,
    /// Draw order within layer.
    pub order_in_layer: i32,
    /// Sorting layer name.
    pub layer: String,
    /// Whether the sprite is flipped horizontally.
    pub flip_h: bool,
    /// Whether the sprite is flipped vertically.
    pub flip_v: bool,
}

/// Tile map data extracted for 2D rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderTileMap {
    /// Source scene object ID.
    pub object: EntityId,
    /// World transform.
    pub transform: Transform,
    /// Tileset texture identifier.
    pub tileset: String,
    /// Tile size in pixels.
    pub tile_size: u32,
    /// Map dimensions in tiles.
    pub map_size: (u32, u32),
    /// Sorting layer name.
    pub layer: String,
}

/// 2D light extracted for rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderLight2D {
    /// Source scene object ID.
    pub object: EntityId,
    /// World transform.
    pub transform: Transform,
    /// Light color.
    pub color: engine_core::math::Vec3,
    /// Light intensity.
    pub intensity: f32,
    /// Light range.
    pub range: f32,
}

/// 2D occluder extracted for shadow casting.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderOccluder2D {
    /// Source scene object ID.
    pub object: EntityId,
    /// World transform.
    pub transform: Transform,
    /// Occlusion polygon vertices.
    pub polygon: Vec<[f32; 2]>,
}

/// 2D render world — flat queue extracted from scene for 2D rendering backends.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderWorld2D {
    /// Active 2D camera.
    pub camera: Option<RenderCamera2D>,
    /// Queued sprites.
    pub sprites: Vec<RenderSprite>,
    /// Queued tile maps.
    pub tilemaps: Vec<RenderTileMap>,
    /// Queued 2D lights.
    pub lights: Vec<RenderLight2D>,
    /// Queued occluders.
    pub occluders: Vec<RenderOccluder2D>,
}

impl RenderWorld2D {
    /// Returns true when there is visible geometry and a camera.
    pub fn is_visible(&self) -> bool {
        self.camera.is_some()
            && (!self.sprites.is_empty() || !self.tilemaps.is_empty())
    }

    /// Extracts 2D renderable data from a Scene.
    ///
    /// Iterates all active game objects and extracts Camera2D, Sprite,
    /// TileMap, Light2D, and Occluder2D components.
    pub fn extract(scene: &engine_ecs::Scene) -> Self {
        let mut world = RenderWorld2D::default();

        for (_entity, obj) in scene.iter_objects() {
            if !obj.active {
                continue;
            }

            for component in &obj.components {
                match component {
                    engine_ecs::ComponentData::Sprite2D(sprite) => {
                        let texture_name = sprite
                            .texture
                            .map(|id| format!("asset:{:016x}", id.as_u128()))
                            .unwrap_or_default();
                        world.sprites.push(RenderSprite {
                            object: obj.id,
                            transform: Transform::IDENTITY,
                            color: sprite.color,
                            texture: texture_name,
                            order_in_layer: sprite.order_in_layer,
                            layer: sprite.layer.clone(),
                            flip_h: sprite.flip_h,
                            flip_v: sprite.flip_v,
                        });
                    }
                    engine_ecs::ComponentData::TileMap(tilemap) => {
                        let tileset_name = tilemap
                            .tileset
                            .map(|id| format!("asset:{:016x}", id.as_u128()))
                            .unwrap_or_default();
                        world.tilemaps.push(RenderTileMap {
                            object: obj.id,
                            transform: Transform::IDENTITY,
                            tileset: tileset_name,
                            tile_size: tilemap.tile_size,
                            map_size: tilemap.map_size,
                            layer: tilemap.layer.clone(),
                        });
                    }
                    engine_ecs::ComponentData::Camera2D(cam) => {
                        world.camera = Some(RenderCamera2D {
                            object: obj.id,
                            transform: Transform::IDENTITY,
                            zoom: cam.zoom,
                        });
                    }
                    engine_ecs::ComponentData::Light2D(light) => {
                        world.lights.push(RenderLight2D {
                            object: obj.id,
                            transform: Transform::IDENTITY,
                            color: light.color,
                            intensity: light.intensity,
                            range: light.range,
                        });
                    }
                    engine_ecs::ComponentData::Occluder2D(occluder) => {
                        world.occluders.push(RenderOccluder2D {
                            object: obj.id,
                            transform: Transform::IDENTITY,
                            polygon: occluder.polygon.clone(),
                        });
                    }
                    _ => {}
                }
            }
        }

        world
    }
}
