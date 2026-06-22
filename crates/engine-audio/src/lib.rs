#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Audio abstraction, asset decoding, and default backends for the Aster engine.
//!
//! The null backend compiles everywhere and satisfies the trait contract without
//! linking any audio library. A real backend (FMOD, kira, cpal, …) replaces it
//! by implementing [`AudioBackend`] and registering it at startup.

pub mod acoustics;
pub mod bus;
mod decode;
#[cfg(feature = "device-output")]
mod device;
pub mod effects;
pub mod hrtf;
pub mod render;
pub mod renderer;
pub mod spatial;
pub mod stream;
pub mod stream_player;
pub mod synth;
pub mod types;
pub mod voice;

use std::collections::HashMap;

use engine_core::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

pub use crate::acoustics::{
    AcousticAabb, AcousticMaterial, AcousticQuality, AcousticSceneSnapshot, AcousticSolverConfig,
    AcousticSourceSample, solve_direct_propagation,
};
pub use crate::bus::{AudioBus, AudioBusGraph};
#[cfg(feature = "device-output")]
pub use crate::device::DeviceAudioBackend;
pub use crate::effects::{
    AudioEffect, ChorusEffect, CompressorEffect, DelayEffect, EqEffect, FilterEffect, FilterType,
    LimiterEffect, ReverbEffect,
};
pub use crate::spatial::{
    AttenuationModel, compute_attenuation, compute_directivity, compute_doppler_rate,
    compute_effective_distance, compute_pan,
};
pub use crate::stream_player::{
    AudioStreamPlayer2DComponentData, AudioStreamPlayer3DComponentData,
};
pub use crate::types::{
    AudioDebugSnapshot, AudioDiagnostics, AudioLatencyProfile, AudioObjectTransform,
    AudioOutputCapabilities, AudioOutputSettings, AudioRendererConfig, AudioSourceShape,
    HrtfQuality, OutputMode, PropagationFrame, SpatialMode, VirtualizationPolicy, VoiceCategory,
};
pub use engine_core::math::Vec3;

// ── Handles ──────────────────────────────────────────────────────────────────

/// Opaque handle to a loaded audio clip.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub struct ClipHandle(pub u64);

/// Opaque handle to a playing audio source.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub struct SourceHandle(pub u64);

// ── AudioClip ────────────────────────────────────────────────────────────────

/// Metadata for a loaded audio clip.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct AudioClipInfo {
    /// Clip name or asset path.
    pub name: String,
    /// Duration in seconds.
    pub duration_secs: f32,
    /// Number of audio channels.
    pub channels: u16,
    /// Sample rate in Hz.
    pub sample_rate: u32,
}

#[derive(Clone, Debug)]
struct AudioClip {
    info: AudioClipInfo,
    _samples: Vec<f32>,
}

// ── AudioSource ──────────────────────────────────────────────────────────────

/// Playback state of an audio source.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackState {
    /// Not playing.
    #[default]
    Stopped,
    /// Currently playing.
    Playing,
    /// Paused mid-playback.
    Paused,
}

/// Parameters for spawning an audio source.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct AudioSourceDesc {
    /// Clip to play.
    pub clip: ClipHandle,
    /// Playback volume in `[0.0, 1.0]`.
    pub volume: f32,
    /// Playback pitch multiplier.
    pub pitch: f32,
    /// Whether to loop.
    pub looping: bool,
    /// World-space position for 3-D spatialization; `None` for 2-D.
    pub position: Option<Vec3>,
    /// Start playing immediately on spawn.
    pub auto_play: bool,
    /// Output bus name.
    #[serde(default = "default_bus_name")]
    pub bus: String,
    /// Semantic spatial rendering mode.
    #[serde(default)]
    pub spatial_mode: SpatialMode,
    /// Geometric source approximation.
    #[serde(default)]
    pub shape: AudioSourceShape,
    /// Distance attenuation model.
    #[serde(default)]
    pub attenuation: AttenuationModel,
    /// Voice-allocation priority. Higher values win.
    #[serde(default = "default_priority")]
    pub priority: u8,
    /// Behavior when the physical voice budget is exhausted.
    #[serde(default)]
    pub virtualization: VirtualizationPolicy,
    /// Voice category used for reservation and fallback policy.
    #[serde(default)]
    pub category: VoiceCategory,
    /// Marks a source as gameplay-critical (protected from virtualization).
    #[serde(default)]
    pub critical: bool,
    /// Doppler scale multiplier (`0.0` disables Doppler).
    #[serde(default = "default_doppler_scale")]
    pub doppler_scale: f32,
    /// Spatial spread in `[0.0, 1.0]` (`0.0` = mono, `1.0` = full directional).
    #[serde(default = "default_spread")]
    pub spread: f32,
    /// Whether this source may be rendered with HRTF in binaural mode.
    #[serde(default = "default_use_hrtf")]
    pub use_hrtf: bool,
}

