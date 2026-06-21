use engine_core::{EngineError, EngineResult};

use crate::{
    BodyHandle, CharacterControllerDesc, CharacterControllerOutput, ColliderDesc, ColliderHandle,
    ContactEvent, JointDesc, JointHandle, JointLimits, JointMotor, JointState, OverlapResult,
    PhysicsStats, Quat, QueryFilter, RayHit, RayHitAll, RigidbodyDesc, SweepHit, Transform, Vec3,
    VehicleDesc, VehicleHandle, VehicleInput, VehicleState,
};

/// Pluggable physics backend contract.
///
/// Implementations are expected to step the simulation on `fixed_update` and
/// synchronise body transforms with the ECS on `sync_transforms`.
pub trait PhysicsBackend: Send + Sync {
    /// Advances the simulation by `dt` seconds.
    fn fixed_update(&mut self, dt: f32);

    /// Creates a rigidbody and returns its handle.
    fn create_body(&mut self, desc: &RigidbodyDesc) -> EngineResult<BodyHandle>;

    /// Destroys a body and all attached colliders.
    fn destroy_body(&mut self, body: BodyHandle) -> EngineResult<()>;

    /// Attaches a collider to a body.
    fn add_collider(
        &mut self,
        body: BodyHandle,
        desc: &ColliderDesc,
    ) -> EngineResult<ColliderHandle>;

    /// Removes a collider.
    fn remove_collider(&mut self, collider: ColliderHandle) -> EngineResult<()>;

    /// Returns the current world-space transform of a body.
    fn body_transform(&self, body: BodyHandle) -> EngineResult<Transform>;

    /// Teleports a body to a new world-space transform.
    fn set_body_transform(&mut self, body: BodyHandle, transform: Transform) -> EngineResult<()>;

    /// Applies a linear impulse to a body.
    fn apply_impulse(&mut self, body: BodyHandle, impulse: Vec3) -> EngineResult<()>;

    /// Casts a ray and returns the closest hit, if any.
    fn raycast(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        filter: QueryFilter,
    ) -> Option<RayHit>;

    /// Returns all colliders overlapping a sphere.
    fn overlap_sphere(&self, center: Vec3, radius: f32, filter: QueryFilter) -> Vec<OverlapResult>;

    /// Sweeps a sphere along a direction and returns the first hit, if any.
    fn sweep_sphere(
        &self,
        center: Vec3,
        radius: f32,
        direction: Vec3,
        max_distance: f32,
        filter: QueryFilter,
    ) -> Option<RayHit>;

    /// Casts a ray and returns all hits along the ray, sorted by distance.
    fn raycast_all(
        &self,
        _origin: Vec3,
        _direction: Vec3,
        _max_distance: f32,
        _filter: QueryFilter,
    ) -> RayHitAll {
        RayHitAll { hits: Vec::new() }
    }

    /// Sweeps a box along a direction and returns the first hit, if any.
    fn sweep_box(
        &self,
        _center: Vec3,
        _half_extents: Vec3,
        _rotation: Quat,
        _direction: Vec3,
        _max_distance: f32,
        _filter: QueryFilter,
    ) -> Option<SweepHit> {
        None
    }

    /// Sweeps a capsule along a direction and returns the first hit, if any.
    fn sweep_capsule(
        &self,
        _center: Vec3,
        _half_height: f32,
        _radius: f32,
        _rotation: Quat,
        _direction: Vec3,
        _max_distance: f32,
        _filter: QueryFilter,
    ) -> Option<SweepHit> {
        None
    }

    /// Returns all colliders overlapping a box.
    fn overlap_box(
        &self,
        _center: Vec3,
        _half_extents: Vec3,
        _rotation: Quat,
        _filter: QueryFilter,
    ) -> Vec<OverlapResult> {
        Vec::new()
    }

    /// Returns all colliders overlapping a capsule.
    fn overlap_capsule(
        &self,
        _center: Vec3,
        _half_height: f32,
        _radius: f32,
        _rotation: Quat,
        _filter: QueryFilter,
    ) -> Vec<OverlapResult> {
        Vec::new()
    }

    /// Drains pending contact events since the last call.
    fn drain_contacts(&mut self) -> Vec<ContactEvent>;

    /// Moves a kinematic body with a basic character controller.
    fn move_character(
        &mut self,
        body: BodyHandle,
        desc: CharacterControllerDesc,
    ) -> EngineResult<CharacterControllerOutput>;

    /// Applies a continuous force to a body (accumulated and applied each step).
    fn apply_force(&mut self, _body: BodyHandle, _force: Vec3) -> EngineResult<()> {
        Err(EngineError::UnsupportedCapability {
            capability: "continuous forces",
        })
    }

    /// Applies a continuous torque to a body.
    fn apply_torque(&mut self, _body: BodyHandle, _torque: Vec3) -> EngineResult<()> {
        Err(EngineError::UnsupportedCapability {
            capability: "continuous torque",
        })
    }

