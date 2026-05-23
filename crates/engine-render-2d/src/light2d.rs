//! 2D light and occluder components.

use engine_core::math::Vec3;
use serde::{Deserialize, Serialize};

/// Kind of 2D light source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Light2DKind {
    /// Point light radiating in all directions.
    #[default]
    Point,
    /// Spot light with direction and angle.
    Spot {
        /// Direction in degrees.
        direction: f32,
        /// Cone angle in degrees.
        angle: f32,
    },
    /// Ambient light affecting the entire scene.
    Ambient,
}

/// Serializable 2D light component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Light2DComponentData {
    /// Light color (RGB).
    pub color: Vec3,
    /// Light intensity.
    #[serde(default = "default_intensity")]
    pub intensity: f32,
    /// Light range in world units.
    #[serde(default = "default_range")]
    pub range: f32,
    /// Light kind.
    #[serde(default)]
    pub kind: Light2DKind,
    /// Whether shadows are enabled.
    #[serde(default)]
    pub shadows: bool,
}

fn default_intensity() -> f32 {
    1.0
}

fn default_range() -> f32 {
    10.0
}

impl Default for Light2DComponentData {
    fn default() -> Self {
        Self {
            color: Vec3::ONE,
            intensity: default_intensity(),
            range: default_range(),
            kind: Light2DKind::Point,
            shadows: false,
        }
    }
}

/// Serializable 2D occluder component for shadow casting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Occluder2DComponentData {
    /// Occluder polygon vertices in local space.
    #[serde(default)]
    pub polygon: Vec<[f32; 2]>,
    /// Whether the occluder is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Occluder2DComponentData {
    fn default() -> Self {
        Self {
            polygon: vec![[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]],
            enabled: default_true(),
        }
    }
}
