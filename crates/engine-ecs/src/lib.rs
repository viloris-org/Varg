#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Atomic ECS and base scene storage.

pub mod scene;
pub mod schema;
pub mod transform;
pub mod world;

pub use scene::{
    CameraRole, GameObject, LifecycleStage, ObjectIdAllocator, Scene, SceneFile, SceneMode,
    ScriptComponentProxy,
};
pub use schema::{
    BuildConfiguration, EditorPreferences, FormatDiagnostic, FormatVersion, PrefabFile,
    ProjectManifest, SchemaEvolution,
};
pub use transform::TransformHierarchy;
pub use world::{Component, ComponentStorage, Entity, World};
