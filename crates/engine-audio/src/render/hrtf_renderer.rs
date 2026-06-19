//! HRTF object renderer with budgeted hero-object selection.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use crate::hrtf::convolver::HrtfConvolver;
use crate::hrtf::dataset::HrtfDataset;
use crate::hrtf::listener_local_direction;
use crate::render::{SpatialRenderContext, SpatialRenderer, SpatialVoiceParams};
use crate::spatial::{compute_attenuation, compute_directivity, compute_effective_distance};
use crate::{SourceHandle, SpatialMode};

/// Binaural HRTF renderer.
///
/// Each source selected for HRTF keeps its own delay line and convolver state.
/// Filter coefficients are interpolated across each output block so that
/// motion is click-free.
#[derive(Clone)]
pub struct HrtfRenderer {
    dataset: Arc<dyn HrtfDataset>,
    convolvers: HashMap<SourceHandle, HrtfConvolver>,
    active_this_block: HashSet<SourceHandle>,
    listener: crate::AudioListenerDesc,
    block_frames: usize,
}

impl fmt::Debug for HrtfRenderer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HrtfRenderer")
            .field("convolvers", &self.convolvers.len())
            .finish_non_exhaustive()
    }
}

impl HrtfRenderer {
    /// Creates an HRTF renderer using the supplied dataset.
    pub fn new(dataset: Arc<dyn HrtfDataset>) -> Self {
        Self {
            dataset,
            convolvers: HashMap::new(),
            active_this_block: HashSet::new(),
            listener: crate::AudioListenerDesc::default(),
            block_frames: 1,
        }
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

impl SpatialRenderer for HrtfRenderer {
    fn reset(&mut self) {
        self.convolvers.clear();
        self.active_this_block.clear();
        self.listener = crate::AudioListenerDesc::default();
    }

    fn release_voice(&mut self, handle: SourceHandle) {
        self.convolvers.remove(&handle);
        self.active_this_block.remove(&handle);
    }

    fn begin_block(&mut self, context: &SpatialRenderContext) {
        self.listener = context.listener;
        self.block_frames = context.block_frames.max(1);
        self.active_this_block.clear();
    }

    fn render_sample(&mut self, params: &SpatialVoiceParams, sample: f32) -> (f32, f32) {
        let direction = listener_local_direction(
            params.position,
            self.listener.position,
            self.listener.forward,
            self.listener.up,
        );
        let gain = self.spatial_gain(params);

        let convolver = self
            .convolvers
            .entry(params.handle)
            .or_insert_with(|| HrtfConvolver::new(self.dataset.filter_taps()));

        if !self.active_this_block.contains(&params.handle) {
            convolver.prepare_block(&*self.dataset, direction, self.block_frames);
            self.active_this_block.insert(params.handle);
        }

        let (left, right) = convolver.process_sample(sample);
        (left * gain, right * gain)
    }

    fn end_block(&mut self) {
        self.active_this_block.clear();
    }
}