fn default_doppler_scale() -> f32 {
    1.0
}

fn default_spread() -> f32 {
    1.0
}

fn default_use_hrtf() -> bool {
    true
}

fn default_bus_name() -> String {
    "Master".to_string()
}

fn default_priority() -> u8 {
    128
}

impl AudioSourceDesc {
    /// Creates a simple 2-D source at full volume.
    pub fn simple(clip: ClipHandle) -> Self {
        Self {
            clip,
            volume: 1.0,
            pitch: 1.0,
            looping: false,
            position: None,
            auto_play: true,
            bus: "Master".to_string(),
            spatial_mode: SpatialMode::Direct,
            shape: AudioSourceShape::Point,
            attenuation: AttenuationModel::default(),
            priority: default_priority(),
            virtualization: VirtualizationPolicy::Virtualize,
            category: VoiceCategory::default(),
            critical: false,
            doppler_scale: default_doppler_scale(),
            spread: default_spread(),
            use_hrtf: default_use_hrtf(),
        }
    }
}

#[derive(Clone, Debug)]
struct AudioSource {
    desc: AudioSourceDesc,
    state: PlaybackState,
    cursor_seconds: f32,
    scheduled_delay_seconds: f32,
}

// ── AudioListener ────────────────────────────────────────────────────────────

/// World-space listener transform used for 3-D spatialization.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct AudioListenerDesc {
    /// Listener position.
    pub position: Vec3,
    /// Forward direction (unit vector).
    pub forward: Vec3,
    /// Up direction (unit vector).
    pub up: Vec3,
    /// World-space velocity in units per second.
    #[serde(default)]
    pub velocity: Vec3,
    /// Preferred output rendering mode.
    #[serde(default)]
    pub output_mode: OutputMode,
    /// Preferred HRTF quality.
    #[serde(default)]
    pub hrtf_quality: HrtfQuality,
    /// Whether HRTF is enabled when the output mode is binaural.
    #[serde(default = "default_hrtf_enabled")]
    pub hrtf_enabled: bool,
}

fn default_hrtf_enabled() -> bool {
    true
}

impl Default for AudioListenerDesc {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            forward: Vec3::new(0.0, 0.0, -1.0),
            up: Vec3::new(0.0, 1.0, 0.0),
            velocity: Vec3::ZERO,
            output_mode: OutputMode::default(),
            hrtf_quality: HrtfQuality::default(),
            hrtf_enabled: default_hrtf_enabled(),
        }
    }
}

// ── Backend trait ─────────────────────────────────────────────────────────────

/// Pluggable audio backend contract.
pub trait AudioBackend {
    /// Loads a clip from raw PCM data (interleaved f32 samples).
    fn load_clip(
        &mut self,
        name: &str,
        samples: &[f32],
        channels: u16,
        sample_rate: u32,
    ) -> EngineResult<ClipHandle>;

    /// Loads an encoded long-form asset for bounded background streaming.
    ///
    /// Backends without streaming support decode the asset as a resident clip.
    fn load_streamed_clip(
        &mut self,
        name: &str,
        bytes: std::sync::Arc<[u8]>,
    ) -> EngineResult<ClipHandle> {
        let (samples, channels, sample_rate) = decode_audio_bytes(name, &bytes)?;
        self.load_clip(name, &samples, channels, sample_rate)
    }

