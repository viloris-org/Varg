//! Scene graph, game object metadata, lifecycle, and serialization support.

use std::{collections::HashMap, fmt};

use engine_core::{
    math::{Transform, Vec3},
    AssetId, EngineError, EngineResult, EntityId,
};

use crate::{
    transform::TransformHierarchy,
    world::{Entity, Lifecycle, World},
};

/// Scene file schema version.
pub const SCENE_FILE_VERSION: u32 = 1;

/// Camera query role attached to a game object.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum CameraRole {
    /// Editor or runtime main camera.
    Main,
    /// Runtime gameplay camera.
    Game,
}

/// Lifecycle stage exposed by the scene API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleStage {
    /// Called once before the first update.
    Start,
    /// Called once per variable frame.
    Update,
    /// Called on fixed timestep ticks.
    FixedUpdate,
    /// Called after regular updates.
    LateUpdate,
    /// Called by editor-only ticking.
    EditorUpdate,
}

impl From<LifecycleStage> for Lifecycle {
    fn from(value: LifecycleStage) -> Self {
        match value {
            LifecycleStage::Start => Self::Start,
            LifecycleStage::Update => Self::Update,
            LifecycleStage::FixedUpdate => Self::FixedUpdate,
            LifecycleStage::LateUpdate => Self::LateUpdate,
            LifecycleStage::EditorUpdate => Self::EditorUpdate,
        }
    }
}

/// Active scene mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneMode {
    /// Scene edits apply to the edit-time state.
    Edit,
    /// Scene edits apply to an isolated play copy.
    Play,
}

/// Monotonic object ID allocator for one runtime session.
#[derive(Clone, Debug)]
pub struct ObjectIdAllocator {
    next: u128,
}

impl Default for ObjectIdAllocator {
    fn default() -> Self {
        Self { next: 1 }
    }
}

impl ObjectIdAllocator {
    /// Allocates a stable object ID for this runtime session.
    pub fn allocate(&mut self) -> EntityId {
        let id = EntityId::from_u128(self.next);
        self.next = self.next.saturating_add(1).max(1);
        id
    }

    /// Observes an ID loaded from disk and advances the allocator past it.
    pub fn observe(&mut self, id: EntityId) {
        self.next = self.next.max(id.as_u128().saturating_add(1));
    }
}

/// Script component placeholder stored without requiring a script backend.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ScriptComponentProxy {
    /// Stable backend name, for example `python`.
    pub backend: String,
    /// Script module or asset path.
    pub script: String,
    /// Serialized script state as opaque JSON.
    pub state_json: Option<String>,
    /// Whether the component awaits backend recovery.
    pub pending_recovery: bool,
}

/// Serializable camera component.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CameraComponentData {
    /// Vertical field of view in degrees.
    pub vertical_fov_degrees: f32,
    /// Near clipping plane.
    pub near: f32,
    /// Far clipping plane.
    pub far: f32,
    /// Optional fixed aspect ratio. Runtime derives it from the target when unset.
    pub aspect_ratio: Option<f32>,
    /// Whether this camera should render to Game View.
    pub primary: bool,
    /// RGB clear color when this camera renders.
    #[serde(default = "default_clear_color")]
    pub clear_color: Vec3,
}

fn default_clear_color() -> Vec3 {
    Vec3::new(0.1, 0.1, 0.1)
}

impl Default for CameraComponentData {
    fn default() -> Self {
        Self {
            vertical_fov_degrees: 60.0,
            near: 0.01,
            far: 1000.0,
            aspect_ratio: None,
            primary: true,
            clear_color: default_clear_color(),
        }
    }
}

/// Serializable material reference.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct MaterialRef {
    /// Optional asset GUID for a project material.
    pub asset: Option<AssetId>,
    /// Built-in material name used when no asset is assigned.
    pub builtin: Option<String>,
}

impl MaterialRef {
    /// Debug material used for fallback rendering.
    pub fn debug() -> Self {
        Self {
            asset: None,
            builtin: Some("debug/default".to_string()),
        }
    }
}

/// Serializable mesh renderer component.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct MeshRendererComponentData {
    /// Optional mesh asset GUID.
    pub mesh: Option<AssetId>,
    /// Built-in mesh name used when no asset is assigned.
    pub builtin_mesh: Option<String>,
    /// Material binding.
    pub material: MaterialRef,
    /// Whether this renderer casts shadows.
    pub casts_shadows: bool,
    /// Whether this renderer receives shadows.
    #[serde(default = "default_true")]
    pub receive_shadows: bool,
}

fn default_true() -> bool {
    true
}

impl Default for MeshRendererComponentData {
    fn default() -> Self {
        Self {
            mesh: None,
            builtin_mesh: Some("debug/cube".to_string()),
            material: MaterialRef::debug(),
            casts_shadows: true,
            receive_shadows: true,
        }
    }
}

