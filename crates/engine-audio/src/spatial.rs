//! Spatial audio attenuation and panning.

use engine_core::math::Vec3;
use serde::{Deserialize, Serialize};

/// Attenuation model for spatial audio.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AttenuationModel {
    /// No attenuation (full volume regardless of distance).
    None,
    /// Inverse distance attenuation: gain = 1 / (1 + distance).
    InverseDistance {
        /// Minimum distance for full volume.
        min_distance: f32,
        /// Maximum distance (beyond this, volume is zero).
        max_distance: f32,
    },
    /// Linear distance attenuation.
    LinearDistance {
        /// Minimum distance for full volume.
        min_distance: f32,
        /// Maximum distance (beyond this, volume is zero).
        max_distance: f32,
    },
    /// Logarithmic distance attenuation.
    LogarithmicDistance {
        /// Minimum distance for full volume.
        min_distance: f32,
        /// Maximum distance (beyond this, volume is zero).
        max_distance: f32,
    },
}

impl Default for AttenuationModel {
    fn default() -> Self {
        Self::InverseDistance {
            min_distance: 1.0,
            max_distance: 100.0,
        }
    }
}

/// Computes the volume attenuation for a given distance.
pub fn compute_attenuation(model: AttenuationModel, distance: f32) -> f32 {
    match model {
        AttenuationModel::None => 1.0,
        AttenuationModel::InverseDistance {
            min_distance,
            max_distance,
        } => {
            let clamped = distance.max(min_distance).min(max_distance);
            min_distance / (min_distance + clamped - min_distance)
        }
        AttenuationModel::LinearDistance {
            min_distance,
            max_distance,
        } => {
            if distance <= min_distance {
                1.0
            } else if distance >= max_distance {
                0.0
            } else {
                1.0 - (distance - min_distance) / (max_distance - min_distance)
            }
        }
        AttenuationModel::LogarithmicDistance {
            min_distance,
            max_distance,
        } => {
            if distance <= min_distance {
                1.0
            } else if distance >= max_distance {
                0.0
            } else {
                let ratio = distance / min_distance;
                1.0 / (1.0 + ratio.ln())
            }
        }
    }
}

/// Computes stereo pan values (left, right) based on source and listener positions.
pub fn compute_pan(
    source_pos: Vec3,
    listener_pos: Vec3,
    listener_forward: Vec3,
    listener_up: Vec3,
) -> (f32, f32) {
    let to_source = source_pos - listener_pos;
    let distance = to_source.length();
    if distance <= f32::EPSILON {
        return (0.5, 0.5);
    }

    let direction = to_source / distance;
    let right = listener_forward.cross(listener_up).normalized();
    let dot = direction.dot(right);

    let pan = (dot + 1.0) / 2.0;
    (1.0 - pan, pan)
}
