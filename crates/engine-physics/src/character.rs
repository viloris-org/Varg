use crate::{QueryFilter, Vec3};

/// Kinematic character movement request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharacterControllerDesc {
    /// Character collider shape used for sweeps.
    pub shape: ColliderShapeRef,
    /// Desired world-space translation for this step.
    pub translation: Vec3,
    /// Step time in seconds.
    pub dt: f32,
    /// Layers considered solid for the controller.
    pub filter: QueryFilter,
}

/// Borrowable collider shape for query/controller APIs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColliderShapeRef {
    /// Axis-aligned box.
    Box {
        /// Half-extents on each axis.
        half_extents: Vec3,
    },
    /// Sphere.
    Sphere {
        /// Radius.
        radius: f32,
    },
    /// Capsule aligned along the Y axis.
    Capsule {
        /// Half-height of the cylindrical section.
        half_height: f32,
        /// Radius of the end caps.
        radius: f32,
    },
}

impl Default for ColliderShapeRef {
    fn default() -> Self {
        Self::Capsule {
            half_height: 0.5,
            radius: 0.25,
        }
    }
}

/// Result of a kinematic character movement.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CharacterControllerOutput {
    /// Effective translation applied after collisions/sliding.
    pub translation: Vec3,
    /// True when the controller ended grounded.
    pub grounded: bool,
    /// Number of collision callbacks observed during the move.
    pub collisions: usize,
}