/// Serializable light component.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct LightComponentData {
    /// RGB light color.
    pub color: Vec3,
    /// Light intensity in engine units.
    pub intensity: f32,
    /// Whether this is directional, point, or spot.
    pub kind: String,
    /// Light range (point/spot only).
    #[serde(default = "default_light_range")]
    pub range: f32,
    /// Spot inner cone angle in degrees (spot only).
    #[serde(default = "default_spot_angle")]
    pub spot_angle: f32,
}

fn default_light_range() -> f32 {
    10.0
}

fn default_spot_angle() -> f32 {
    30.0
}

impl Default for LightComponentData {
    fn default() -> Self {
        Self {
            color: Vec3::ONE,
            intensity: 1.0,
            kind: "directional".to_string(),
            range: default_light_range(),
            spot_angle: default_spot_angle(),
        }
    }
}

/// Serializable rigidbody component.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct RigidbodyComponentData {
    /// `dynamic`, `kinematic`, or `static`.
    pub body_type: String,
    /// Rigidbody mass.
    pub mass: f32,
    /// Whether gravity affects the body.
    pub use_gravity: bool,
    /// Linear velocity damping.
    #[serde(default)]
    pub linear_damping: f32,
    /// Angular velocity damping.
    #[serde(default = "default_angular_damping")]
    pub angular_damping: f32,
    /// Lock position axes [x, y, z].
    #[serde(default)]
    pub lock_position: [bool; 3],
    /// Lock rotation axes [x, y, z].
    #[serde(default)]
    pub lock_rotation: [bool; 3],
}

fn default_angular_damping() -> f32 {
    0.05
}

impl Default for RigidbodyComponentData {
    fn default() -> Self {
        Self {
            body_type: "dynamic".to_string(),
            mass: 1.0,
            use_gravity: true,
            linear_damping: 0.0,
            angular_damping: default_angular_damping(),
            lock_position: [false; 3],
            lock_rotation: [false; 3],
        }
    }
}

/// Serializable collider component.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ColliderComponentData {
    /// `box`, `sphere`, or `capsule`.
    pub shape: String,
    /// Shape dimensions.
    pub size: Vec3,
    /// Whether this collider is a trigger.
    pub is_trigger: bool,
    /// Bitmask of layers this collider can interact with.
    #[serde(default = "default_collider_mask")]
    pub mask: u32,
    /// Physics material preset name.
    #[serde(default = "default_physics_material")]
    pub physics_material: String,
}

fn default_physics_material() -> String {
    "default".to_string()
}

impl Default for ColliderComponentData {
    fn default() -> Self {
        Self {
            shape: "box".to_string(),
            size: Vec3::ONE,
            is_trigger: false,
            mask: default_collider_mask(),
            physics_material: default_physics_material(),
        }
    }
}

fn default_collider_mask() -> u32 {
    !0
}

/// Serializable audio source component.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct AudioSourceComponentData {
    /// Optional clip asset GUID.
    pub clip: Option<AssetId>,
    /// Volume multiplier.
    pub volume: f32,
    /// Whether the clip loops.
    pub looping: bool,
    /// Whether playback starts on scene start.
    pub play_on_start: bool,
    /// Blend between 2D (0.0) and 3D (1.0) spatial audio.
    #[serde(default)]
    pub spatial_blend: f32,
}

impl Default for AudioSourceComponentData {
    fn default() -> Self {
        Self {
            clip: None,
            volume: 1.0,
            looping: false,
            play_on_start: false,
            spatial_blend: 0.0,
        }
    }
}

pub use crate::particle::ParticleEmitterComponentData;

/// Serializable 2D sprite component.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Sprite2DComponentData {
    /// Texture asset GUID.
    pub texture: Option<AssetId>,
    /// Tint color (RGBA).
    #[serde(default = "default_sprite_color")]
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
    #[serde(default = "default_sprite_layer")]
    pub layer: String,
    /// Whether the sprite is centered.
    #[serde(default = "default_true")]
    pub centered: bool,
}

fn default_sprite_color() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

fn default_sprite_layer() -> String {
    "Default".to_string()
}

impl Default for Sprite2DComponentData {
    fn default() -> Self {
        Self {
            texture: None,
            color: default_sprite_color(),
            flip_h: false,
            flip_v: false,
            order_in_layer: 0,
            layer: default_sprite_layer(),
            centered: true,
        }
    }
}

