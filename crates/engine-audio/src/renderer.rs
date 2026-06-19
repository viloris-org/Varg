//! Deterministic block renderer used by device, memory, and offline backends.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::{
    compute_attenuation, compute_doppler_rate,
    hrtf::default_dataset,
    render::{
        HrtfRenderer, SpatialRenderContext, SpatialRenderer, SpatialVoiceParams, StereoRenderer,
    },
    voice::{select_voices, VoiceScoreInput},
    AudioDebugSnapshot, AudioDiagnostics, AudioListenerDesc, AudioObjectTransform,
    AudioRendererConfig, AudioSourceDesc, ClipHandle, HrtfQuality, OutputMode, PlaybackState,
    PropagationFrame, SourceHandle, SpatialMode, VirtualizationPolicy,
};

/// Immutable decoded PCM clip consumed by the real-time renderer.
#[derive(Clone, Debug)]
pub struct PcmClip {
    /// Interleaved floating-point samples.
    pub samples: Vec<f32>,
    /// Source channel count.
    pub channels: u16,
    /// Source sample rate.
    pub sample_rate: u32,
    streaming: bool,
    ended: bool,
}

impl PcmClip {
    /// Creates a validated PCM clip.
    pub fn new(samples: Arc<[f32]>, channels: u16, sample_rate: u32) -> Option<Self> {
        (channels > 0 && sample_rate > 0).then_some(Self {
            samples: samples.to_vec(),
            channels,
            sample_rate,
            streaming: false,
            ended: true,
        })
    }

    /// Creates an initially empty streaming clip.
    pub fn streaming(channels: u16, sample_rate: u32) -> Option<Self> {
        (channels > 0 && sample_rate > 0).then_some(Self {
            samples: Vec::with_capacity(sample_rate as usize * channels as usize * 2),
            channels,
            sample_rate,
            streaming: true,
            ended: false,
        })
    }

    fn frame_count(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }
}

/// Commands accepted by the render plane.
#[derive(Clone, Debug)]
pub enum AudioCommand {
    /// Register immutable decoded samples.
    LoadClip {
        /// Clip handle.
        handle: ClipHandle,
        /// PCM data.
        clip: PcmClip,
    },
    /// Append a decoded packet to a streaming clip.
    AppendStream {
        /// Clip handle.
        handle: ClipHandle,
        /// Interleaved samples.
        samples: Arc<[f32]>,
    },
    /// Mark a streaming clip as complete.
    EndStream {
        /// Clip handle.
        handle: ClipHandle,
    },
    /// Remove a clip and all voices that reference it.
    UnloadClip {
        /// Clip handle.
        handle: ClipHandle,
    },
    /// Create a logical source.
    SpawnSource {
        /// Source handle.
        handle: SourceHandle,
        /// Source description.
        desc: AudioSourceDesc,
    },
    /// Destroy a logical source.
    DestroySource {
        /// Source handle.
        handle: SourceHandle,
    },
    /// Change playback state.
    SetPlayback {
        /// Source handle.
        handle: SourceHandle,
        /// New state.
        state: PlaybackState,
    },
    /// Schedule playback after a sample-frame delay.
    SchedulePlay {
        /// Source handle.
        handle: SourceHandle,
        /// Delay measured in output sample frames.
        delay_frames: u64,
    },
    /// Change source volume.
    SetVolume {
        /// Source handle.
        handle: SourceHandle,
        /// Linear gain.
        volume: f32,
    },
    /// Change source pitch.
    SetPitch {
        /// Source handle.
        handle: SourceHandle,
        /// Playback-rate multiplier.
        pitch: f32,
    },
    /// Seek to an absolute position.
    Seek {
        /// Source handle.
        handle: SourceHandle,
        /// Position in seconds.
        seconds: f32,
    },
    /// Smoothly fade to a target gain.
    FadeTo {
        /// Source handle.
        handle: SourceHandle,
        /// Target linear gain.
        volume: f32,
        /// Fade duration in seconds.
        duration_seconds: f32,
    },
    /// Change looping behavior.
    SetLooping {
        /// Source handle.
        handle: SourceHandle,
        /// Whether playback loops.
        looping: bool,
    },
    /// Update source world transform and velocity.
    SetSourceTransform {
        /// Source handle.
        handle: SourceHandle,
        /// New transform.
        transform: AudioObjectTransform,
    },
    /// Update source acoustic propagation parameters.
    SetSourcePropagation {
        /// Source handle.
        handle: SourceHandle,
        /// Propagation frame.
        propagation: PropagationFrame,
    },
    /// Update the active listener.
    SetListener {
        /// Listener descriptor.
        listener: AudioListenerDesc,
    },
    /// Set the linear gain applied to a named bus.
    SetBusGain {
        /// Bus name.
        bus: String,
        /// Linear gain.
        gain: f32,
    },
    /// Set the output rendering mode.
    SetOutputMode {
        /// Output mode.
        mode: OutputMode,
    },
    /// Set the HRTF quality tier.
    SetHrtfQuality {
        /// HRTF quality.
        quality: HrtfQuality,
    },
    /// Set the maximum number of HRTF objects.
    SetHrtfBudget {
        /// Maximum HRTF objects.
        max_hrtf_objects: u32,
    },
}

