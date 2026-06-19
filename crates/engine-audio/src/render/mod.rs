//! Spatial renderers used by the object-audio mixer.

use engine_core::math::Vec3;

use crate::{AttenuationModel, AudioListenerDesc, AudioSourceShape, SourceHandle, SpatialMode};

pub mod hrtf_renderer;
pub mod stereo_renderer;

pub use crate::render::hrtf_renderer::HrtfRenderer;
pub use crate::render::stereo_renderer::StereoRenderer;

/// Context shared with a spatial renderer for one output block.
#[derive(Clone, Copy, Debug)]
pub struct SpatialRenderContext {
    /// Listener descriptor for this block.
    pub listener: AudioListenerDesc,
    /// Output sample rate.
    pub sample_rate: u32,
    /// Number of output channels.
    pub channels: u16,
    /// Number of frames in the current block.
    pub block_frames: usize,
}

/// Per-sample source parameters passed to a spatial renderer.
#[derive(Clone, Copy, Debug)]
pub struct SpatialVoiceParams {
    /// Source handle.
    pub handle: SourceHandle,
    /// World-space source position.
    pub position: Vec3,
    /// Unit forward vector for directivity.
    pub forward: Vec3,
    /// Geometric source approximation.
    pub shape: AudioSourceShape,
    /// Distance attenuation model.
    pub attenuation: AttenuationModel,
    /// Spatial rendering mode.
    pub spatial_mode: SpatialMode,
    /// Spatial spread in `[0.0, 1.0]`.
    pub spread: f32,
    /// Base gain (volume * bus gain) before spatial attenuation/directivity.
    pub gain: f32,
}

/// Trait implemented by stereo panner and HRTF binaural renderer.
pub trait SpatialRenderer: Send {
    /// Resets all renderer state.
    fn reset(&mut self);
    /// Releases any per-source state associated with the given handle.
    fn release_voice(&mut self, _handle: SourceHandle) {}
    /// Called once at the start of each output block.
    fn begin_block(&mut self, context: &SpatialRenderContext);
    /// Returns the left/right contribution for a single source sample.
    fn render_sample(&mut self, params: &SpatialVoiceParams, sample: f32) -> (f32, f32);
    /// Called once at the end of each output block.
    fn end_block(&mut self) {}
}
