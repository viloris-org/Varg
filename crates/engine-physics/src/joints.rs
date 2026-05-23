//! Physics joints: constraints connecting two rigid bodies.

use engine_core::math::Vec3;
use serde::{Deserialize, Serialize};

/// Opaque handle to a physics joint.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub struct JointHandle(pub u64);

/// Motor configuration for powered joints.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct JointMotor {
    /// Target velocity for the motor.
    pub target_velocity: f32,
    /// Maximum force the motor can apply.
    pub max_force: f32,
    /// Whether the motor is enabled.
    pub enabled: bool,
}

impl Default for JointMotor {
    fn default() -> Self {
        Self {
            target_velocity: 0.0,
            max_force: f32::MAX,
            enabled: false,
        }
    }
}

/// Angular or linear limits for a joint axis.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct JointLimits {
    /// Minimum angle/position.
    pub min: f32,
    /// Maximum angle/position.
    pub max: f32,
    /// Whether a spring force pushes back at the limit boundary.
    pub spring_enabled: bool,
    /// Spring stiffness.
    pub stiffness: f32,
    /// Spring damping.
    pub damping: f32,
}

impl Default for JointLimits {
    fn default() -> Self {
        Self {
            min: -90.0_f32.to_radians(),
            max: 90.0_f32.to_radians(),
            spring_enabled: false,
            stiffness: 0.0,
            damping: 0.0,
        }
    }
}

/// Joint type determining the constraint behavior.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum JointType {
    /// Fixed relative pose between two bodies (point-to-point).
    Pin {
        /// Anchor point in body A's local space.
        anchor_a: Vec3,
        /// Anchor point in body B's local space.
        anchor_b: Vec3,
    },
    /// Single axis rotation (like a door hinge).
    Hinge {
        /// Anchor point in body A's local space.
        anchor_a: Vec3,
        /// Anchor point in body B's local space.
        anchor_b: Vec3,
        /// Hinge axis in body A's local space.
        axis_a: Vec3,
        /// Angular limits.
        limits: JointLimits,
        /// Optional motor.
        motor: Option<JointMotor>,
    },
    /// Single axis translation (like a piston).
    Slider {
        /// Anchor point in body A's local space.
        anchor_a: Vec3,
        /// Anchor point in body B's local space.
        anchor_b: Vec3,
        /// Slider axis in body A's local space.
        axis_a: Vec3,
        /// Distance limits.
        limits: JointLimits,
        /// Optional motor.
        motor: Option<JointMotor>,
    },
    /// Distance constraint with spring behavior.
    SpringArm {
        /// Anchor point in body A's local space.
        anchor_a: Vec3,
        /// Anchor point in body B's local space.
        anchor_b: Vec3,
        /// Rest length of the spring.
        rest_length: f32,
        /// Spring stiffness.
        stiffness: f32,
        /// Spring damping.
        damping: f32,
    },
    /// Swing + twist limits (shoulder/hip socket).
    ConeTwist {
        /// Anchor point in body A's local space.
        anchor_a: Vec3,
        /// Anchor point in body B's local space.
        anchor_b: Vec3,
        /// Twist axis in body A's local space.
        twist_axis_a: Vec3,
        /// Swing limits.
        swing_limits: JointLimits,
        /// Twist limits.
        twist_limits: JointLimits,
    },
    /// Full six-degree-of-freedom constraint.
    Generic6DOF {
        /// Anchor point in body A's local space.
        anchor_a: Vec3,
        /// Anchor point in body B's local space.
        anchor_b: Vec3,
        /// Linear limits for X, Y, Z axes.
        linear_limits: [JointLimits; 3],
        /// Angular limits for X, Y, Z axes.
        angular_limits: [JointLimits; 3],
        /// Optional motors per DOF.
        motors: [Option<JointMotor>; 6],
    },
}

/// Creation descriptor for a joint.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct JointDesc {
    /// Joint type and parameters.
    pub joint_type: JointType,
    /// First connected body.
    pub body_a: super::BodyHandle,
    /// Second connected body.
    pub body_b: super::BodyHandle,
    /// Maximum force before joint breaks (0 = never).
    pub break_force: f32,
    /// Maximum torque before joint breaks (0 = never).
    pub break_torque: f32,
}

/// Runtime state of a joint.
#[derive(Clone, Debug, PartialEq)]
pub struct JointState {
    /// Joint handle.
    pub handle: JointHandle,
    /// Joint descriptor.
    pub desc: JointDesc,
}

impl JointDesc {
    /// Creates a pin joint between two bodies at shared anchor points.
    pub fn pin(
        body_a: super::BodyHandle,
        body_b: super::BodyHandle,
        anchor_a: Vec3,
        anchor_b: Vec3,
    ) -> Self {
        Self {
            joint_type: JointType::Pin { anchor_a, anchor_b },
            body_a,
            body_b,
            break_force: 0.0,
            break_torque: 0.0,
        }
    }

    /// Creates a hinge joint.
    pub fn hinge(
        body_a: super::BodyHandle,
        body_b: super::BodyHandle,
        anchor_a: Vec3,
        anchor_b: Vec3,
        axis_a: Vec3,
    ) -> Self {
        Self {
            joint_type: JointType::Hinge {
                anchor_a,
                anchor_b,
                axis_a,
                limits: JointLimits::default(),
                motor: None,
            },
            body_a,
            body_b,
            break_force: 0.0,
            break_torque: 0.0,
        }
    }

    /// Creates a spring arm joint.
    pub fn spring_arm(
        body_a: super::BodyHandle,
        body_b: super::BodyHandle,
        anchor_a: Vec3,
        anchor_b: Vec3,
        rest_length: f32,
        stiffness: f32,
        damping: f32,
    ) -> Self {
        Self {
            joint_type: JointType::SpringArm {
                anchor_a,
                anchor_b,
                rest_length,
                stiffness,
                damping,
            },
            body_a,
            body_b,
            break_force: 0.0,
            break_torque: 0.0,
        }
    }
}
