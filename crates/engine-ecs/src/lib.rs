#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Atomic ECS and base scene storage.

pub mod particle;
pub mod scene;
pub mod schema;
pub mod transform;
pub mod world;

#[cfg(feature = "physics")]
pub mod physics;

#[cfg(feature = "audio")]
pub mod audio;

pub use particle::{ParticleInstance, ParticleSystem};
pub use scene::{
    AcousticGeometryComponentData, AcousticMaterialComponentData, AcousticPortalComponentData,
    AcousticRoomComponentData, AnimationPlayerComponentData, AudioListenerComponentData,
    AudioSourceComponentData, AudioStreamPlayer2DComponentData, AudioStreamPlayer3DComponentData,
    AudioZoneComponentData, Camera2DComponentData, CameraComponentData, CameraRole,
    ColliderComponentData, ComponentData, GameObject, LifecycleStage, Light2DComponentData,
    LightComponentData, MaterialRef, MeshRendererComponentData, ObjectIdAllocator,
    Occluder2DComponentData, ParticleEmitterComponentData, RigidbodyComponentData, Scene,
    SceneFile, SceneMode, ScriptComponentProxy, SkinnedMeshRendererComponentData,
    SkyboxComponentData, Sprite2DComponentData, TileMap2DComponentData,
};
pub use schema::{
    BuildConfiguration, ComponentFieldKind, ComponentFieldSchema, ComponentSchema,
    ComponentSchemaRegistry, EditorPreferences, FormatDiagnostic, FormatVersion, PrefabFile,
    ProjectManifest, SchemaEvolution,
};
pub use transform::TransformHierarchy;
pub use world::{Component, ComponentStorage, Entity, World};

#[cfg(feature = "physics")]
pub use physics::{ColliderComponent, RigidbodyComponent};