    /// Unloads a clip.
    fn unload_clip(&mut self, clip: ClipHandle) -> EngineResult<()>;

    /// Returns clip metadata.
    fn clip_info(&self, clip: ClipHandle) -> EngineResult<AudioClipInfo>;

    /// Spawns an audio source and returns its handle.
    fn spawn_source(&mut self, desc: &AudioSourceDesc) -> EngineResult<SourceHandle>;

    /// Destroys a source.
    fn destroy_source(&mut self, source: SourceHandle) -> EngineResult<()>;

    /// Starts or resumes playback.
    fn play(&mut self, source: SourceHandle) -> EngineResult<()>;

    /// Schedules playback after a delay in seconds.
    fn play_scheduled(&mut self, source: SourceHandle, _delay_seconds: f32) -> EngineResult<()> {
        self.play(source)
    }

    /// Pauses playback.
    fn pause(&mut self, source: SourceHandle) -> EngineResult<()>;

    /// Stops playback and rewinds.
    fn stop(&mut self, source: SourceHandle) -> EngineResult<()>;

    /// Sets the volume of a source.
    fn set_volume(&mut self, source: SourceHandle, volume: f32) -> EngineResult<()>;

    /// Sets the playback pitch/rate multiplier.
    fn set_pitch(&mut self, _source: SourceHandle, _pitch: f32) -> EngineResult<()> {
        Ok(())
    }

    /// Seeks to an absolute playback position.
    fn seek(&mut self, _source: SourceHandle, _seconds: f32) -> EngineResult<()> {
        Ok(())
    }

    /// Fades a source to a target volume over the requested duration.
    fn fade_to(
        &mut self,
        source: SourceHandle,
        volume: f32,
        _duration_seconds: f32,
    ) -> EngineResult<()> {
        self.set_volume(source, volume)
    }

    /// Sets the loop flag of a source.
    fn set_looping(&mut self, source: SourceHandle, looping: bool) -> EngineResult<()>;

    /// Updates a source's world transform and velocity.
    fn set_source_transform(
        &mut self,
        source: SourceHandle,
        transform: AudioObjectTransform,
    ) -> EngineResult<()> {
        let _ = (source, transform);
        Ok(())
    }

    /// Updates acoustic propagation parameters for a source.
    fn set_source_propagation(
        &mut self,
        _source: SourceHandle,
        _propagation: PropagationFrame,
    ) -> EngineResult<()> {
        Ok(())
    }

    /// Returns the current playback state.
    fn playback_state(&self, source: SourceHandle) -> EngineResult<PlaybackState>;

    /// Updates the listener transform for 3-D spatialization.
    fn set_listener(&mut self, desc: &AudioListenerDesc);

    /// Returns active output capabilities.
    fn capabilities(&self) -> AudioOutputCapabilities {
        AudioOutputCapabilities::default()
    }

    /// Returns a non-blocking diagnostics snapshot.
    fn diagnostics(&self) -> AudioDiagnostics {
        AudioDiagnostics::default()
    }

    /// Returns the latest renderer debug snapshot.
    fn debug_snapshot(&self) -> AudioDebugSnapshot {
        AudioDebugSnapshot::default()
    }

    /// Updates a named bus gain in the render plane.
    fn set_bus_gain(&mut self, _bus: &str, _gain: f32) -> EngineResult<()> {
        Ok(())
    }

    /// Sets the output rendering mode.
    fn set_output_mode(&mut self, _mode: OutputMode) -> EngineResult<()> {
        Ok(())
    }

    /// Sets the HRTF quality tier.
    fn set_hrtf_quality(&mut self, _quality: HrtfQuality) -> EngineResult<()> {
        Ok(())
    }

    /// Sets the maximum number of HRTF objects the renderer may use.
    fn set_hrtf_budget(&mut self, _max_hrtf_objects: u32) -> EngineResult<()> {
        Ok(())
    }

    /// Advances the audio engine by `dt` seconds (called each frame).
    fn update(&mut self, dt: f32);
}