/// Events emitted by the render plane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioEvent {
    /// A non-looping source reached the end of its clip.
    SourceFinished(SourceHandle),
}

#[derive(Clone, Debug)]
struct RenderVoice {
    desc: AudioSourceDesc,
    state: PlaybackState,
    cursor_frames: f64,
    smoothed_volume: f32,
    transform: AudioObjectTransform,
    target_transform: AudioObjectTransform,
    propagation: PropagationFrame,
    smoothed_propagation: PropagationFrame,
    fade_target: Option<f32>,
    fade_frames_remaining: u64,
    scheduled_start_frames: u64,
}

/// Device-independent real-time mixer.
pub struct AudioRenderer {
    config: AudioRendererConfig,
    clips: HashMap<ClipHandle, PcmClip>,
    voices: HashMap<SourceHandle, RenderVoice>,
    listener: AudioListenerDesc,
    bus_gains: HashMap<String, f32>,
    diagnostics: AudioDiagnostics,
    events: Vec<AudioEvent>,
    stereo_renderer: StereoRenderer,
    hrtf_renderer: HrtfRenderer,
    hrtf_dataset: std::sync::Arc<dyn crate::hrtf::HrtfDataset>,
    voice_inputs: Vec<VoiceScoreInput>,
    physical_voices: HashSet<SourceHandle>,
    hrtf_voices: HashSet<SourceHandle>,
    debug_snapshot: AudioDebugSnapshot,
}

impl std::fmt::Debug for AudioRenderer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AudioRenderer")
            .field("config", &self.config)
            .field("clips", &self.clips.len())
            .field("voices", &self.voices.len())
            .field("listener", &self.listener)
            .field("bus_gains", &self.bus_gains)
            .field("diagnostics", &self.diagnostics)
            .field("events", &self.events)
            .field("stereo_renderer", &self.stereo_renderer)
            .field("hrtf_renderer", &self.hrtf_renderer)
            .field("voice_inputs", &self.voice_inputs.len())
            .field("physical_voices", &self.physical_voices)
            .field("hrtf_voices", &self.hrtf_voices)
            .field("debug_snapshot", &self.debug_snapshot)
            .finish()
    }
}

impl AudioRenderer {
    /// Creates an empty renderer.
    pub fn new(config: AudioRendererConfig) -> Self {
        let voice_capacity = config.max_physical_voices.max(256) as usize;
        let mut bus_gains = HashMap::with_capacity(16);
        bus_gains.insert("Master".to_string(), 1.0);
        let hrtf_dataset =
            default_dataset(config.sample_rate, config.head_radius, config.hrtf_quality);
        Self {
            config,
            clips: HashMap::with_capacity(256),
            voices: HashMap::with_capacity(voice_capacity),
            listener: AudioListenerDesc::default(),
            bus_gains,
            diagnostics: AudioDiagnostics::default(),
            events: Vec::with_capacity(voice_capacity),
            stereo_renderer: StereoRenderer::new(),
            hrtf_renderer: HrtfRenderer::new(std::sync::Arc::clone(&hrtf_dataset)),
            hrtf_dataset,
            voice_inputs: Vec::with_capacity(voice_capacity),
            physical_voices: HashSet::with_capacity(voice_capacity),
            hrtf_voices: HashSet::with_capacity(32),
            debug_snapshot: AudioDebugSnapshot::default(),
        }
    }