/// Serializable 2D tile map component.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TileMap2DComponentData {
    /// Tileset texture asset GUID.
    pub tileset: Option<AssetId>,
    /// Tile size in pixels.
    #[serde(default = "default_tile_size")]
    pub tile_size: u32,
    /// Map dimensions in tiles (width, height).
    #[serde(default)]
    pub map_size: (u32, u32),
    /// Tile indices.
    #[serde(default)]
    pub tiles: Vec<u32>,
    /// Sorting layer name.
    #[serde(default = "default_tile_layer")]
    pub layer: String,
}

fn default_tile_size() -> u32 {
    16
}

fn default_tile_layer() -> String {
    "Default".to_string()
}

impl Default for TileMap2DComponentData {
    fn default() -> Self {
        Self {
            tileset: None,
            tile_size: default_tile_size(),
            map_size: (1, 1),
            tiles: Vec::new(),
            layer: default_tile_layer(),
        }
    }
}

/// Serializable 2D camera component.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Camera2DComponentData {
    /// Zoom level.
    #[serde(default = "default_camera2d_zoom")]
    pub zoom: f32,
    /// RGB clear color.
    #[serde(default = "default_camera2d_clear_color")]
    pub clear_color: Vec3,
}

fn default_camera2d_zoom() -> f32 {
    1.0
}

fn default_camera2d_clear_color() -> Vec3 {
    Vec3::new(0.1, 0.1, 0.1)
}

impl Default for Camera2DComponentData {
    fn default() -> Self {
        Self {
            zoom: default_camera2d_zoom(),
            clear_color: default_camera2d_clear_color(),
        }
    }
}

/// Serializable 2D light component.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Light2DComponentData {
    /// RGB light color.
    pub color: Vec3,
    /// Light intensity.
    #[serde(default = "default_light2d_intensity")]
    pub intensity: f32,
    /// Light range in world units.
    #[serde(default = "default_light2d_range")]
    pub range: f32,
}

fn default_light2d_intensity() -> f32 {
    1.0
}

fn default_light2d_range() -> f32 {
    10.0
}

impl Default for Light2DComponentData {
    fn default() -> Self {
        Self {
            color: Vec3::ONE,
            intensity: 1.0,
            range: 10.0,
        }
    }
}

/// Serializable 2D occluder component.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Occluder2DComponentData {
    /// Occlusion polygon vertices in local space.
    #[serde(default = "default_occluder_polygon")]
    pub polygon: Vec<[f32; 2]>,
}

fn default_occluder_polygon() -> Vec<[f32; 2]> {
    vec![[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]]
}

impl Default for Occluder2DComponentData {
    fn default() -> Self {
        Self {
            polygon: default_occluder_polygon(),
        }
    }
}

/// Serializable animation player component.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct AnimationPlayerComponentData {
    /// Animation clip asset GUID.
    pub clip: Option<AssetId>,
    /// Whether to auto-play on scene start.
    #[serde(default)]
    pub auto_play: bool,
    /// Playback speed multiplier.
    #[serde(default = "default_anim_speed")]
    pub speed: f32,
}

fn default_anim_speed() -> f32 {
    1.0
}

impl Default for AnimationPlayerComponentData {
    fn default() -> Self {
        Self {
            clip: None,
            auto_play: false,
            speed: 1.0,
        }
    }
}

/// Serializable skinned mesh renderer component.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SkinnedMeshRendererComponentData {
    /// Mesh asset GUID.
    pub mesh: Option<AssetId>,
    /// Material reference.
    pub material: MaterialRef,
    /// Entity with the Skeleton component.
    pub skeleton_root: Option<EntityId>,
    /// Whether this renderer casts shadows.
    #[serde(default = "default_true")]
    pub casts_shadows: bool,
}

impl Default for SkinnedMeshRendererComponentData {
    fn default() -> Self {
        Self {
            mesh: None,
            material: MaterialRef::debug(),
            skeleton_root: None,
            casts_shadows: true,
        }
    }
}

/// Serializable audio stream player 2D component.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct AudioStreamPlayer2DComponentData {
    /// Audio clip asset GUID.
    pub clip: Option<AssetId>,
    /// Output bus name.
    #[serde(default = "default_bus_name")]
    pub bus: String,
    /// Volume multiplier.
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Whether to loop.
    #[serde(default)]
    pub looping: bool,
    /// Whether to auto-play on scene start.
    #[serde(default)]
    pub play_on_start: bool,
}

fn default_bus_name() -> String {
    "SFX".to_string()
}

fn default_volume() -> f32 {
    1.0
}

impl Default for AudioStreamPlayer2DComponentData {
    fn default() -> Self {
        Self {
            clip: None,
            bus: "SFX".to_string(),
            volume: 1.0,
            looping: false,
            play_on_start: false,
        }
    }
}