/// Decodes an audio asset into interleaved f32 PCM samples.
///
/// WAV PCM and IEEE-float files are supported directly. OGG containers are
/// recognized and reported as unsupported unless a concrete backend provides its
/// own decoder.
pub fn decode_audio_bytes(name: &str, bytes: &[u8]) -> EngineResult<(Vec<f32>, u16, u32)> {
    decode::decode(name, bytes)
}

// ── Null backend ──────────────────────────────────────────────────────────────

/// No-op audio backend. Compiles everywhere; produces no sound.
#[derive(Default)]
pub struct NullAudioBackend;

impl AudioBackend for NullAudioBackend {
    fn load_clip(
        &mut self,
        _name: &str,
        _samples: &[f32],
        _channels: u16,
        _sample_rate: u32,
    ) -> EngineResult<ClipHandle> {
        Err(EngineError::other("null audio backend"))
    }

    fn unload_clip(&mut self, _clip: ClipHandle) -> EngineResult<()> {
        Ok(())
    }

    fn clip_info(&self, _clip: ClipHandle) -> EngineResult<AudioClipInfo> {
        Err(EngineError::other("null audio backend"))
    }

    fn spawn_source(&mut self, _desc: &AudioSourceDesc) -> EngineResult<SourceHandle> {
        Err(EngineError::other("null audio backend"))
    }

    fn destroy_source(&mut self, _source: SourceHandle) -> EngineResult<()> {
        Ok(())
    }

    fn play(&mut self, _source: SourceHandle) -> EngineResult<()> {
        Ok(())
    }

    fn pause(&mut self, _source: SourceHandle) -> EngineResult<()> {
        Ok(())
    }

    fn stop(&mut self, _source: SourceHandle) -> EngineResult<()> {
        Ok(())
    }

    fn set_volume(&mut self, _source: SourceHandle, _volume: f32) -> EngineResult<()> {
        Ok(())
    }

    fn set_looping(&mut self, _source: SourceHandle, _looping: bool) -> EngineResult<()> {
        Ok(())
    }

    fn playback_state(&self, _source: SourceHandle) -> EngineResult<PlaybackState> {
        Err(EngineError::other("null audio backend"))
    }

    fn set_listener(&mut self, _desc: &AudioListenerDesc) {}

    fn update(&mut self, _dt: f32) {}
}

// ── Memory backend ───────────────────────────────────────────────────────────

/// Deterministic in-memory audio backend used by runtime tests and headless demos.
///
/// The backend decodes clips, owns source lifecycle, tracks listener/source state,
/// and advances playback cursors during frame updates. It does not open an OS
/// audio device, so projects can validate audio behavior on machines without
/// native audio output.
#[derive(Default)]
pub struct MemoryAudioBackend {
    next_clip: u64,
    next_source: u64,
    clips: HashMap<ClipHandle, AudioClip>,
    sources: HashMap<SourceHandle, AudioSource>,
    propagation: HashMap<SourceHandle, PropagationFrame>,
    listener: AudioListenerDesc,
}

impl MemoryAudioBackend {
    /// Creates an empty memory audio backend.
    pub fn new() -> Self {
        Self {
            next_clip: 1,
            next_source: 1,
            clips: HashMap::new(),
            sources: HashMap::new(),
            propagation: HashMap::new(),
            listener: AudioListenerDesc::default(),
        }
    }

    /// Decodes and loads a clip from an encoded asset.
    pub fn load_encoded_clip(&mut self, name: &str, bytes: &[u8]) -> EngineResult<ClipHandle> {
        let (samples, channels, sample_rate) = decode_audio_bytes(name, bytes)?;
        self.load_clip(name, &samples, channels, sample_rate)
    }

    /// Returns the number of loaded clips.
    pub fn clip_count(&self) -> usize {
        self.clips.len()
    }

    /// Returns the number of live sources.
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    /// Returns the latest listener descriptor.
    pub fn listener(&self) -> AudioListenerDesc {
        self.listener
    }

    fn clip(&self, handle: ClipHandle) -> EngineResult<&AudioClip> {
        self.clips
            .get(&handle)
            .ok_or_else(|| EngineError::invalid_handle("audio clip does not exist"))
    }