    /// Applies one control-plane command.
    pub fn apply(&mut self, command: AudioCommand) {
        match command {
            AudioCommand::LoadClip { handle, clip } => {
                self.clips.insert(handle, clip);
            }
            AudioCommand::AppendStream { handle, samples } => {
                if let Some(clip) = self.clips.get_mut(&handle) {
                    clip.samples.extend_from_slice(&samples);
                }
            }
            AudioCommand::EndStream { handle } => {
                if let Some(clip) = self.clips.get_mut(&handle) {
                    clip.ended = true;
                }
            }
            AudioCommand::UnloadClip { handle } => {
                self.clips.remove(&handle);
                self.voices.retain(|_, voice| voice.desc.clip != handle);
            }
            AudioCommand::SpawnSource { handle, desc } => {
                self.voices.insert(
                    handle,
                    RenderVoice {
                        state: if desc.auto_play {
                            PlaybackState::Playing
                        } else {
                            PlaybackState::Stopped
                        },
                        smoothed_volume: desc.volume.clamp(0.0, 1.0),
                        transform: AudioObjectTransform {
                            position: desc.position.unwrap_or_default(),
                            ..AudioObjectTransform::default()
                        },
                        target_transform: AudioObjectTransform {
                            position: desc.position.unwrap_or_default(),
                            ..AudioObjectTransform::default()
                        },
                        propagation: PropagationFrame::default(),
                        smoothed_propagation: PropagationFrame::default(),
                        desc,
                        cursor_frames: 0.0,
                        fade_target: None,
                        fade_frames_remaining: 0,
                        scheduled_start_frames: 0,
                    },
                );
            }
            AudioCommand::DestroySource { handle } => {
                self.voices.remove(&handle);
                self.stereo_renderer.release_voice(handle);
                self.hrtf_renderer.release_voice(handle);
            }
            AudioCommand::SetPlayback { handle, state } => {
                if let Some(voice) = self.voices.get_mut(&handle) {
                    voice.state = state;
                    if state == PlaybackState::Stopped {
                        voice.cursor_frames = 0.0;
                    }
                    if state == PlaybackState::Playing {
                        voice.scheduled_start_frames = 0;
                    }
                }
            }
            AudioCommand::SchedulePlay {
                handle,
                delay_frames,
            } => {
                if let Some(voice) = self.voices.get_mut(&handle) {
                    voice.state = PlaybackState::Playing;
                    voice.scheduled_start_frames = delay_frames;
                }
            }
            AudioCommand::SetVolume { handle, volume } => {
                if let Some(voice) = self.voices.get_mut(&handle) {
                    voice.desc.volume = volume.clamp(0.0, 1.0);
                    voice.fade_target = None;
                    voice.fade_frames_remaining = 0;
                }
            }
            AudioCommand::SetPitch { handle, pitch } => {
                if let Some(voice) = self.voices.get_mut(&handle) {
                    voice.desc.pitch = pitch.max(0.0);
                }
            }
            AudioCommand::Seek { handle, seconds } => {
                if let Some(voice) = self.voices.get_mut(&handle) {
                    if let Some(clip) = self.clips.get(&voice.desc.clip) {
                        voice.cursor_frames =
                            f64::from(seconds.max(0.0)) * f64::from(clip.sample_rate);
                    }
                }
            }
            AudioCommand::FadeTo {
                handle,
                volume,
                duration_seconds,
            } => {
                if let Some(voice) = self.voices.get_mut(&handle) {
                    voice.fade_target = Some(volume.clamp(0.0, 1.0));
                    voice.fade_frames_remaining =
                        (duration_seconds.max(0.0) * self.config.sample_rate as f32) as u64;
                    if voice.fade_frames_remaining == 0 {
                        voice.desc.volume = volume.clamp(0.0, 1.0);
                        voice.fade_target = None;
                    }
                }
            }
            AudioCommand::SetLooping { handle, looping } => {
                if let Some(voice) = self.voices.get_mut(&handle) {
                    voice.desc.looping = looping;
                }
            }
            AudioCommand::SetSourceTransform { handle, transform } => {
                if let Some(voice) = self.voices.get_mut(&handle) {
                    voice.target_transform = transform;
                    voice.desc.position = Some(transform.position);
                }
            }
            AudioCommand::SetSourcePropagation {
                handle,
                propagation,
            } => {
                if let Some(voice) = self.voices.get_mut(&handle) {
                    voice.propagation = propagation.sanitized();
                }
            }
            AudioCommand::SetListener { listener } => {
                self.config.output_mode = listener.output_mode;
                self.config.hrtf_quality = listener.hrtf_quality;
                self.listener = listener;
            }
            AudioCommand::SetBusGain { bus, gain } => {
                self.bus_gains.insert(bus, gain.clamp(0.0, 1.0));
            }
            AudioCommand::SetOutputMode { mode } => self.config.output_mode = mode,
            AudioCommand::SetHrtfQuality { quality } => {
                self.config.hrtf_quality = quality;
                self.hrtf_dataset = default_dataset(
                    self.config.sample_rate,
                    self.config.head_radius,
                    self.config.hrtf_quality,
                );
                self.hrtf_renderer = HrtfRenderer::new(std::sync::Arc::clone(&self.hrtf_dataset));
            }
            AudioCommand::SetHrtfBudget { max_hrtf_objects } => {
                self.config.max_hrtf_objects = max_hrtf_objects.max(1);
            }
        }
        self.refresh_counts();
    }

