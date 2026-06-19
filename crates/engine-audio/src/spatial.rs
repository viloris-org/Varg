//! Spatial audio attenuation and panning.

use engine_core::math::Vec3;
use serde::{Deserialize, Serialize};

use crate::AudioSourceShape;

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

/// Smooth Hermite interpolation between `edge0` and `edge1`.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Computes the directivity gain for a source shape and listener direction.
///
/// Returns a value in `[0.0, 1.0]` where `1.0` means the listener is fully inside
/// the main lobe and `0.0` (or `outer_gain`) means they are outside the cone.
pub fn compute_directivity(
    shape: AudioSourceShape,
    source_forward: Vec3,
    source_to_listener: Vec3,
) -> f32 {
    match shape {
        AudioSourceShape::Cone {
            inner_angle_degrees,
            outer_angle_degrees,
            outer_gain,
        } => {
            let distance = source_to_listener.length();
            if distance <= f32::EPSILON {
                return 1.0;
            }
            let direction = source_to_listener / distance;
            let source_forward = source_forward.normalized();
            let cos_angle = source_forward.dot(direction);
            let angle = cos_angle.acos().to_degrees();
            if angle <= inner_angle_degrees {
                1.0
            } else if angle >= outer_angle_degrees {
                outer_gain.clamp(0.0, 1.0)
            } else {
                let t = smoothstep(inner_angle_degrees, outer_angle_degrees, angle);
                1.0 - t * (1.0 - outer_gain.clamp(0.0, 1.0))
            }
        }
        AudioSourceShape::Sphere { .. } | AudioSourceShape::Point => 1.0,
    }
}

/// Computes the effective source-listener distance for attenuation.
///
/// Spherical sources start attenuating from their surface; point sources use
/// the raw distance.
pub fn compute_effective_distance(shape: AudioSourceShape, distance: f32) -> f32 {
    match shape {
        AudioSourceShape::Sphere { radius } => (distance - radius.max(0.0)).max(0.0),
        _ => distance.max(0.0),
    }
}

/// Computes the Doppler pitch rate for a moving source and listener.
///
/// The returned value is clamped to `[0.5, 2.0]` to avoid excessive pitch shift.
pub fn compute_doppler_rate(
    source_velocity: Vec3,
    listener_velocity: Vec3,
    source_to_listener: Vec3,
    speed_of_sound: f32,
    doppler_scale: f32,
) -> f32 {
    let distance = source_to_listener.length();
    if distance <= f32::EPSILON || speed_of_sound <= f32::EPSILON {
        return 1.0;
    }
    let direction = source_to_listener / distance;
    let relative_speed = (source_velocity - listener_velocity).dot(direction);
    let rate = 1.0 + relative_speed * doppler_scale.max(0.0) / speed_of_sound;
    rate.clamp(0.5, 2.0)
}

/// Computes a spread-aware equal-power stereo pan.
///
/// `spread` in `[0.0, 1.0]` controls how wide the source is rendered:
/// `0.0` collapses to mono and `1.0` gives full directional panning.
pub fn compute_spread_pan(
    source_pos: Vec3,
    listener_pos: Vec3,
    listener_forward: Vec3,
    listener_up: Vec3,
    spread: f32,
) -> (f32, f32) {
    let (_, base_pan) = compute_pan(source_pos, listener_pos, listener_forward, listener_up);
    let spread = spread.clamp(0.0, 1.0).max(0.01);
    let p = base_pan.powf(spread);
    ((1.0 - p).sqrt(), p.sqrt())
}