    fn source_mut(&mut self, handle: SourceHandle) -> EngineResult<&mut AudioSource> {
        self.sources
            .get_mut(&handle)
            .ok_or_else(|| EngineError::invalid_handle("audio source does not exist"))
    }
}

impl AudioBackend for MemoryAudioBackend {
    fn load_clip(
        &mut self,
        name: &str,
        samples: &[f32],
        channels: u16,
        sample_rate: u32,
    ) -> EngineResult<ClipHandle> {
        if channels == 0 || sample_rate == 0 {
            return Err(EngineError::other(
                "audio clip must have channels and sample rate",
            ));
        }
        let handle = ClipHandle(self.next_clip);
        self.next_clip = self.next_clip.saturating_add(1).max(1);
        let duration_secs = samples.len() as f32 / channels as f32 / sample_rate as f32;
        self.clips.insert(
            handle,
            AudioClip {
                info: AudioClipInfo {
                    name: name.to_string(),
                    duration_secs,
                    channels,
                    sample_rate,
                },
                _samples: samples.to_vec(),
            },
        );
        Ok(handle)
    }

    fn unload_clip(&mut self, clip: ClipHandle) -> EngineResult<()> {
        self.clips
            .remove(&clip)
            .ok_or_else(|| EngineError::invalid_handle("audio clip does not exist"))?;
        self.sources.retain(|_, source| source.desc.clip != clip);
        self.propagation
            .retain(|source, _| self.sources.contains_key(source));
        Ok(())
    }

    fn clip_info(&self, clip: ClipHandle) -> EngineResult<AudioClipInfo> {
        Ok(self.clip(clip)?.info.clone())
    }

    fn spawn_source(&mut self, desc: &AudioSourceDesc) -> EngineResult<SourceHandle> {
        self.clip(desc.clip)?;
        let handle = SourceHandle(self.next_source);
        self.next_source = self.next_source.saturating_add(1).max(1);
        self.sources.insert(
            handle,
            AudioSource {
                desc: desc.clone(),
                state: if desc.auto_play {
                    PlaybackState::Playing
                } else {
                    PlaybackState::Stopped
                },
                cursor_seconds: 0.0,
                scheduled_delay_seconds: 0.0,
            },
        );
        Ok(handle)
    }

    fn destroy_source(&mut self, source: SourceHandle) -> EngineResult<()> {
        self.sources
            .remove(&source)
            .ok_or_else(|| EngineError::invalid_handle("audio source does not exist"))?;
        self.propagation.remove(&source);
        Ok(())
    }

    fn play(&mut self, source: SourceHandle) -> EngineResult<()> {
        let source = self.source_mut(source)?;
        source.state = PlaybackState::Playing;
        source.scheduled_delay_seconds = 0.0;
        Ok(())
    }

    fn play_scheduled(&mut self, source: SourceHandle, delay_seconds: f32) -> EngineResult<()> {
        let source = self.source_mut(source)?;
        source.state = PlaybackState::Playing;
        source.scheduled_delay_seconds = delay_seconds.max(0.0);
        Ok(())
    }

    fn pause(&mut self, source: SourceHandle) -> EngineResult<()> {
        self.source_mut(source)?.state = PlaybackState::Paused;
        Ok(())
    }

    fn stop(&mut self, source: SourceHandle) -> EngineResult<()> {
        let source = self.source_mut(source)?;
        source.state = PlaybackState::Stopped;
        source.cursor_seconds = 0.0;
        Ok(())
    }

    fn set_volume(&mut self, source: SourceHandle, volume: f32) -> EngineResult<()> {
        self.source_mut(source)?.desc.volume = volume.clamp(0.0, 1.0);
        Ok(())
    }

    fn set_pitch(&mut self, source: SourceHandle, pitch: f32) -> EngineResult<()> {
        self.source_mut(source)?.desc.pitch = pitch.max(0.0);
        Ok(())
    }