    /// Renders interleaved samples into `output`, replacing its contents.
    pub fn render(&mut self, output: &mut [f32]) {
        output.fill(0.0);
        let channels = usize::from(self.config.channels.max(1));
        let frame_count = output.len() / channels;
        if frame_count == 0 {
            return;
        }

        self.voice_inputs.clear();
        self.voice_inputs.extend(
            self.voices
                .iter()
                .filter(|(_, voice)| voice.state == PlaybackState::Playing)
                .map(|(handle, voice)| {
                    let distance = (voice.transform.position - self.listener.position).length();
                    let estimated_gain =
                        voice.desc.volume * compute_attenuation(voice.desc.attenuation, distance);
                    VoiceScoreInput {
                        handle: *handle,
                        category: voice.desc.category,
                        priority: voice.desc.priority,
                        critical: voice.desc.critical,
                        virtualization: voice.desc.virtualization,
                        spatial_mode: voice.desc.spatial_mode,
                        use_hrtf: voice.desc.use_hrtf,
                        volume: voice.desc.volume,
                        estimated_gain,
                    }
                }),
        );
        self.physical_voices.clear();
        self.hrtf_voices.clear();
        let hrtf_budget =
            if self.config.output_mode == OutputMode::Binaural && self.listener.hrtf_enabled {
                self.config.max_hrtf_objects as usize
            } else {
                0
            };
        let selection = select_voices(
            &self.voice_inputs,
            self.config.max_physical_voices as usize,
            hrtf_budget,
        );
        self.physical_voices.extend(selection.physical);
        self.hrtf_voices.extend(selection.hrtf);
        let physical_sources = sorted_handles(&self.physical_voices);
        let hrtf_sources = sorted_handles(&self.hrtf_voices);
        let mut virtual_sources = self
            .voice_inputs
            .iter()
            .map(|input| input.handle)
            .filter(|handle| !self.physical_voices.contains(handle))
            .map(|handle| handle.0)
            .collect::<Vec<_>>();
        virtual_sources.sort_unstable();
        let mut stereo_fallback_sources = self
            .voice_inputs
            .iter()
            .filter(|input| {
                input.spatial_mode == SpatialMode::Object
                    && input.use_hrtf
                    && self.physical_voices.contains(&input.handle)
                    && !self.hrtf_voices.contains(&input.handle)
            })
            .map(|input| input.handle.0)
            .collect::<Vec<_>>();
        stereo_fallback_sources.sort_unstable();
        self.debug_snapshot = AudioDebugSnapshot {
            physical_sources,
            virtual_sources,
            hrtf_sources,
            stereo_fallback_sources,
        };

        let listener = self.listener;
        let sample_rate = self.config.sample_rate;
        let bus_gains = &self.bus_gains;
        let clips = &self.clips;
        let mut finished = Vec::new();
        let mut virtual_count = 0_u32;
        let mut acoustic_sources = HashSet::new();
        let context = SpatialRenderContext {
            listener,
            sample_rate,
            channels: self.config.channels,
            block_frames: frame_count,
        };
        self.stereo_renderer.begin_block(&context);
        self.hrtf_renderer.begin_block(&context);

        for (handle, voice) in &mut self.voices {
            if voice.state != PlaybackState::Playing {
                continue;
            }
            let Some(clip) = clips.get(&voice.desc.clip) else {
                continue;
            };
            let transform_smoothing = 1.0 - (-1.0 / (0.02 * sample_rate as f32)).exp();
            let propagation_smoothing = 1.0 - (-1.0 / (0.05 * sample_rate as f32)).exp();
            let is_physical = self.physical_voices.contains(handle);
            if !is_physical {
                virtual_count = virtual_count.saturating_add(1);
                if voice.desc.virtualization == VirtualizationPolicy::Stop {
                    voice.state = PlaybackState::Stopped;
                    voice.cursor_frames = 0.0;
                    continue;
                }
            }

            let mut rate = f64::from(clip.sample_rate) / f64::from(sample_rate)
                * f64::from(voice.desc.pitch.max(0.0));
            if voice.desc.spatial_mode != SpatialMode::Direct && voice.desc.doppler_scale > 0.0 {
                let source_to_listener = listener.position - voice.transform.position;
                rate *= f64::from(compute_doppler_rate(
                    voice.transform.velocity,
                    listener.velocity,
                    source_to_listener,
                    self.config.speed_of_sound,
                    voice.desc.doppler_scale,
                ));
            }
            let distance = (voice.transform.position - listener.position).length();
            let spatial_gain = if voice.desc.spatial_mode == SpatialMode::Direct {
                1.0
            } else {
                compute_attenuation(voice.desc.attenuation, distance)
            };
            let bus_gain = bus_gains
                .get(&voice.desc.bus)
                .copied()
                .unwrap_or_else(|| bus_gains.get("Master").copied().unwrap_or(1.0));
            let gain_without_source = spatial_gain * bus_gain;
            let mut target_volume = voice.desc.volume.clamp(0.0, 1.0);
            let smoothing = 1.0 - (-1.0 / (0.005 * sample_rate as f32)).exp();
            let use_hrtf = self.hrtf_voices.contains(handle);

            for frame in 0..frame_count {
                let transform_smoothing = transform_smoothing.clamp(0.0, 1.0);
                voice.transform.position += (voice.target_transform.position
                    - voice.transform.position)
                    * transform_smoothing;
                voice.transform.forward = (voice.transform.forward
                    + (voice.target_transform.forward - voice.transform.forward)
                        * transform_smoothing)
                    .normalized();
                voice.transform.velocity += (voice.target_transform.velocity
                    - voice.transform.velocity)
                    * transform_smoothing;
                let propagation_smoothing = propagation_smoothing.clamp(0.0, 1.0);
                voice.smoothed_propagation.direct_gain += (voice.propagation.direct_gain
                    - voice.smoothed_propagation.direct_gain)
                    * propagation_smoothing;
                voice.smoothed_propagation.low_pass_hz += (voice.propagation.low_pass_hz
                    - voice.smoothed_propagation.low_pass_hz)
                    * propagation_smoothing;
                voice.smoothed_propagation.reverb_send += (voice.propagation.reverb_send
                    - voice.smoothed_propagation.reverb_send)
                    * propagation_smoothing;
                if voice.scheduled_start_frames > 0 {
                    voice.scheduled_start_frames -= 1;
                    continue;
                }
                if let Some(fade_target) = voice.fade_target {
                    if voice.fade_frames_remaining == 0 {
                        voice.desc.volume = fade_target;
                        voice.fade_target = None;
                    } else {
                        voice.desc.volume +=
                            (fade_target - voice.desc.volume) / voice.fade_frames_remaining as f32;
                        voice.fade_frames_remaining -= 1;
                    }
                    target_volume = voice.desc.volume.clamp(0.0, 1.0);
                }
                let source_frame = voice.cursor_frames.floor() as usize;
                if source_frame >= clip.frame_count() {
                    if voice.desc.looping && clip.frame_count() > 0 {
                        voice.cursor_frames %= clip.frame_count() as f64;
                    } else if clip.streaming && !clip.ended {
                        self.diagnostics.underruns = self.diagnostics.underruns.saturating_add(1);
                        break;
                    } else {
                        voice.state = PlaybackState::Stopped;
                        finished.push(*handle);
                        break;
                    }
                }
                let source_frame = voice.cursor_frames.floor() as usize;
                let next_frame = (source_frame + 1).min(clip.frame_count().saturating_sub(1));
                let fraction = (voice.cursor_frames - source_frame as f64) as f32;
                let (left, right) = sample_stereo(clip, source_frame, next_frame, fraction);
                voice.smoothed_volume +=
                    (target_volume - voice.smoothed_volume) * smoothing.clamp(0.0, 1.0);
                if is_physical {
                    let base = frame * channels;
                    let acoustic_gain = if voice.desc.spatial_mode == SpatialMode::Direct {
                        1.0
                    } else {
                        voice.smoothed_propagation.direct_gain
                    };
                    if acoustic_gain < 0.999 || voice.smoothed_propagation.reverb_send > 0.0 {
                        acoustic_sources.insert(*handle);
                    }
                    let source_gain = voice.smoothed_volume * gain_without_source * acoustic_gain;
                    if channels == 1 {
                        output[base] += (left + right) * 0.5 * source_gain;
                    } else if voice.desc.spatial_mode == SpatialMode::Direct {
                        output[base] += left * source_gain;
                        output[base + 1] += right * source_gain;
                        for channel in 2..channels {
                            output[base + channel] += (left + right) * 0.5 * source_gain;
                        }
                    } else {
                        let mono = (left + right) * 0.5;
                        let params = SpatialVoiceParams {
                            handle: *handle,
                            position: voice.transform.position,
                            forward: voice.transform.forward,
                            shape: voice.desc.shape,
                            attenuation: voice.desc.attenuation,
                            spatial_mode: voice.desc.spatial_mode,
                            spread: voice.desc.spread,
                            gain: voice.smoothed_volume * bus_gain * acoustic_gain,
                        };
                        let (spatial_left, spatial_right) = if use_hrtf {
                            self.hrtf_renderer.render_sample(&params, mono)
                        } else {
                            self.stereo_renderer.render_sample(&params, mono)
                        };
                        output[base] += spatial_left;
                        output[base + 1] += spatial_right;
                        for channel in 2..channels {
                            output[base + channel] += (spatial_left + spatial_right) * 0.5;
                        }
                    }
                }
                voice.cursor_frames += rate;
            }
        }

        let ceiling = self.config.limiter_ceiling.clamp(0.01, 1.0);
        let mut peak = 0.0_f32;
        for sample in output {
            *sample = sample.clamp(-ceiling, ceiling);
            peak = peak.max(sample.abs());
        }
        self.diagnostics.physical_voices = self.physical_voices.len().min(u32::MAX as usize) as u32;
        self.diagnostics.virtual_voices = virtual_count;
        self.diagnostics.hrtf_objects = self.hrtf_voices.len().min(u32::MAX as usize) as u32;
        self.diagnostics.hrtf_fallback_objects = self
            .debug_snapshot
            .stereo_fallback_sources
            .len()
            .min(u32::MAX as usize) as u32;
        self.diagnostics.acoustics_sources = acoustic_sources.len().min(u32::MAX as usize) as u32;
        self.diagnostics.output_peak = peak;
        self.diagnostics.rendered_frames = self
            .diagnostics
            .rendered_frames
            .saturating_add(frame_count as u64);
        for handle in finished {
            if let Some(voice) = self.voices.get_mut(&handle) {
                voice.cursor_frames = clips
                    .get(&voice.desc.clip)
                    .map(|clip| clip.frame_count() as f64)
                    .unwrap_or_default();
            }
            self.events.push(AudioEvent::SourceFinished(handle));
        }
        self.compact_streaming_clips();
        self.stereo_renderer.end_block();
        self.hrtf_renderer.end_block();
        self.refresh_counts();
    }