/// Serializable audio stream player 3D component.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct AudioStreamPlayer3DComponentData {
    /// Audio clip asset GUID.
    pub clip: Option<AssetId>,
    /// Output bus name.
    #[serde(default = "default_bus_name")]
    pub bus: String,
    /// Volume multiplier.
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Whether to loop.
    #[serde(default)]
    pub looping: bool,
    /// Whether to auto-play on scene start.
    #[serde(default)]
    pub play_on_start: bool,
    /// Blend between 2D (0.0) and 3D (1.0) spatial audio.
    #[serde(default)]
    pub spatial_blend: f32,
}

impl Default for AudioStreamPlayer3DComponentData {
    fn default() -> Self {
        Self {
            clip: None,
            bus: "SFX".to_string(),
            volume: 1.0,
            looping: false,
            play_on_start: false,
            spatial_blend: 1.0,
        }
    }
}

/// Versioned component payload used by scenes and prefabs.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", content = "data")]
pub enum ComponentData {
    /// Camera component.
    Camera(CameraComponentData),
    /// Mesh renderer component.
    MeshRenderer(MeshRendererComponentData),
    /// Light component.
    Light(LightComponentData),
    /// Rigidbody component.
    Rigidbody(RigidbodyComponentData),
    /// Collider component.
    Collider(ColliderComponentData),
    /// Audio source component.
    AudioSource(AudioSourceComponentData),
    /// Particle emitter component.
    ParticleEmitter(ParticleEmitterComponentData),
    /// Script proxy component.
    Script(ScriptComponentProxy),
    /// 2D sprite component.
    Sprite2D(Sprite2DComponentData),
    /// 2D tile map component.
    TileMap(TileMap2DComponentData),
    /// 2D camera component.
    Camera2D(Camera2DComponentData),
    /// 2D light component.
    Light2D(Light2DComponentData),
    /// 2D occluder component.
    Occluder2D(Occluder2DComponentData),
    /// Animation player component.
    AnimationPlayer(AnimationPlayerComponentData),
    /// Skinned mesh renderer component.
    SkinnedMeshRenderer(SkinnedMeshRendererComponentData),
    /// Audio stream player 2D component.
    AudioStreamPlayer2D(AudioStreamPlayer2DComponentData),
    /// Audio stream player 3D component.
    AudioStreamPlayer3D(AudioStreamPlayer3DComponentData),
}

impl ComponentData {
    /// Stable component type ID used by schema registries and serialized data.
    pub fn type_id(&self) -> &'static str {
        match self {
            Self::Camera(_) => "Camera",
            Self::MeshRenderer(_) => "MeshRenderer",
            Self::Light(_) => "Light",
            Self::Rigidbody(_) => "Rigidbody",
            Self::Collider(_) => "Collider",
            Self::AudioSource(_) => "AudioSource",
            Self::ParticleEmitter(_) => "ParticleEmitter",
            Self::Script(_) => "Script",
            Self::Sprite2D(_) => "Sprite2D",
            Self::TileMap(_) => "TileMap",
            Self::Camera2D(_) => "Camera2D",
            Self::Light2D(_) => "Light2D",
            Self::Occluder2D(_) => "Occluder2D",
            Self::AnimationPlayer(_) => "AnimationPlayer",
            Self::SkinnedMeshRenderer(_) => "SkinnedMeshRenderer",
            Self::AudioStreamPlayer2D(_) => "AudioStreamPlayer2D",
            Self::AudioStreamPlayer3D(_) => "AudioStreamPlayer3D",
        }
    }
}

/// Game object metadata.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct GameObject {
    /// Runtime-session stable object ID.
    pub id: EntityId,
    /// User-visible object name.
    pub name: String,
    /// User-visible tag.
    pub tag: String,
    /// Render/physics layer.
    pub layer: u32,
    /// Optional camera role.
    pub camera_role: Option<CameraRole>,
    /// Whether this object is active.
    pub active: bool,
    /// Optional script proxy records.
    #[serde(default)]
    pub scripts: Vec<ScriptComponentProxy>,
    /// Serializable non-transform components attached to this object.
    #[serde(default)]
    pub components: Vec<ComponentData>,
}

impl GameObject {
    /// Creates default metadata for a named object.
    pub fn new(id: EntityId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            tag: "Untagged".to_string(),
            layer: 0,
            camera_role: None,
            active: true,
            scripts: Vec::new(),
            components: Vec::new(),
        }
    }
}

/// Serializable game object record.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SerializedGameObject {
    /// Game object metadata.
    pub object: GameObject,
    /// Local transform.
    pub local_transform: Transform,
    /// Parent object ID.
    pub parent: Option<EntityId>,
    /// Sibling index under parent or roots.
    pub sibling_index: usize,
}

/// Scene file format.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SceneFile {
    /// Explicit schema version.
    pub version: u32,
    /// Human-readable scene name.
    pub name: String,
    /// Serialized objects.
    pub objects: Vec<SerializedGameObject>,
}