    fn seek(&mut self, source: SourceHandle, seconds: f32) -> EngineResult<()> {
        let source = self.source_mut(source)?;
        source.cursor_seconds = seconds.max(0.0);
        Ok(())
    }

    fn fade_to(
        &mut self,
        source: SourceHandle,
        volume: f32,
        _duration_seconds: f32,
    ) -> EngineResult<()> {
        self.set_volume(source, volume)
    }

    fn set_looping(&mut self, source: SourceHandle, looping: bool) -> EngineResult<()> {
        self.source_mut(source)?.desc.looping = looping;
        Ok(())
    }

    fn set_source_transform(
        &mut self,
        source: SourceHandle,
        transform: AudioObjectTransform,
    ) -> EngineResult<()> {
        self.source_mut(source)?.desc.position = Some(transform.position);
        Ok(())
    }

    fn set_source_propagation(
        &mut self,
        source: SourceHandle,
        propagation: PropagationFrame,
    ) -> EngineResult<()> {
        self.source_mut(source)?;
        self.propagation.insert(source, propagation.sanitized());
        Ok(())
    }

    fn playback_state(&self, source: SourceHandle) -> EngineResult<PlaybackState> {
        self.sources
            .get(&source)
            .map(|source| source.state)
            .ok_or_else(|| EngineError::invalid_handle("audio source does not exist"))
    }

    fn set_listener(&mut self, desc: &AudioListenerDesc) {
        self.listener = *desc;
    }

    fn capabilities(&self) -> AudioOutputCapabilities {
        AudioOutputCapabilities::default()
    }

    fn diagnostics(&self) -> AudioDiagnostics {
        AudioDiagnostics {
            loaded_clips: self.clips.len().min(u32::MAX as usize) as u32,
            logical_sources: self.sources.len().min(u32::MAX as usize) as u32,
            physical_voices: self
                .sources
                .values()
                .filter(|source| source.state == PlaybackState::Playing)
                .count()
                .min(u32::MAX as usize) as u32,
            acoustics_sources: self
                .propagation
                .values()
                .filter(|frame| {
                    frame.direct_gain < 0.999
                        || frame.reverb_send > 0.0
                        || frame.low_pass_hz < 19_999.0
                })
                .count()
                .min(u32::MAX as usize) as u32,
            ..AudioDiagnostics::default()
        }
    }

    fn update(&mut self, dt: f32) {
        let dt = dt.max(0.0);
        let clip_durations = self
            .clips
            .iter()
            .map(|(handle, clip)| (*handle, clip.info.duration_secs))
            .collect::<HashMap<_, _>>();
        for source in self.sources.values_mut() {
            if source.state != PlaybackState::Playing {
                continue;
            }
            if source.scheduled_delay_seconds > 0.0 {
                source.scheduled_delay_seconds = (source.scheduled_delay_seconds - dt).max(0.0);
                continue;
            }
            source.cursor_seconds += dt * source.desc.pitch.max(0.0);
            let duration = clip_durations
                .get(&source.desc.clip)
                .copied()
                .unwrap_or_default();
            if duration <= f32::EPSILON {
                source.state = PlaybackState::Stopped;
                source.cursor_seconds = 0.0;
            } else if source.cursor_seconds >= duration {
                if source.desc.looping {
                    source.cursor_seconds %= duration;
                } else {
                    source.state = PlaybackState::Stopped;
                    source.cursor_seconds = duration;
                }
            }
        }
    }
}

// ── AudioContext ──────────────────────────────────────────────────────────────

/// Top-level audio context that owns a backend and bus graph.
pub struct AudioContext {
    backend: Box<dyn AudioBackend>,
    /// Audio bus graph for mixing and effects.
    pub bus_graph: AudioBusGraph,
}

impl std::fmt::Debug for AudioContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("AudioContext").finish()
    }
}

impl AudioContext {
    /// Creates an audio context with the given backend.
    pub fn new(backend: impl AudioBackend + 'static) -> Self {
        Self {
            backend: Box::new(backend),
            bus_graph: AudioBusGraph::new(),
        }
    }

    /// Creates an audio context backed by the null backend.
    pub fn null() -> Self {
        Self::new(NullAudioBackend)
    }

