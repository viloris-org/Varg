#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Physics abstraction and null backend for the Varg engine.
//!
//! The null backend compiles everywhere and satisfies the trait contract without
//! linking any physics library. The `rapier` feature enables [`RapierPhysicsBackend`],
//! the production backend used by runtime-game builds.

mod backend;
mod character;
mod collision;
mod destruction;
mod filters;
mod fluid;
pub mod fracture;
pub mod joints;
mod layers;
mod material;
#[cfg(feature = "rapier")]
mod rapier_backend;
mod simple;
mod stats;
mod types;
pub mod vehicle;
mod world;

pub use crate::backend::{NullPhysicsBackend, PhysicsBackend};
pub use crate::character::{CharacterControllerDesc, CharacterControllerOutput, ColliderShapeRef};
pub use crate::collision::{
    CollisionChannel, CollisionProfile, CollisionProfileRegistry, CollisionResponse,
};
pub use crate::destruction::{
    DestructibleDesc, DestructibleHandle, DestructionDamage, DestructionEvent, DestructionFragment,
    DestructionWorld,
};
pub use crate::filters::{ContactFilter, ContactFilterChain};
pub use crate::fluid::{
    BuoyancyBodySample, BuoyancyProbeSet, FluidForce, FluidSurfaceModel, FluidVolumeDesc,
    FluidVolumeSample, collider_displacement_volume, solve_probe_buoyancy, solve_volume_buoyancy,
};
pub use crate::fracture::{
    FractureConfig, FractureFragment, FracturePattern, FractureSystem, FragmentTracker,
};
pub use crate::joints::{JointDesc, JointHandle, JointLimits, JointMotor, JointState, JointType};
pub use crate::layers::{
    LAYER_DEFAULT, LAYER_ENEMY, LAYER_PLAYER, LAYER_PROJECTILE, LAYER_TRIGGER, LayerMatrix,
};
pub use crate::material::{PhysicalMaterial, PhysicalMaterialRegistry, built_in_physical_material};
#[cfg(feature = "rapier")]
pub use crate::rapier_backend::RapierPhysicsBackend;
pub use crate::simple::SimplePhysicsBackend;
pub use crate::stats::PhysicsStats;
pub use crate::types::{
    BodyHandle, BodyKind, CcdMode, ColliderDesc, ColliderHandle, ColliderShape, CombineMode,
    ContactEvent, OverlapResult, QueryFilter, RayHit, RayHitAll, RigidbodyDesc, SleepParams,
    SweepHit,
};
pub use crate::vehicle::{
    VehicleDesc, VehicleHandle, VehicleInput, VehicleState, VehicleTuning, WheelDesc,
};
pub use crate::world::PhysicsWorld;
pub use engine_core::math::{Quat, Transform, Vec3};

#[cfg(test)]
mod tests;