    /// Clears accumulated forces on a body.
    fn clear_forces(&mut self, _body: BodyHandle) -> EngineResult<()> {
        Ok(())
    }

    /// Puts a body to sleep or wakes it up.
    fn set_body_sleep(&mut self, _body: BodyHandle, _sleeping: bool) -> EngineResult<()> {
        Ok(())
    }

    /// Returns whether a body is sleeping.
    fn is_body_sleeping(&self, _body: BodyHandle) -> EngineResult<bool> {
        Ok(false)
    }

    /// Creates a joint between two bodies.
    fn create_joint(&mut self, _desc: &JointDesc) -> EngineResult<JointHandle> {
        Err(EngineError::UnsupportedCapability {
            capability: "joints",
        })
    }

    /// Destroys a joint.
    fn destroy_joint(&mut self, _joint: JointHandle) -> EngineResult<()> {
        Ok(())
    }

    /// Returns the state of a joint.
    fn joint_state(&self, _joint: JointHandle) -> EngineResult<JointState> {
        Err(EngineError::UnsupportedCapability {
            capability: "joints",
        })
    }

    /// Sets the motor on a joint.
    fn set_joint_motor(&mut self, _joint: JointHandle, _motor: JointMotor) -> EngineResult<()> {
        Ok(())
    }

    /// Sets the limits on a joint.
    fn set_joint_limits(&mut self, _joint: JointHandle, _limits: JointLimits) -> EngineResult<()> {
        Ok(())
    }

    /// Returns the reaction force and torque magnitude on a joint.
    fn joint_forces(&self, _joint: JointHandle) -> EngineResult<(f32, f32)> {
        Ok((0.0, 0.0))
    }

    /// Creates a wheeled vehicle attached to a chassis body.
    fn create_vehicle(&mut self, _desc: &VehicleDesc) -> EngineResult<VehicleHandle> {
        Err(EngineError::UnsupportedCapability {
            capability: "vehicles",
        })
    }

    /// Destroys a vehicle.
    fn destroy_vehicle(&mut self, _vehicle: VehicleHandle) -> EngineResult<()> {
        Err(EngineError::UnsupportedCapability {
            capability: "vehicles",
        })
    }

    /// Updates vehicle inputs and returns current state.
    fn update_vehicle(
        &mut self,
        _vehicle: VehicleHandle,
        _input: VehicleInput,
    ) -> EngineResult<VehicleState> {
        Err(EngineError::UnsupportedCapability {
            capability: "vehicles",
        })
    }

    /// Returns profiling statistics from the last `fixed_update`.
    fn stats(&self) -> PhysicsStats {
        PhysicsStats::default()
    }
}

// ── Null backend ─────────────────────────────────────────────────────────────

/// No-op physics backend. Compiles everywhere; produces no simulation.
#[derive(Default)]
pub struct NullPhysicsBackend;

impl PhysicsBackend for NullPhysicsBackend {
    fn fixed_update(&mut self, _dt: f32) {}

    fn create_body(&mut self, _desc: &RigidbodyDesc) -> EngineResult<BodyHandle> {
        Err(EngineError::other("null physics backend"))
    }

    fn destroy_body(&mut self, _body: BodyHandle) -> EngineResult<()> {
        Ok(())
    }

    fn add_collider(
        &mut self,
        _body: BodyHandle,
        _desc: &ColliderDesc,
    ) -> EngineResult<ColliderHandle> {
        Err(EngineError::other("null physics backend"))
    }

    fn remove_collider(&mut self, _collider: ColliderHandle) -> EngineResult<()> {
        Ok(())
    }

    fn body_transform(&self, _body: BodyHandle) -> EngineResult<Transform> {
        Err(EngineError::other("null physics backend"))
    }

    fn set_body_transform(&mut self, _body: BodyHandle, _transform: Transform) -> EngineResult<()> {
        Ok(())
    }

    fn apply_impulse(&mut self, _body: BodyHandle, _impulse: Vec3) -> EngineResult<()> {
        Ok(())
    }

    fn raycast(
        &self,
        _origin: Vec3,
        _direction: Vec3,
        _max_distance: f32,
        _filter: QueryFilter,
    ) -> Option<RayHit> {
        None
    }

    fn overlap_sphere(
        &self,
        _center: Vec3,
        _radius: f32,
        _filter: QueryFilter,
    ) -> Vec<OverlapResult> {
        Vec::new()
    }

    fn sweep_sphere(
        &self,
        _center: Vec3,
        _radius: f32,
        _direction: Vec3,
        _max_distance: f32,
        _filter: QueryFilter,
    ) -> Option<RayHit> {
        None
    }

    fn drain_contacts(&mut self) -> Vec<ContactEvent> {
        Vec::new()
    }

    fn move_character(
        &mut self,
        _body: BodyHandle,
        _desc: CharacterControllerDesc,
    ) -> EngineResult<CharacterControllerOutput> {
        Err(EngineError::other("null physics backend"))
    }
}

// ── Contact filter chain ─────────────────────────────────────────────────────