#[derive(Default)]
struct SceneState {
    world: World,
    transforms: TransformHierarchy,
    objects: HashMap<Entity, GameObject>,
    by_id: HashMap<EntityId, Entity>,
    id_allocator: ObjectIdAllocator,
    version: u64,
    pending_destroy: Vec<Entity>,
}

impl fmt::Debug for SceneState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SceneState")
            .field("objects", &self.objects.len())
            .field("version", &self.version)
            .field("pending_destroy", &self.pending_destroy)
            .finish()
    }
}

impl SceneState {
    fn spawn_object(&mut self, name: impl Into<String>) -> EngineResult<Entity> {
        let entity = self.world.spawn()?;
        let object = GameObject::new(self.id_allocator.allocate(), name);
        self.by_id.insert(object.id, entity);
        self.objects.insert(entity, object);
        self.transforms.set_local(entity, Transform::IDENTITY);
        self.transforms.set_parent(entity, None)?;
        self.bump_version();
        Ok(entity)
    }

    fn bump_version(&mut self) {
        self.version = self.version.saturating_add(1);
    }
}

/// Rust-native scene API with isolated edit and play states.
#[derive(Default)]
pub struct Scene {
    edit: SceneState,
    play: Option<SceneState>,
    mode: SceneMode,
}

impl fmt::Debug for Scene {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Scene")
            .field("mode", &self.mode)
            .field("edit", &self.edit)
            .field("play_active", &self.play.is_some())
            .finish()
    }
}

impl Default for SceneMode {
    fn default() -> Self {
        Self::Edit
    }
}

impl Scene {
    /// Creates an empty scene.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the active mode.
    pub fn mode(&self) -> SceneMode {
        self.mode
    }

    /// Returns the active scene structure version.
    pub fn structure_version(&self) -> u64 {
        self.active().version
    }

    /// Returns active transform hierarchy.
    pub fn transforms(&self) -> &TransformHierarchy {
        &self.active().transforms
    }

    /// Returns active transform hierarchy mutably.
    pub fn transforms_mut(&mut self) -> &mut TransformHierarchy {
        &mut self.active_mut().transforms
    }

    /// Returns active ECS world.
    pub fn world(&self) -> &World {
        &self.active().world
    }

