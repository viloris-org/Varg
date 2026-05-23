//! Audio stream player component data for 2D and 3D spatial audio sources.

use engine_core::AssetId;
use serde::{Deserialize, Serialize};

use crate::spatial::AttenuationModel;

/// Serializable 2D audio stream player component.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct AudioStreamPlayer2DComponentData {
    /// Audio clip asset GUID.
    pub clip: Option<AssetId>,
    /// Output bus name.
    #[serde(default = "default_bus")]
    pub bus: String,
    /// Volume multiplier.
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Whether to loop.
    #[serde(default)]
    pub looping: bool,
    /// Whether to auto-play on scene start.
    #[serde(default)]
    pub play_on_start: bool,
}

fn default_bus() -> String {
    "SFX".to_string()
}

fn default_volume() -> f32 {
    1.0
}

impl Default for AudioStreamPlayer2DComponentData {
    fn default() -> Self {
        Self {
            clip: None,
            bus: default_bus(),
            volume: default_volume(),
            looping: false,
            play_on_start: false,
        }
    }
}

/// Serializable 3D audio stream player component.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct AudioStreamPlayer3DComponentData {
    /// Audio clip asset GUID.
    pub clip: Option<AssetId>,
    /// Output bus name.
    #[serde(default = "default_bus")]
    pub bus: String,
    /// Volume multiplier.
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Whether to loop.
    #[serde(default)]
    pub looping: bool,
    /// Whether to auto-play on scene start.
    #[serde(default)]
    pub play_on_start: bool,
    /// Blend between 2D (0.0) and 3D (1.0) spatial audio.
    #[serde(default)]
    pub spatial_blend: f32,
    /// Attenuation model for distance-based volume.
    #[serde(default)]
    pub attenuation: AttenuationModel,
}

impl Default for AudioStreamPlayer3DComponentData {
    fn default() -> Self {
        Self {
            clip: None,
            bus: default_bus(),
            volume: default_volume(),
            looping: false,
            play_on_start: false,
            spatial_blend: 1.0,
            attenuation: AttenuationModel::default(),
        }
    }
}