    /// Creates an audio context backed by the deterministic memory backend.
    pub fn memory() -> Self {
        Self::new(MemoryAudioBackend::new())
    }

    /// Opens the operating system's default output device.
    #[cfg(feature = "device-output")]
    pub fn device_default() -> EngineResult<Self> {
        Ok(Self::new(DeviceAudioBackend::open_default()?))
    }

    /// Opens the operating system's default output device with explicit latency preferences.
    #[cfg(feature = "device-output")]
    pub fn device_with_settings(settings: AudioOutputSettings) -> EngineResult<Self> {
        Ok(Self::new(DeviceAudioBackend::open_with_settings(settings)?))
    }

    /// Returns a mutable reference to the backend.
    pub fn backend_mut(&mut self) -> &mut dyn AudioBackend {
        self.backend.as_mut()
    }

    /// Returns a shared reference to the backend.
    pub fn backend(&self) -> &dyn AudioBackend {
        self.backend.as_ref()
    }

    /// Advances the audio engine and processes the bus graph.
    pub fn update(&mut self, dt: f32) {
        for (bus, gain) in self.bus_graph.effective_gains() {
            let _ = self.backend.set_bus_gain(&bus, gain);
        }
        self.backend.update(dt);
    }

    /// Returns current output capabilities.
    pub fn capabilities(&self) -> AudioOutputCapabilities {
        self.backend.capabilities()
    }

    /// Returns current audio diagnostics.
    pub fn diagnostics(&self) -> AudioDiagnostics {
        self.backend.diagnostics()
    }

    /// Processes audio through the bus graph with the given sample buffer.
    pub fn process_bus(&mut self, samples: &mut [f32], dt: f32) {
        self.bus_graph.process(samples, dt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_backend_load_clip_returns_error() {
        let mut ctx = AudioContext::null();
        let result = ctx.backend_mut().load_clip("test", &[], 1, 44100);
        assert!(result.is_err());
    }

    #[test]
    fn null_backend_play_pause_stop_are_noops() {
        let mut ctx = AudioContext::null();
        let handle = SourceHandle(0);
        assert!(ctx.backend_mut().play(handle).is_ok());
        assert!(ctx.backend_mut().pause(handle).is_ok());
        assert!(ctx.backend_mut().stop(handle).is_ok());
    }

    #[test]
    fn null_backend_update_does_not_panic() {
        let mut ctx = AudioContext::null();
        ctx.update(1.0 / 60.0);
    }

    #[test]
    fn audio_source_desc_simple_defaults() {
        let desc = AudioSourceDesc::simple(ClipHandle(1));
        assert_eq!(desc.volume, 1.0);
        assert!(!desc.looping);
        assert!(desc.auto_play);
    }

    #[test]
    fn audio_listener_default_faces_negative_z() {
        let listener = AudioListenerDesc::default();
        assert_eq!(listener.forward, Vec3::new(0.0, 0.0, -1.0));
    }

    #[test]
    fn memory_backend_tracks_source_lifecycle() {
        let mut backend = MemoryAudioBackend::new();
        let clip = backend
            .load_clip("tone", &[0.0; 44_100], 1, 44_100)
            .unwrap();
        let source = backend
            .spawn_source(&AudioSourceDesc::simple(clip))
            .unwrap();
        assert_eq!(
            backend.playback_state(source).unwrap(),
            PlaybackState::Playing
        );

        backend.update(2.0);
        assert_eq!(
            backend.playback_state(source).unwrap(),
            PlaybackState::Stopped
        );
    }

    #[test]
    fn decodes_pcm16_wav() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&44_100u32.to_le_bytes());
        bytes.extend_from_slice(&88_200u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&0i16.to_le_bytes());
        bytes.extend_from_slice(&i16::MAX.to_le_bytes());

        let (samples, channels, sample_rate) = decode_audio_bytes("test.wav", &bytes).unwrap();
        assert_eq!(channels, 1);
        assert_eq!(sample_rate, 44_100);
        assert_eq!(samples.len(), 2);
        assert!(samples[1] > 0.99);
    }
}
