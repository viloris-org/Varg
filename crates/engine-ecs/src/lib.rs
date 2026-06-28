#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Atomic ECS and base scene storage.

mod object_store;
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
    AudioZoneComponentData, BuoyancyProbeSetComponentData, Camera2DComponentData,
    CameraComponentData, CameraRole, ColliderComponentData, ComponentData, DirectionalShadowMode,
    EnvironmentComponentData, FluidVolumeComponentData, GameObject, LifecycleStage,
    Light2DComponentData, LightBakeMode, LightComponentData, LightKind, MaterialRef,
    MeshRendererComponentData, ObjectIdAllocator, Occluder2DComponentData,
    ParticleEmitterComponentData, RigidbodyComponentData, SCENE_FILE_VERSION, Scene, SceneFile,
    SceneMode, ScriptComponent, SerializedGameObject, SkinnedMeshRendererComponentData,
    SkyboxComponentData, Sprite2DComponentData, TileMap2DComponentData, WindZoneComponentData,
};
pub use schema::{
    BuildConfiguration, BuildRenderSettings, ComponentFieldKind, ComponentFieldSchema,
    ComponentSchema, ComponentSchemaRegistry, EditorPreferences, FormatDiagnostic, FormatVersion,
    PROJECT_MANIFEST_FILE_NAME, PrefabFile, ProjectManifest, SchemaEvolution,
    project_manifest_path,
};
pub use transform::TransformHierarchy;
pub use world::{Component, ComponentStorage, Entity, World};

#[cfg(feature = "physics")]
pub use physics::{ColliderComponent, RigidbodyComponent};
