//! Time-domain FIR convolver with per-block filter interpolation.

use std::collections::VecDeque;

use engine_core::math::Vec3;

use crate::hrtf::dataset::HrtfDataset;

/// Per-source HRTF convolver.
///
/// Maintains a delay line and interpolates filter coefficients from the
/// previous block's filter to the current block's filter. This keeps motion
/// continuous even when the source direction changes.
#[derive(Clone, Debug)]
pub struct HrtfConvolver {
    delay: VecDeque<f32>,
    prev_left: Vec<f32>,
    prev_right: Vec<f32>,
    target_left: Vec<f32>,
    target_right: Vec<f32>,
    block_frames: usize,
    frame_index: usize,
}

impl HrtfConvolver {
    /// Creates a convolver with the given filter length.
    pub fn new(filter_taps: usize) -> Self {
        let taps = filter_taps.max(1);
        Self {
            delay: VecDeque::with_capacity(taps),
            prev_left: Vec::new(),
            prev_right: Vec::new(),
            target_left: Vec::new(),
            target_right: Vec::new(),
            block_frames: 1,
            frame_index: 0,
        }
    }

    /// Prepares the convolver for a new output block.
    ///
    /// `direction` is in listener-local coordinates: `x` right, `y` up,
    /// `z` forward.
    pub fn prepare_block(
        &mut self,
        dataset: &dyn HrtfDataset,
        direction: Vec3,
        block_frames: usize,
    ) {
        self.block_frames = block_frames.max(1);
        self.frame_index = 0;
        let (left, right) = dataset.filter_for_direction(direction);
        self.target_left = left;
        self.target_right = right;
        if self.prev_left.is_empty() {
            self.prev_left = self.target_left.clone();
            self.prev_right = self.target_right.clone();
        }
    }

    /// Processes a single mono sample and returns the left/right output.
    pub fn process_sample(&mut self, sample: f32) -> (f32, f32) {
        self.delay.push_back(sample);
        let taps = self.target_left.len().max(self.target_right.len());
        while self.delay.len() > taps {
            self.delay.pop_front();
        }

        let t = self.frame_index as f32 / self.block_frames as f32;
        let left = convolve(&self.delay, &self.prev_left, &self.target_left, t);
        let right = convolve(&self.delay, &self.prev_right, &self.target_right, t);

        self.frame_index += 1;
        if self.frame_index >= self.block_frames {
            // Commit the current target as the starting point for the next block.
            self.prev_left.clone_from(&self.target_left);
            self.prev_right.clone_from(&self.target_right);
        }

        (left, right)
    }

    /// Resets internal state without deallocating capacity.
    pub fn reset(&mut self) {
        self.delay.clear();
        self.prev_left.clear();
        self.prev_right.clear();
        self.target_left.clear();
        self.target_right.clear();
        self.frame_index = 0;
    }
}

fn convolve(delay: &VecDeque<f32>, prev: &[f32], target: &[f32], t: f32) -> f32 {
    let len = delay.len();
    let taps = prev.len().min(target.len()).min(len);
    if taps == 0 {
        return 0.0;
    }
    let mut sum = 0.0_f32;
    for k in 0..taps {
        let sample = delay[len - 1 - k];
        let coeff = prev[k] + (target[k] - prev[k]) * t;
        sum += sample * coeff;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hrtf::dataset::SyntheticHrtfDataset;

    fn dataset() -> SyntheticHrtfDataset {
        SyntheticHrtfDataset::new(48_000, 0.0875, 64)
    }

    #[test]
    fn convolver_produces_finite_output() {
        let ds = dataset();
        let mut convolver = HrtfConvolver::new(ds.filter_taps());
        convolver.prepare_block(&ds, Vec3::new(1.0, 0.0, 0.0), 128);
        for _ in 0..256 {
            let (l, r) = convolver.process_sample(0.5);
            assert!(l.is_finite());
            assert!(r.is_finite());
        }
    }

    #[test]
    fn moving_source_does_not_create_discontinuities() {
        let ds = dataset();
        let mut convolver = HrtfConvolver::new(ds.filter_taps());
        convolver.prepare_block(&ds, Vec3::new(0.0, 0.0, 1.0), 128);
        let mut last_left = 0.0_f32;
        let mut max_jump = 0.0_f32;
        for i in 0..128 {
            let angle = (i as f32 / 128.0) * std::f32::consts::PI;
            let dir = Vec3::new(angle.sin(), 0.0, angle.cos());
            convolver.prepare_block(&ds, dir, 1);
            let (l, _) = convolver.process_sample(1.0);
            max_jump = max_jump.max((l - last_left).abs());
            last_left = l;
        }
        assert!(max_jump < 0.5);
    }
}