    /// Returns active ECS world mutably.
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.active_mut().world
    }

    /// Creates a new object at the scene root.
    pub fn create_object(&mut self, name: impl Into<String>) -> EngineResult<Entity> {
        self.active_mut().spawn_object(name)
    }

    /// Returns immutable object metadata.
    pub fn object(&self, entity: Entity) -> Option<&GameObject> {
        self.active().objects.get(&entity)
    }

    /// Returns all active object entities and metadata in deterministic order.
    pub fn objects(&self) -> Vec<(Entity, &GameObject)> {
        let mut objects = self
            .active()
            .objects
            .iter()
            .map(|(entity, object)| (*entity, object))
            .collect::<Vec<_>>();
        objects.sort_by_key(|(_, object)| object.id.as_u128());
        objects
    }

    /// Iterates active object entities and metadata without allocating or sorting.
    pub fn iter_objects(&self) -> impl Iterator<Item = (Entity, &GameObject)> {
        self.active()
            .objects
            .iter()
            .map(|(entity, object)| (*entity, object))
    }

    /// Returns the number of active objects without allocating.
    pub fn object_count(&self) -> usize {
        self.active().objects.len()
    }

    /// Returns mutable object metadata and bumps scene version.
    pub fn object_mut(&mut self, entity: Entity) -> Option<&mut GameObject> {
        self.active_mut().bump_version();
        self.active_mut().objects.get_mut(&entity)
    }

    /// Returns serialized components attached to an object.
    pub fn components(&self, entity: Entity) -> Option<&[ComponentData]> {
        self.active()
            .objects
            .get(&entity)
            .map(|object| object.components.as_slice())
    }

    /// Replaces or inserts a serialized component by component type.
    pub fn upsert_component(
        &mut self,
        entity: Entity,
        component: ComponentData,
    ) -> EngineResult<()> {
        let state = self.active_mut();
        Self::ensure_alive(state, entity)?;
        let object = state
            .objects
            .get_mut(&entity)
            .ok_or_else(|| EngineError::invalid_handle("object metadata is missing"))?;
        let component_type = component.type_id();
        if let Some(existing) = object
            .components
            .iter_mut()
            .find(|candidate| candidate.type_id() == component_type)
        {
            *existing = component;
        } else {
            object.components.push(component);
        }
        state.bump_version();
        Ok(())
    }

    /// Removes a serialized component by stable component type ID.
    pub fn remove_component(&mut self, entity: Entity, component_type: &str) -> EngineResult<bool> {
        let state = self.active_mut();
        Self::ensure_alive(state, entity)?;
        let object = state
            .objects
            .get_mut(&entity)
            .ok_or_else(|| EngineError::invalid_handle("object metadata is missing"))?;
        let before = object.components.len();
        object
            .components
            .retain(|component| component.type_id() != component_type);
        let removed = before != object.components.len();
        if removed {
            state.bump_version();
        }
        Ok(removed)
    }

    /// Finds the first object by name.
    pub fn find_by_name(&self, name: &str) -> Option<Entity> {
        self.active()
            .objects
            .iter()
            .find_map(|(entity, object)| (object.name == name).then_some(*entity))
    }

    /// Finds an object by its stable scene object ID.
    pub fn find_by_id(&self, id: EntityId) -> Option<Entity> {
        self.active().by_id.get(&id).copied()
    }

    /// Finds all objects with a tag.
    pub fn find_by_tag(&self, tag: &str) -> Vec<Entity> {
        self.active()
            .objects
            .iter()
            .filter_map(|(entity, object)| (object.tag == tag).then_some(*entity))
            .collect()
    }

    /// Finds all objects on a layer.
    pub fn find_by_layer(&self, layer: u32) -> Vec<Entity> {
        self.active()
            .objects
            .iter()
            .filter_map(|(entity, object)| (object.layer == layer).then_some(*entity))
            .collect()
    }

    /// Returns the object marked as the main camera.
    pub fn main_camera(&self) -> Option<Entity> {
        self.find_camera(CameraRole::Main)
    }

    /// Returns the object marked as the game camera.
    pub fn game_camera(&self) -> Option<Entity> {
        self.find_camera(CameraRole::Game)
    }

    /// Sets an object's parent.
    pub fn set_parent(&mut self, child: Entity, parent: Option<Entity>) -> EngineResult<()> {
        let state = self.active_mut();
        Self::ensure_alive(state, child)?;
        if let Some(parent) = parent {
            Self::ensure_alive(state, parent)?;
        }
        state.transforms.set_parent(child, parent)?;
        state.bump_version();
        Ok(())
    }

    /// Defers object destruction until a frame-safe processing point.
    pub fn destroy_deferred(&mut self, entity: Entity) -> EngineResult<()> {
        let state = self.active_mut();
        Self::ensure_alive(state, entity)?;
        if !state.pending_destroy.contains(&entity) {
            state.pending_destroy.push(entity);
        }
        Ok(())
    }

    /// Processes pending destruction.
    pub fn process_deferred_destroy(&mut self) -> EngineResult<()> {
        let state = self.active_mut();
        let pending = std::mem::take(&mut state.pending_destroy);
        for entity in pending {
            if let Some(object) = state.objects.remove(&entity) {
                state.by_id.remove(&object.id);
            }
            state.transforms.remove(entity);
            if state.world.is_alive(entity) {
                state.world.despawn(entity)?;
            }
            state.bump_version();
        }
        Ok(())
    }

    /// Runs lifecycle hooks in the required stage order chosen by the caller.
    pub fn run_lifecycle(&mut self, stage: LifecycleStage) {
        self.active_mut().world.run_lifecycle(stage.into());
    }

    /// Runs a variable runtime frame in `Start`, `Update`, then `LateUpdate` order.
    pub fn tick_runtime_frame(&mut self) {
        self.run_lifecycle(LifecycleStage::Start);
        self.run_lifecycle(LifecycleStage::Update);
        self.tick_particles(1.0 / 60.0);
        self.run_lifecycle(LifecycleStage::LateUpdate);
    }

    /// Advances all particle emitters in the active scene by `delta_seconds`.
    pub fn tick_particles(&mut self, delta_seconds: f32) {
        let state = self.active_mut();
        for object in state.objects.values_mut() {
            for component in &mut object.components {
                if let ComponentData::ParticleEmitter(emitter) = component {
                    emitter.tick(delta_seconds);
                }
            }
        }
        state.bump_version();
    }

    /// Runs a fixed timestep lifecycle pass.
    pub fn tick_fixed_frame(&mut self) {
        self.run_lifecycle(LifecycleStage::FixedUpdate);
    }

    /// Runs an editor lifecycle pass.
    pub fn tick_editor_frame(&mut self) {
        self.run_lifecycle(LifecycleStage::EditorUpdate);
    }

    /// Clones object metadata and transform into a new object.
    pub fn clone_object(&mut self, source: Entity) -> EngineResult<Entity> {
        let state = self.active_mut();
        Self::ensure_alive(state, source)?;
        let source_object = state
            .objects
            .get(&source)
            .ok_or_else(|| EngineError::invalid_handle("source object is missing metadata"))?
            .clone();
        let new_entity = state.world.spawn()?;
        let mut cloned = source_object;
        cloned.id = state.id_allocator.allocate();
        cloned.name = format!("{} (Copy)", cloned.name);
        state.by_id.insert(cloned.id, new_entity);
        state.objects.insert(new_entity, cloned);
        state.transforms.set_local(
            new_entity,
            state.transforms.local(source).unwrap_or_default(),
        );
        state
            .transforms
            .set_parent(new_entity, state.transforms.parent(source))?;
        state.bump_version();
        Ok(new_entity)
    }

    /// Instantiates a prefab file into the active scene.
    pub fn instantiate_prefab(
        &mut self,
        prefab: &crate::schema::PrefabFile,
    ) -> EngineResult<Vec<Entity>> {
        self.load_objects_from_scene_file(&prefab.scene)
    }

    /// Enters play mode by cloning serializable edit-time state.
    pub fn enter_play_mode(&mut self) -> EngineResult<()> {
        if self.play.is_some() {
            self.mode = SceneMode::Play;
            return Ok(());
        }
        self.play = Some(Self::state_from_file(&self.to_scene_file("play-copy")?)?);
        self.mode = SceneMode::Play;
        Ok(())
    }

    /// Exits play mode and restores edit-time state as the active state.
    pub fn exit_play_mode(&mut self) {
        self.play = None;
        self.mode = SceneMode::Edit;
    }

    /// Converts active scene state to a serializable file.
    pub fn to_scene_file(&self, name: impl Into<String>) -> EngineResult<SceneFile> {
        let state = self.active();
        let mut objects = Vec::with_capacity(state.objects.len());
        for (entity, object) in &state.objects {
            let parent = state
                .transforms
                .parent(*entity)
                .and_then(|parent| state.objects.get(&parent))
                .map(|parent| parent.id);
            objects.push(SerializedGameObject {
                object: object.clone(),
                local_transform: state.transforms.local(*entity).unwrap_or_default(),
                parent,
                sibling_index: state.transforms.sibling_index(*entity).unwrap_or_default(),
            });
        }
        objects.sort_by_key(|object| (object.parent.map(EntityId::as_u128), object.sibling_index));
        Ok(SceneFile {
            version: SCENE_FILE_VERSION,
            name: name.into(),
            objects,
        })
    }

    /// Serializes active scene to pretty JSON.
    pub fn to_json(&self, name: impl Into<String>) -> EngineResult<String> {
        serde_json::to_string_pretty(&self.to_scene_file(name)?)
            .map_err(|error| EngineError::other(format!("scene serialization failed: {error}")))
    }

    /// Loads a scene from JSON.
    pub fn from_json(input: &str) -> EngineResult<Self> {
        let file = serde_json::from_str::<SceneFile>(input)
            .map_err(|error| EngineError::other(format!("scene parse failed: {error}")))?;
        Self::from_scene_file(file)
    }

    /// Loads a scene from a scene file structure.
    pub fn from_scene_file(file: SceneFile) -> EngineResult<Self> {
        Ok(Self {
            edit: Self::state_from_file(&file)?,
            play: None,
            mode: SceneMode::Edit,
        })
    }

    fn load_objects_from_scene_file(&mut self, file: &SceneFile) -> EngineResult<Vec<Entity>> {
        let state = self.active_mut();
        let mut source_to_entity = HashMap::new();
        let mut created = Vec::with_capacity(file.objects.len());

        for record in &file.objects {
            let entity = state.world.spawn()?;
            let mut object = record.object.clone();
            object.id = state.id_allocator.allocate();
            state.id_allocator.observe(object.id);
            state.by_id.insert(object.id, entity);
            state.objects.insert(entity, object);
            state.transforms.set_local(entity, record.local_transform);
            source_to_entity.insert(record.object.id, entity);
            created.push(entity);
        }

        for record in &file.objects {
            let entity = source_to_entity[&record.object.id];
            let parent = record
                .parent
                .and_then(|id| source_to_entity.get(&id).copied());
            state.transforms.set_parent(entity, parent)?;
        }
        state.bump_version();
        Ok(created)
    }

    fn state_from_file(file: &SceneFile) -> EngineResult<SceneState> {
        if file.version > SCENE_FILE_VERSION {
            return Err(EngineError::other(format!(
                "scene version {} is newer than supported version {}",
                file.version, SCENE_FILE_VERSION
            )));
        }
        let mut state = SceneState::default();
        let mut source_to_entity = HashMap::new();
        let mut records = file.objects.clone();
        records.sort_by_key(|record| (record.parent.map(EntityId::as_u128), record.sibling_index));

        for record in &records {
            let entity = state.world.spawn()?;
            state.id_allocator.observe(record.object.id);
            state.by_id.insert(record.object.id, entity);
            state.objects.insert(entity, record.object.clone());
            state.transforms.set_local(entity, record.local_transform);
            source_to_entity.insert(record.object.id, entity);
        }
        for record in &records {
            let entity = source_to_entity[&record.object.id];
            let parent = record
                .parent
                .and_then(|id| source_to_entity.get(&id).copied());
            state.transforms.set_parent(entity, parent)?;
        }
        Ok(state)
    }

    fn active(&self) -> &SceneState {
        match self.mode {
            SceneMode::Edit => &self.edit,
            SceneMode::Play => self.play.as_ref().unwrap_or(&self.edit),
        }
    }

    fn active_mut(&mut self) -> &mut SceneState {
        match self.mode {
            SceneMode::Edit => &mut self.edit,
            SceneMode::Play => self.play.as_mut().unwrap_or(&mut self.edit),
        }
    }

    fn ensure_alive(state: &SceneState, entity: Entity) -> EngineResult<()> {
        if state.world.is_alive(entity) {
            Ok(())
        } else {
            Err(EngineError::invalid_handle("scene object is not live"))
        }
    }

    fn find_camera(&self, role: CameraRole) -> Option<Entity> {
        self.active()
            .objects
            .iter()
            .find_map(|(entity, object)| (object.camera_role == Some(role)).then_some(*entity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::Any;

    #[test]
    fn saves_loads_and_saves_again() {
        let mut scene = Scene::new();
        let root = scene.create_object("Root").unwrap();
        let child = scene.create_object("Child").unwrap();
        scene.set_parent(child, Some(root)).unwrap();
        scene.object_mut(child).unwrap().tag = "Enemy".to_string();

        let first = scene.to_json("Example").unwrap();
        let loaded = Scene::from_json(&first).unwrap();
        let second = loaded.to_json("Example").unwrap();

        assert_eq!(first, second);
        assert_eq!(loaded.find_by_tag("Enemy").len(), 1);
    }

    #[test]
    fn serializes_regular_components() {
        let mut scene = Scene::new();
        let camera = scene.create_object("Camera").unwrap();
        scene
            .upsert_component(
                camera,
                ComponentData::Camera(CameraComponentData {
                    primary: true,
                    ..CameraComponentData::default()
                }),
            )
            .unwrap();

        let json = scene.to_json("Components").unwrap();
        let loaded = Scene::from_json(&json).unwrap();
        let camera = loaded.find_by_name("Camera").unwrap();

        assert!(matches!(
            loaded.components(camera).unwrap()[0],
            ComponentData::Camera(_)
        ));
    }

    #[test]
    fn deferred_destroy_happens_at_safe_point() {
        let mut scene = Scene::new();
        let entity = scene.create_object("Temp").unwrap();

        scene.destroy_deferred(entity).unwrap();
        assert!(scene.world().is_alive(entity));
        scene.process_deferred_destroy().unwrap();

        assert!(!scene.world().is_alive(entity));
    }

    #[test]
    fn play_mode_preserves_edit_time_data() {
        let mut scene = Scene::new();
        let entity = scene.create_object("Editable").unwrap();
        scene.enter_play_mode().unwrap();
        let play_entity = scene.find_by_name("Editable").unwrap();
        scene.object_mut(play_entity).unwrap().name = "Runtime".to_string();
        scene.exit_play_mode();

        assert_eq!(scene.object(entity).unwrap().name, "Editable");
        assert!(scene.find_by_name("Runtime").is_none());
    }

    struct LifecycleRecorder {
        stages: Vec<&'static str>,
    }

    impl crate::Component for LifecycleRecorder {
        fn start(&mut self) {
            self.stages.push("start");
        }

        fn update(&mut self) {
            self.stages.push("update");
        }

        fn fixed_update(&mut self) {
            self.stages.push("fixed_update");
        }

        fn late_update(&mut self) {
            self.stages.push("late_update");
        }

        fn editor_update(&mut self) {
            self.stages.push("editor_update");
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn lifecycle_ticks_in_expected_order() {
        let mut scene = Scene::new();
        let entity = scene.create_object("Actor").unwrap();
        scene
            .world_mut()
            .insert_component(entity, LifecycleRecorder { stages: Vec::new() })
            .unwrap();

        scene.tick_runtime_frame();
        scene.tick_fixed_frame();
        scene.tick_editor_frame();

        let stages = &scene
            .world_mut()
            .component_mut::<LifecycleRecorder>(entity)
            .unwrap()
            .stages;
        assert_eq!(
            stages,
            &[
                "start",
                "update",
                "late_update",
                "fixed_update",
                "editor_update"
            ]
        );
    }
}
