//! Universal stereo/speaker panner.

use crate::render::{SpatialRenderContext, SpatialRenderer, SpatialVoiceParams};
use crate::spatial::{
    compute_attenuation, compute_directivity, compute_effective_distance, compute_spread_pan,
};
use crate::SpatialMode;

/// Stateless stereo panner that applies distance attenuation, directivity,
/// spread-based panning, and optional rear attenuation.
#[derive(Clone, Copy, Debug, Default)]
pub struct StereoRenderer {
    listener: crate::AudioListenerDesc,
}

impl StereoRenderer {
    /// Creates a new stereo renderer.
    pub fn new() -> Self {
        Self::default()
    }

    fn spatial_gain(&self, params: &SpatialVoiceParams) -> f32 {
        let to_listener = self.listener.position - params.position;
        let distance = to_listener.length();
        let effective_distance = compute_effective_distance(params.shape, distance);
        let attenuation = if params.spatial_mode == SpatialMode::Direct {
            1.0
        } else {
            compute_attenuation(params.attenuation, effective_distance)
        };
        let directivity = if params.spatial_mode == SpatialMode::Direct {
            1.0
        } else {
            compute_directivity(params.shape, params.forward, -to_listener)
        };
        params.gain * attenuation * directivity
    }
}

impl SpatialRenderer for StereoRenderer {
    fn reset(&mut self) {
        self.listener = crate::AudioListenerDesc::default();
    }

    fn begin_block(&mut self, context: &SpatialRenderContext) {
        self.listener = context.listener;
    }

    fn render_sample(&mut self, params: &SpatialVoiceParams, sample: f32) -> (f32, f32) {
        let gain = self.spatial_gain(params);
        let (left_pan, right_pan) = if params.spatial_mode == SpatialMode::Direct {
            (1.0, 1.0)
        } else {
            compute_spread_pan(
                params.position,
                self.listener.position,
                self.listener.forward,
                self.listener.up,
                params.spread,
            )
        };
        (sample * gain * left_pan, sample * gain * right_pan)
    }
}
