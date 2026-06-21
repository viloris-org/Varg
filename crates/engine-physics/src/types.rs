use engine_core::math::{Transform, Vec3};
use serde::{Deserialize, Serialize};

// ── Primitive types ──────────────────────────────────────────────────────────

/// Opaque handle to a physics body.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub struct BodyHandle(pub u64);

/// Opaque handle to a collider.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub struct ColliderHandle(pub u64);

/// Physics body kind.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    /// Fully simulated body.
    #[default]
    Dynamic,
    /// Moved by the user, pushes dynamic bodies.
    Kinematic,
    /// Never moves.
    Static,
}

/// Rigidbody creation parameters.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct RigidbodyDesc {
    /// Initial world-space transform.
    pub transform: Transform,
    /// Body kind.
    pub kind: BodyKind,
    /// Linear damping coefficient.
    pub linear_damping: f32,
    /// Angular damping coefficient.
    pub angular_damping: f32,
    /// Gravity scale multiplier.
    pub gravity_scale: f32,
    /// CCD mode for this body.
    pub ccd: CcdMode,
    /// Optional sleep thresholds; `None` uses engine defaults.
    pub sleep_params: Option<SleepParams>,
}

impl Default for RigidbodyDesc {
    fn default() -> Self {
        Self {
            transform: Transform::IDENTITY,
            kind: BodyKind::Dynamic,
            linear_damping: 0.0,
            angular_damping: 0.0,
            gravity_scale: 1.0,
            ccd: CcdMode::Disabled,
            sleep_params: None,
        }
    }
}

// ── Collider shapes ──────────────────────────────────────────────────────────

/// Collider shape.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ColliderShape {
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
    /// Convex mesh approximation (vertex soup).
    Mesh {
        /// Flat list of vertex positions (x,y,z triplets).
        vertices: Vec<f32>,
    },
    /// Triangle mesh (static bodies only).
    TriMesh {
        /// Flat list of vertex positions (x,y,z triplets).
        vertices: Vec<f32>,
        /// Triangle indices (three per triangle, u32).
        indices: Vec<u32>,
    },
    /// Heightfield terrain collider (static only).
    Heightfield {
        /// Number of samples along the X axis (width).
        num_x: u32,
        /// Number of samples along the Z axis (depth).
        num_z: u32,
        /// Height samples in row-major order (num_x * num_z values).
        heights: Vec<f32>,
        /// World-space scale of the heightfield.
        scale: Vec3,
    },
}

/// CCD (Continuous Collision Detection) mode.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CcdMode {
    /// CCD disabled (default).
    #[default]
    Disabled,
    /// CCD enabled — prevents fast-moving objects from tunneling.
    Enabled,
}

/// Sleep thresholds for a rigidbody.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct SleepParams {
    /// Linear velocity threshold for sleeping.
    pub linear_threshold: f32,
    /// Angular velocity threshold for sleeping.
    pub angular_threshold: f32,
    /// Time (in seconds) the body must be below thresholds before sleeping.
    pub time_before_sleep: f32,
}

impl Default for SleepParams {
    fn default() -> Self {
        Self {
            linear_threshold: 0.01,
            angular_threshold: 0.01,
            time_before_sleep: 1.0,
        }
    }
}

/// Friction/restitution combine mode.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CombineMode {
    /// Average of the two values (default).
    #[default]
    Average,
    /// Minimum of the two values.
    Min,
    /// Product of the two values.
    Multiply,
    /// Maximum of the two values.
    Max,
}

/// Collider creation parameters.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ColliderDesc {
    /// Shape.
    pub shape: ColliderShape,
    /// Friction coefficient.
    pub friction: f32,
    /// Restitution (bounciness) coefficient.
    pub restitution: f32,
    /// When true the collider fires overlap events instead of resolving contacts.
    pub is_trigger: bool,
    /// Collision layer this collider belongs to.
    pub layer: u32,
    /// Bitmask of layers this collider collides with.
    pub mask: u32,
    /// How friction values from two bodies are combined.
    pub friction_combine: CombineMode,
    /// How restitution values from two bodies are combined.
    pub restitution_combine: CombineMode,
    /// Whether to report contact force events for this collider.
    pub active_contact_events: bool,
}

impl Default for ColliderDesc {
    fn default() -> Self {
        Self {
            shape: ColliderShape::Box {
                half_extents: Vec3::new(0.5, 0.5, 0.5),
            },
            friction: 0.5,
            restitution: 0.0,
            is_trigger: false,
            layer: 1,
            mask: !0,
            friction_combine: CombineMode::Average,
            restitution_combine: CombineMode::Average,
            active_contact_events: false,
        }
    }
}

// ── Contact callbacks ────────────────────────────────────────────────────────

/// A contact event between two bodies.
#[derive(Clone, Debug, PartialEq)]
pub struct ContactEvent {
    /// First body.
    pub body_a: BodyHandle,
    /// Second body.
    pub body_b: BodyHandle,
    /// First collider.
    pub collider_a: ColliderHandle,
    /// Second collider.
    pub collider_b: ColliderHandle,
    /// Contact point in world space.
    pub point: Vec3,
    /// Contact normal pointing from B toward A.
    pub normal: Vec3,
    /// Whether this is an enter (true) or exit (false) event.
    pub entered: bool,
    /// Whether at least one collider in the pair is a trigger/sensor.
    pub is_trigger: bool,
    /// Contact points in world space (multiple points for complex contacts).
    pub contact_points: Vec<Vec3>,
}

// ── Query types ──────────────────────────────────────────────────────────────

/// A single raycast hit.
#[derive(Clone, Debug, PartialEq)]
pub struct RayHit {
    /// Hit body.
    pub body: BodyHandle,
    /// Hit collider.
    pub collider: ColliderHandle,
    /// Hit point in world space.
    pub point: Vec3,
    /// Surface normal at the hit point.
    pub normal: Vec3,
    /// Distance from ray origin.
    pub distance: f32,
}

/// A single overlap result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverlapResult {
    /// Overlapping body.
    pub body: BodyHandle,
    /// Overlapping collider.
    pub collider: ColliderHandle,
}

/// Query filter controlling which layers are tested.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QueryFilter {
    /// Layer mask; zero means test all layers.
    pub mask: u32,
}

// ── Multi-hit query results ──────────────────────────────────────────────────

/// Result of a raycast that returns all hits along the ray.
#[derive(Clone, Debug, PartialEq)]
pub struct RayHitAll {
    /// All hits sorted by distance (nearest first).
    pub hits: Vec<RayHit>,
}

/// Result of a box sweep query.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepHit {
    /// Hit body.
    pub body: BodyHandle,
    /// Hit collider.
    pub collider: ColliderHandle,
    /// Hit point in world space.
    pub point: Vec3,
    /// Surface normal at the hit point.
    pub normal: Vec3,
    /// Distance traveled before the hit.
    pub distance: f32,
}
