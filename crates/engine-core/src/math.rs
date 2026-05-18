//! Public math types.

/// 3D vector with single-precision components.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Vec3 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
    /// Z component.
    pub z: f32,
}

impl Vec3 {
    /// Zero vector.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    /// One vector.
    pub const ONE: Self = Self::new(1.0, 1.0, 1.0);

    /// Creates a vector.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

/// Quaternion in `(x, y, z, w)` order.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Quat {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
    /// Z component.
    pub z: f32,
    /// W component.
    pub w: f32,
}

impl Quat {
    /// Identity rotation.
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };
}

impl Default for Quat {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Translation, rotation, and scale transform.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Transform {
    /// Position in parent space.
    pub translation: Vec3,
    /// Orientation in parent space.
    pub rotation: Quat,
    /// Scale in parent space.
    pub scale: Vec3,
}

impl Transform {
    /// Identity transform.
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}
