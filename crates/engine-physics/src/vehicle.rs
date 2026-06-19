//! Wheeled vehicle system built on raycast suspension.

use engine_core::math::{Transform, Vec3};
use serde::{Deserialize, Serialize};

use crate::BodyHandle;

/// Opaque handle to a vehicle.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub struct VehicleHandle(pub u64);

/// Wheel definition attached to a vehicle chassis.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct WheelDesc {
    /// Chassis-relative attachment point.
    pub chassis_connection: Vec3,
    /// Wheel center offset from the attachment (typically downward).
    pub center_offset: Vec3,
    /// Wheel radius in meters.
    pub radius: f32,
    /// Suspension rest length.
    pub suspension_rest: f32,
    /// Suspension maximum travel.
    pub suspension_travel: f32,
    /// Suspension stiffness in N/m.
    pub suspension_stiffness: f32,
    /// Suspension damping in N·s/m.
    pub suspension_damping: f32,
    /// Maximum suspension force in N.
    pub max_suspension_force: f32,
    /// Lateral friction stiffness (corrective force on side-slip).
    pub lateral_friction_stiffness: f32,
    /// Longitudinal friction stiffness (drive/brake force).
    pub longitudinal_friction_stiffness: f32,
    /// Whether this wheel can be steered.
    pub is_steered: bool,
    /// Whether this wheel is powered by the engine.
    pub is_powered: bool,
    /// Whether this wheel is connected to the brakes.
    pub is_braked: bool,
}

impl Default for WheelDesc {
    fn default() -> Self {
        Self {
            chassis_connection: Vec3::new(0.0, -0.3, 0.0),
            center_offset: Vec3::new(0.0, -0.3, 0.0),
            radius: 0.3,
            suspension_rest: 0.2,
            suspension_travel: 0.1,
            suspension_stiffness: 80_000.0,
            suspension_damping: 3_500.0,
            max_suspension_force: 100_000.0,
            lateral_friction_stiffness: 100.0,
            longitudinal_friction_stiffness: 100.0,
            is_steered: true,
            is_powered: true,
            is_braked: true,
        }
    }
}

/// Vehicle-wide tuning parameters.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct VehicleTuning {
    /// Mass fraction of the chassis used for vehicle inertia (0..1).
    pub chassis_mass_fraction: f32,
    /// Maximum steering angle in radians.
    pub max_steering_angle: f32,
    /// Worn-down wheel radius for the controller.
    pub wheel_radius: f32,
}

impl Default for VehicleTuning {
    fn default() -> Self {
        Self {
            chassis_mass_fraction: 0.8,
            max_steering_angle: 0.6,
            wheel_radius: 0.3,
        }
    }
}

/// Vehicle creation descriptor.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct VehicleDesc {
    /// Chassis rigidbody handle.
    pub chassis: BodyHandle,
    /// Per-wheel definitions (typically 4 for a car).
    pub wheels: Vec<WheelDesc>,
    /// Vehicle-wide tuning.
    pub tuning: VehicleTuning,
}

/// Per-step vehicle input (digital, like a gamepad or keyboard).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VehicleInput {
    /// Forward acceleration (0..1).
    pub throttle: f32,
    /// Brake force (0..1).
    pub brake: f32,
    /// Steering angle fraction (-1..1). Scaled by max_steering_angle.
    pub steering: f32,
    /// Handbrake toggle.
    pub handbrake: bool,
}

/// Runtime state returned after each vehicle step.
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleState {
    /// Current speed magnitude in m/s.
    pub speed: f32,
    /// Current gear direction: -1 reverse, 0 neutral, 1 forward.
    pub gear: i8,
    /// Per-wheel world-space transforms.
    pub wheel_transforms: Vec<Transform>,
    /// Per-wheel suspension displacement (0=rest, positive=compressed).
    pub suspension_displacements: Vec<f32>,
    /// Whether the vehicle is grounded.
    pub grounded: bool,
}
