//! 2D camera component with parallax, zoom, and drag margins.

use engine_core::math::Vec3;
use serde::{Deserialize, Serialize};

/// Anchor point for 2D camera positioning.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum Anchor2D {
    /// Center of the screen.
    #[default]
    Center,
    /// Top-left corner.
    TopLeft,
    /// Top-right corner.
    TopRight,
    /// Bottom-left corner.
    BottomLeft,
    /// Bottom-right corner.
    BottomRight,
}

/// A parallax layer that scrolls at a different rate.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParallaxLayer {
    /// Scroll scale relative to camera movement.
    pub scale: f32,
}

impl Default for ParallaxLayer {
    fn default() -> Self {
        Self { scale: 1.0 }
    }
}

/// Serializable 2D camera component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera2DComponentData {
    /// Zoom level.
    #[serde(default = "default_zoom")]
    pub zoom: f32,
    /// Parallax layer scales.
    #[serde(default)]
    pub parallax_layers: Vec<ParallaxLayer>,
    /// Anchor point.
    #[serde(default)]
    pub anchor: Anchor2D,
    /// RGB clear color.
    #[serde(default = "default_clear_color")]
    pub clear_color: Vec3,
}

fn default_zoom() -> f32 {
    1.0
}

fn default_clear_color() -> Vec3 {
    Vec3::new(0.1, 0.1, 0.1)
}

impl Default for Camera2DComponentData {
    fn default() -> Self {
        Self {
            zoom: default_zoom(),
            parallax_layers: vec![ParallaxLayer::default()],
            anchor: Anchor2D::default(),
            clear_color: default_clear_color(),
        }
    }
}
