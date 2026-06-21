use std::fmt;

use engine_core::EngineResult;

use crate::{
    ContactEvent, ContactFilterChain, JointDesc, JointHandle, JointLimits, JointMotor, LayerMatrix,
    NullPhysicsBackend, PhysicsBackend, PhysicsStats, VehicleDesc, VehicleHandle, VehicleInput,
    VehicleState,
};

/// Physics world that owns a backend, layer matrix, and contact filter chain.
pub struct PhysicsWorld {
    backend: Box<dyn PhysicsBackend>,
    /// Layer collision matrix.
    pub layer_matrix: LayerMatrix,
    /// Runtime contact filter chain.
    pub contact_filter_chain: ContactFilterChain,
}

impl fmt::Debug for PhysicsWorld {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PhysicsWorld")
            .field("layer_matrix", &self.layer_matrix)
            .finish_non_exhaustive()
    }
}

impl PhysicsWorld {
    /// Creates a physics world with the given backend.
    pub fn new(backend: impl PhysicsBackend + 'static) -> Self {
        Self {
            backend: Box::new(backend),
            layer_matrix: LayerMatrix::default(),
            contact_filter_chain: ContactFilterChain::default(),
        }
    }

    /// Creates a physics world backed by the null backend.
    pub fn null() -> Self {
        Self::new(NullPhysicsBackend)
    }

    /// Steps the simulation.
    pub fn fixed_update(&mut self, dt: f32) {
        self.backend.fixed_update(dt);
    }

    /// Delegates to the backend.
    pub fn backend_mut(&mut self) -> &mut dyn PhysicsBackend {
        self.backend.as_mut()
    }

    /// Delegates to the backend (read-only).
    pub fn backend(&self) -> &dyn PhysicsBackend {
        self.backend.as_ref()
    }

    /// Creates a joint between two bodies.
    pub fn create_joint(&mut self, desc: &JointDesc) -> EngineResult<JointHandle> {
        self.backend.create_joint(desc)
    }

    /// Destroys a joint.
    pub fn destroy_joint(&mut self, joint: JointHandle) -> EngineResult<()> {
        self.backend.destroy_joint(joint)
    }

    /// Sets a joint motor.
    pub fn set_joint_motor(&mut self, joint: JointHandle, motor: JointMotor) -> EngineResult<()> {
        self.backend.set_joint_motor(joint, motor)
    }

    /// Sets joint limits.
    pub fn set_joint_limits(
        &mut self,
        joint: JointHandle,
        limits: JointLimits,
    ) -> EngineResult<()> {
        self.backend.set_joint_limits(joint, limits)
    }

    /// Returns joint reaction forces.
    pub fn joint_forces(&self, joint: JointHandle) -> EngineResult<(f32, f32)> {
        self.backend.joint_forces(joint)
    }

    /// Creates a wheeled vehicle attached to an existing chassis body.
    pub fn create_vehicle(&mut self, desc: &VehicleDesc) -> EngineResult<VehicleHandle> {
        self.backend.create_vehicle(desc)
    }

    /// Destroys a wheeled vehicle.
    pub fn destroy_vehicle(&mut self, vehicle: VehicleHandle) -> EngineResult<()> {
        self.backend.destroy_vehicle(vehicle)
    }

    /// Updates vehicle input and returns the latest runtime vehicle state.
    pub fn update_vehicle(
        &mut self,
        vehicle: VehicleHandle,
        input: VehicleInput,
    ) -> EngineResult<VehicleState> {
        self.backend.update_vehicle(vehicle, input)
    }

    /// Drains contact events from the backend, filtering through the contact filter chain.
    pub fn drain_contacts(&mut self) -> Vec<ContactEvent> {
        let chain = &self.contact_filter_chain;
        self.backend
            .drain_contacts()
            .into_iter()
            .filter(|event| chain.should_process(event.body_a, event.body_b))
            .collect()
    }

    /// Returns profiling statistics from the most recent `fixed_update`.
    pub fn stats(&self) -> PhysicsStats {
        self.backend.stats()
    }
}

// ── Simple deterministic backend ─────────────────────────────────────────────
