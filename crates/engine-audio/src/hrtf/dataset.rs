//! Synthesizable, license-free HRTF dataset.

use std::f32::consts::PI;

use engine_core::math::Vec3;

/// Abstraction over an HRTF dataset.
///
/// Implementations provide a pair of FIR filters (left ear and right ear) for a
/// source direction expressed in listener-local coordinates: `x` right, `y` up,
/// `z` forward.
pub trait HrtfDataset: Send + Sync {
    /// Sample rate the filters were designed for.
    fn sample_rate(&self) -> u32;
    /// Length of every FIR filter in samples.
    fn filter_taps(&self) -> usize;
    /// Returns left and right FIR coefficients for the given direction.
    fn filter_for_direction(&self, direction: Vec3) -> (Vec<f32>, Vec<f32>);
}

/// Synthesizable HRTF dataset based on a spherical-head ITD/ILD model.
///
/// No proprietary measurements are embedded. The filters are generated on demand
/// from the source direction, so the dataset can be swapped for a real HRTF set
/// later without changing the convolver.
#[derive(Clone, Debug)]
pub struct SyntheticHrtfDataset {
    sample_rate: u32,
    head_radius: f32,
    filter_taps: usize,
}

impl SyntheticHrtfDataset {
    /// Creates a new synthetic dataset.
    ///
    /// `head_radius` is in world units (default 0.0875 m).
    pub fn new(sample_rate: u32, head_radius: f32, filter_taps: usize) -> Self {
        Self {
            sample_rate,
            head_radius,
            filter_taps: filter_taps.max(4),
        }
    }
}

impl HrtfDataset for SyntheticHrtfDataset {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn filter_taps(&self) -> usize {
        self.filter_taps
    }

    fn filter_for_direction(&self, direction: Vec3) -> (Vec<f32>, Vec<f32>) {
        let taps = self.filter_taps;
        let mut left = vec![0.0_f32; taps];
        let mut right = vec![0.0_f32; taps];

        let local = direction.normalized();
        if local.length_squared() <= f32::EPSILON {
            // Centered source: equal, centered response.
            let center = (taps / 2) as f32;
            insert_peak(&mut left, center, 0.5_f32.sqrt());
            insert_peak(&mut right, center, 0.5_f32.sqrt());
            return (left, right);
        }

        // Azimuth in [-pi, pi], positive to the right.
        let azimuth = local.x.atan2(local.z);
        let elevation = local.y.clamp(-1.0, 1.0).asin();

        // Woodworth spherical-head ITD model, clamped to the audible side range.
        let abs_azimuth = azimuth.abs().min(PI / 2.0);
        let itd_seconds = (self.head_radius / speed_of_sound()) * (abs_azimuth + abs_azimuth.sin());
        let itd_samples = itd_seconds * self.sample_rate as f32;

        // Positive signed_itd means the left ear receives sound earlier.
        let signed_itd = -azimuth.signum() * itd_samples;

        let base_delay = (taps / 2) as f32;
        let left_delay = base_delay - signed_itd * 0.5;
        let right_delay = base_delay + signed_itd * 0.5;

        // Pan derived from the lateral component, with equal-power panning.
        let pan = ((local.x + 1.0) * 0.5).clamp(0.0, 1.0);
        let left_gain = (1.0 - pan).sqrt();
        let right_gain = pan.sqrt();

        // Simple rear attenuation: sources behind the listener are slightly
        // quieter and more diffuse than frontal sources.
        let rear_gain = 0.7 + 0.3 * local.z.clamp(-1.0, 1.0);

        // A tiny elevation cue: shift a small amount of energy one sample early
        // or late depending on whether the source is above or below the horizon.
        let elevation_shift = (elevation / (PI / 2.0)).clamp(-1.0, 1.0) * 0.15;

        insert_peak(&mut left, left_delay, left_gain * rear_gain.sqrt());
        insert_peak(&mut right, right_delay, right_gain * rear_gain.sqrt());

        // Add a diffuse elevation component to smooth vertical perception.
        let diffuse_gain = elevation_shift.abs();
        if diffuse_gain > f32::EPSILON {
            let shift = if elevation_shift > 0.0 { 1.0 } else { -1.0 };
            let diffuse_delay = (base_delay + shift).clamp(0.0, (taps - 2) as f32);
            insert_peak(&mut left, diffuse_delay, left_gain * diffuse_gain * 0.3);
            insert_peak(&mut right, diffuse_delay, right_gain * diffuse_gain * 0.3);
        }

        (left, right)
    }
}

fn speed_of_sound() -> f32 {
    343.0
}

/// Inserts a fractional-delay energy peak into `filter` at `delay`.
///
/// Energy is split between `delay.floor()` and the next tap using linear
/// interpolation so that motion produces smooth filter changes.
fn insert_peak(filter: &mut [f32], delay: f32, gain: f32) {
    let taps = filter.len();
    if taps == 0 {
        return;
    }
    let index = delay.floor();
    let frac = delay - index;
    let i0 = index as usize;
    let i1 = i0 + 1;
    if i0 < taps {
        filter[i0] += gain * (1.0 - frac);
    }
    if i1 < taps {
        filter[i1] += gain * frac;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dataset() -> SyntheticHrtfDataset {
        SyntheticHrtfDataset::new(48_000, 0.0875, 64)
    }

    #[test]
    fn filter_lengths_match_quality() {
        let ds = dataset();
        let (left, right) = ds.filter_for_direction(Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(left.len(), 64);
        assert_eq!(right.len(), 64);
    }

    #[test]
    fn front_source_has_balanced_ears() {
        let ds = dataset();
        let (left, right) = ds.filter_for_direction(Vec3::new(0.0, 0.0, 1.0));
        let left_energy: f32 = left.iter().map(|v| v * v).sum();
        let right_energy: f32 = right.iter().map(|v| v * v).sum();
        let ratio = left_energy / right_energy.max(f32::EPSILON);
        assert!(ratio > 0.9 && ratio < 1.1);
    }

    #[test]
    fn left_source_emphasizes_left_ear() {
        let ds = dataset();
        let (left, right) = ds.filter_for_direction(Vec3::new(-1.0, 0.0, 0.0));
        let left_energy: f32 = left.iter().map(|v| v * v).sum();
        let right_energy: f32 = right.iter().map(|v| v * v).sum();
        assert!(left_energy > right_energy * 2.0);
    }

    #[test]
    fn right_source_emphasizes_right_ear() {
        let ds = dataset();
        let (left, right) = ds.filter_for_direction(Vec3::new(1.0, 0.0, 0.0));
        let left_energy: f32 = left.iter().map(|v| v * v).sum();
        let right_energy: f32 = right.iter().map(|v| v * v).sum();
        assert!(right_energy > left_energy * 2.0);
    }
}