    /// Drains render-plane events into the provided callback.
    pub fn drain_events(&mut self, mut callback: impl FnMut(AudioEvent)) {
        for event in self.events.drain(..) {
            callback(event);
        }
    }

    /// Returns the current diagnostics snapshot.
    pub fn diagnostics(&self) -> AudioDiagnostics {
        self.diagnostics
    }

    /// Returns the latest renderer debug snapshot.
    pub fn debug_snapshot(&self) -> AudioDebugSnapshot {
        self.debug_snapshot.clone()
    }

    /// Increments the backend error/underrun counter.
    pub fn report_underrun(&mut self) {
        self.diagnostics.underruns = self.diagnostics.underruns.saturating_add(1);
    }

    fn refresh_counts(&mut self) {
        self.diagnostics.loaded_clips = self.clips.len().min(u32::MAX as usize) as u32;
        self.diagnostics.logical_sources = self.voices.len().min(u32::MAX as usize) as u32;
    }

    fn compact_streaming_clips(&mut self) {
        for (clip_handle, clip) in &mut self.clips {
            if !clip.streaming {
                continue;
            }
            let retained_limit_frames = clip.sample_rate as usize * 4;
            if clip.frame_count() <= retained_limit_frames * 2 {
                continue;
            }
            let minimum_cursor = self
                .voices
                .values()
                .filter(|voice| voice.desc.clip == *clip_handle)
                .map(|voice| voice.cursor_frames.max(0.0) as usize)
                .min()
                .unwrap_or(clip.frame_count());
            let drop_frames =
                minimum_cursor.min(clip.frame_count().saturating_sub(retained_limit_frames));
            if drop_frames == 0 {
                continue;
            }
            let drop_samples = drop_frames * usize::from(clip.channels);
            clip.samples.drain(..drop_samples);
            for voice in self
                .voices
                .values_mut()
                .filter(|voice| voice.desc.clip == *clip_handle)
            {
                voice.cursor_frames = (voice.cursor_frames - drop_frames as f64).max(0.0);
            }
        }
    }
}

