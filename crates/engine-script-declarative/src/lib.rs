#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Complete declarative game engine interface optimized for AI code generation.
//!
//! This crate provides six declarative systems for describing entire games
//! using JSON/YAML, inspired by three.js but designed specifically for
//! AI agents to generate reliably.
//!
//! # Design Philosophy
//!
//! - **Declarative over imperative**: Describe "what" not "how"
//! - **Three.js-inspired**: Familiar scene graph and object model
//! - **Type-safe**: JSON Schema validation for LLM outputs
//! - **Pattern-based**: Consistent structures LLMs can reliably generate
//!
//! # Complete System (6 declarative layers)
//!
//! 1. **Behavior Trees** - Game logic and AI
//! 2. **Scene Graphs** - three.js-like 3D object hierarchy
//! 3. **UI Layouts** - Declarative user interfaces
//! 4. **Systems Config** - Game systems (combat, economy, etc.)
//! 5. **Asset Manifest** - Resource management
//! 6. **Project Structure** - Top-level project configuration

mod action;
mod behavior;
mod compiler;
mod condition;
pub mod presets;
mod runtime;
mod schema;

// New declarative systems (three.js-inspired)
mod assets;
mod project;
mod scene;
mod systems;
mod ui;

// Behavior system (original)
pub use action::{Action, ActionContext, ActionExpr, ExecutionState};
pub use behavior::{BehaviorNode, BehaviorTree, NodeResult};
pub use compiler::BehaviorCompiler;
pub use condition::{Condition, ConditionContext, ConditionExpr, FloatComparison};
pub use runtime::DeclarativeScriptBackend;
pub use schema::{generate_json_schema, BehaviorSchema, EntityBehaviorConfig};

// Scene system (three.js-like)
pub use scene::{
    CameraType, Environment, FogConfig, GeometryRef, LightType, MaterialRef, Object3D, SceneSchema,
};

// UI system
pub use ui::{
    Anchor, BarStyle, ButtonStyle, ContainerStyle, DataBinding, InputStyle, LayoutType, Position,
    TextStyle, UIElement, UISchema,
};

// Systems configuration
pub use systems::{
    AudioSystem, CombatSystem, EconomySystem, PhysicsSystem, ProgressionSystem, StatsPerLevel,
    SystemParam, SystemsConfig, XPCurve,
};

// Asset management
pub use assets::{
    AssetManifest, AssetMeta, AssetRef, LoadingStrategy, PrefabSchema, PrefabVariant,
    ProceduralConfig, ProceduralParam,
};

// Project structure
pub use project::{
    BuildConfig, BuildSettings, OptimizationLevel, Platform, ProjectSchema, SceneRef, UIRef,
};

/// Re-export key types for convenience.
pub mod prelude {
    // Behavior system
    pub use crate::{
        Action, ActionContext, ActionExpr, BehaviorCompiler, BehaviorNode, BehaviorSchema,
        BehaviorTree, Condition, ConditionContext, ConditionExpr, DeclarativeScriptBackend,
        EntityBehaviorConfig, ExecutionState, FloatComparison, NodeResult,
    };

    // Scene system
    pub use crate::{
        CameraType, Environment, FogConfig, GeometryRef, LightType, MaterialRef, Object3D,
        SceneSchema,
    };

    // UI system
    pub use crate::{
        Anchor, BarStyle, ButtonStyle, DataBinding, LayoutType, Position, TextStyle, UIElement,
        UISchema,
    };

    // Systems
    pub use crate::{CombatSystem, EconomySystem, SystemsConfig};

    // Assets
    pub use crate::{AssetManifest, AssetRef, LoadingStrategy};

    // Project
    pub use crate::{ProjectSchema, SceneRef};
}
