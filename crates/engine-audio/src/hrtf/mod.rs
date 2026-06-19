//! Head-related transfer function (HRTF) support for binaural rendering.

use std::sync::Arc;

use engine_core::math::Vec3;

pub mod convolver;
pub mod dataset;

pub use crate::hrtf::convolver::HrtfConvolver;
pub use crate::hrtf::dataset::{HrtfDataset, SyntheticHrtfDataset};

/// Creates a default synthesizable HRTF dataset for the given sample rate and quality.
pub fn default_dataset(
    sample_rate: u32,
    head_radius: f32,
    quality: crate::HrtfQuality,
) -> Arc<dyn HrtfDataset> {
    Arc::new(SyntheticHrtfDataset::new(
        sample_rate,
        head_radius,
        quality.filter_taps(),
    ))
}

/// Transforms a world-space source direction into listener-local coordinates.
///
/// The returned vector has `x` pointing right, `y` up, and `z` forward relative
/// to the listener. It is normalized, or zero if the source is at the listener.
pub fn listener_local_direction(
    source_position: Vec3,
    listener_position: Vec3,
    listener_forward: Vec3,
    listener_up: Vec3,
) -> Vec3 {
    let to_source = source_position - listener_position;
    let distance = to_source.length();
    if distance <= f32::EPSILON {
        return Vec3::ZERO;
    }
    let forward = listener_forward.normalized();
    let up = listener_up.normalized();
    let right = forward.cross(up).normalized();
    let local = Vec3::new(
        to_source.dot(right) / distance,
        to_source.dot(up) / distance,
        to_source.dot(forward) / distance,
    );
    local.normalized()
}