fn sorted_handles(handles: &HashSet<SourceHandle>) -> Vec<u64> {
    let mut handles = handles.iter().map(|handle| handle.0).collect::<Vec<_>>();
    handles.sort_unstable();
    handles
}

fn sample_stereo(clip: &PcmClip, frame: usize, next_frame: usize, fraction: f32) -> (f32, f32) {
    let channels = usize::from(clip.channels);
    let sample = |frame: usize, channel: usize| {
        clip.samples
            .get(frame * channels + channel.min(channels.saturating_sub(1)))
            .copied()
            .unwrap_or_default()
    };
    let interpolate = |channel: usize| {
        let current = sample(frame, channel);
        current + (sample(next_frame, channel) - current) * fraction
    };
    if channels == 1 {
        let mono = interpolate(0);
        (mono, mono)
    } else {
        (interpolate(0), interpolate(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AttenuationModel, AudioSourceShape, Vec3, VoiceCategory};

    fn source(clip: ClipHandle) -> AudioSourceDesc {
        AudioSourceDesc {
            clip,
            volume: 1.0,
            pitch: 1.0,
            looping: false,
            position: None,
            auto_play: true,
            bus: "Master".to_string(),
            spatial_mode: SpatialMode::Direct,
            shape: AudioSourceShape::Point,
            attenuation: AttenuationModel::None,
            priority: 128,
            virtualization: VirtualizationPolicy::Virtualize,
            category: VoiceCategory::Sfx,
            critical: false,
            doppler_scale: 1.0,
            spread: 1.0,
            use_hrtf: true,
        }
    }

    #[test]
    fn renderer_resamples_and_limits_output() {
        let mut renderer = AudioRenderer::new(AudioRendererConfig {
            sample_rate: 48_000,
            channels: 2,
            max_physical_voices: 8,
            limiter_ceiling: 0.5,
            ..AudioRendererConfig::default()
        });
        renderer.apply(AudioCommand::LoadClip {
            handle: ClipHandle(1),
            clip: PcmClip::new(Arc::from([1.0_f32; 24_000]), 1, 24_000).unwrap(),
        });
        renderer.apply(AudioCommand::SpawnSource {
            handle: SourceHandle(1),
            desc: source(ClipHandle(1)),
        });
        let mut output = vec![0.0; 128];
        renderer.render(&mut output);
        assert!(output.iter().any(|sample| *sample != 0.0));
        assert!(output.iter().all(|sample| sample.abs() <= 0.5));
        assert_eq!(renderer.diagnostics().physical_voices, 1);
    }

    #[test]
    fn excess_voices_are_virtualized_deterministically() {
        let mut renderer = AudioRenderer::new(AudioRendererConfig {
            max_physical_voices: 1,
            ..AudioRendererConfig::default()
        });
        renderer.apply(AudioCommand::LoadClip {
            handle: ClipHandle(1),
            clip: PcmClip::new(Arc::from([0.25_f32; 1024]), 1, 48_000).unwrap(),
        });
        for handle in 1..=2 {
            let mut desc = source(ClipHandle(1));
            desc.priority = handle as u8;
            renderer.apply(AudioCommand::SpawnSource {
                handle: SourceHandle(handle),
                desc,
            });
        }
        renderer.render(&mut [0.0; 128]);
        assert_eq!(renderer.diagnostics().physical_voices, 1);
        assert_eq!(renderer.diagnostics().virtual_voices, 1);
    }

    #[test]
    fn scheduled_playback_stays_silent_until_delay_elapses() {
        let mut renderer = AudioRenderer::new(AudioRendererConfig::default());
        renderer.apply(AudioCommand::LoadClip {
            handle: ClipHandle(1),
            clip: PcmClip::new(Arc::from([0.5_f32; 1024]), 1, 48_000).unwrap(),
        });
        let mut desc = source(ClipHandle(1));
        desc.auto_play = false;
        renderer.apply(AudioCommand::SpawnSource {
            handle: SourceHandle(1),
            desc,
        });
        renderer.apply(AudioCommand::SchedulePlay {
            handle: SourceHandle(1),
            delay_frames: 16,
        });
        let mut output = [0.0; 64];
        renderer.render(&mut output);
        assert!(output[..32].iter().all(|sample| *sample == 0.0));
        assert!(output[32..].iter().any(|sample| *sample != 0.0));
    }

    #[test]
    fn fade_reaches_target_without_exceeding_limiter() {
        let mut renderer = AudioRenderer::new(AudioRendererConfig::default());
        renderer.apply(AudioCommand::LoadClip {
            handle: ClipHandle(1),
            clip: PcmClip::new(Arc::from([1.0_f32; 4096]), 1, 48_000).unwrap(),
        });
        renderer.apply(AudioCommand::SpawnSource {
            handle: SourceHandle(1),
            desc: source(ClipHandle(1)),
        });
        renderer.apply(AudioCommand::FadeTo {
            handle: SourceHandle(1),
            volume: 0.0,
            duration_seconds: 64.0 / 48_000.0,
        });
        let mut output = [0.0; 2048];
        renderer.render(&mut output);
        assert!(output.iter().all(|sample| sample.abs() <= 0.98));
        assert!(output[output.len() - 16..]
            .iter()
            .all(|sample| sample.abs() < 0.1));
    }

    #[test]
    fn streaming_clip_accepts_incremental_blocks() {
        let mut renderer = AudioRenderer::new(AudioRendererConfig::default());
        renderer.apply(AudioCommand::LoadClip {
            handle: ClipHandle(1),
            clip: PcmClip::streaming(1, 48_000).unwrap(),
        });
        renderer.apply(AudioCommand::AppendStream {
            handle: ClipHandle(1),
            samples: Arc::from([0.25_f32; 64]),
        });
        renderer.apply(AudioCommand::SpawnSource {
            handle: SourceHandle(1),
            desc: source(ClipHandle(1)),
        });
        let mut first = [0.0; 128];
        renderer.render(&mut first);
        assert!(first.iter().any(|sample| *sample != 0.0));

        renderer.apply(AudioCommand::AppendStream {
            handle: ClipHandle(1),
            samples: Arc::from([0.5_f32; 64]),
        });
        renderer.apply(AudioCommand::EndStream {
            handle: ClipHandle(1),
        });
        let mut second = [0.0; 128];
        renderer.render(&mut second);
        assert!(second.iter().any(|sample| *sample != 0.0));
    }

    #[test]
    fn binaural_mode_applies_hrtf_budget_without_dropping_objects() {
        let mut renderer = AudioRenderer::new(AudioRendererConfig {
            output_mode: OutputMode::Binaural,
            max_physical_voices: 4,
            max_hrtf_objects: 1,
            ..AudioRendererConfig::default()
        });
        renderer.apply(AudioCommand::LoadClip {
            handle: ClipHandle(1),
            clip: PcmClip::new(Arc::from([0.25_f32; 4096]), 1, 48_000).unwrap(),
        });
        for handle in 1..=2 {
            let mut desc = source(ClipHandle(1));
            desc.spatial_mode = SpatialMode::Object;
            desc.position = Some(Vec3::new(handle as f32, 0.0, -1.0));
            renderer.apply(AudioCommand::SpawnSource {
                handle: SourceHandle(handle),
                desc,
            });
        }

        let mut output = [0.0; 512];
        renderer.render(&mut output);

        let diagnostics = renderer.diagnostics();
        assert_eq!(diagnostics.physical_voices, 2);
        assert_eq!(diagnostics.virtual_voices, 0);
        assert_eq!(diagnostics.hrtf_objects, 1);
        assert!(output.iter().any(|sample| *sample != 0.0));
    }

    #[test]
    fn direct_sources_bypass_hrtf_selection() {
        let mut renderer = AudioRenderer::new(AudioRendererConfig {
            output_mode: OutputMode::Binaural,
            max_physical_voices: 2,
            max_hrtf_objects: 2,
            ..AudioRendererConfig::default()
        });
        renderer.apply(AudioCommand::LoadClip {
            handle: ClipHandle(1),
            clip: PcmClip::new(Arc::from([0.25_f32; 4096]), 1, 48_000).unwrap(),
        });
        let mut desc = source(ClipHandle(1));
        desc.spatial_mode = SpatialMode::Direct;
        desc.position = Some(Vec3::new(1.0, 0.0, -1.0));
        renderer.apply(AudioCommand::SpawnSource {
            handle: SourceHandle(1),
            desc,
        });

        let mut output = [0.0; 512];
        renderer.render(&mut output);

        assert_eq!(renderer.diagnostics().physical_voices, 1);
        assert_eq!(renderer.diagnostics().hrtf_objects, 0);
        assert!(output.iter().any(|sample| *sample != 0.0));
    }

    #[test]
    fn propagation_smoothly_reduces_object_output() {
        let mut renderer = AudioRenderer::new(AudioRendererConfig::default());
        renderer.apply(AudioCommand::LoadClip {
            handle: ClipHandle(1),
            clip: PcmClip::new(Arc::from([0.5_f32; 4096]), 1, 48_000).unwrap(),
        });
        let mut desc = source(ClipHandle(1));
        desc.spatial_mode = SpatialMode::Object;
        desc.position = Some(Vec3::new(0.0, 0.0, -1.0));
        renderer.apply(AudioCommand::SpawnSource {
            handle: SourceHandle(1),
            desc,
        });
        let mut clear = [0.0; 2048];
        renderer.render(&mut clear);
        let clear_peak = clear
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));

        renderer.apply(AudioCommand::SetSourcePropagation {
            handle: SourceHandle(1),
            propagation: PropagationFrame {
                direct_gain: 0.1,
                low_pass_hz: 2_000.0,
                reverb_send: 0.2,
                delay_seconds: 0.0,
            },
        });
        let mut occluded = [0.0; 4096];
        renderer.render(&mut occluded);
        let occluded_peak = occluded
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));

        assert!(occluded_peak < clear_peak);
        assert_eq!(renderer.diagnostics().acoustics_sources, 1);
    }

    #[test]
    fn debug_snapshot_reports_hrtf_fallback_sources() {
        let mut renderer = AudioRenderer::new(AudioRendererConfig {
            output_mode: OutputMode::Binaural,
            max_physical_voices: 2,
            max_hrtf_objects: 1,
            ..AudioRendererConfig::default()
        });
        renderer.apply(AudioCommand::LoadClip {
            handle: ClipHandle(1),
            clip: PcmClip::new(Arc::from([0.25_f32; 4096]), 1, 48_000).unwrap(),
        });
        for handle in 1..=2 {
            let mut desc = source(ClipHandle(1));
            desc.spatial_mode = SpatialMode::Object;
            desc.position = Some(Vec3::new(handle as f32, 0.0, -1.0));
            renderer.apply(AudioCommand::SpawnSource {
                handle: SourceHandle(handle),
                desc,
            });
        }

        renderer.render(&mut [0.0; 512]);
        let snapshot = renderer.debug_snapshot();
        assert_eq!(snapshot.physical_sources.len(), 2);
        assert_eq!(snapshot.hrtf_sources.len(), 1);
        assert_eq!(snapshot.stereo_fallback_sources.len(), 1);
        assert_eq!(renderer.diagnostics().hrtf_fallback_objects, 1);
    }
}
