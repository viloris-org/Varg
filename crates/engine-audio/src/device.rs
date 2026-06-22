//! CPAL-backed production PCM output.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use engine_core::{EngineError, EngineResult};

use crate::renderer::{AudioCommand, AudioEvent, AudioRenderer, PcmClip};
use crate::stream::{AudioStreamPoll, AudioStreamReader};
use crate::{
    AudioBackend, AudioClipInfo, AudioDiagnostics, AudioListenerDesc, AudioObjectTransform,
    AudioOutputCapabilities, AudioOutputSettings, AudioRendererConfig, AudioSourceDesc, ClipHandle,
    HrtfQuality, OutputMode, PlaybackState, PropagationFrame, SourceHandle,
};

const COMMAND_CAPACITY: usize = 4096;

#[derive(Default)]
struct SharedDiagnostics {
    loaded_clips: AtomicU32,
    logical_sources: AtomicU32,
    physical_voices: AtomicU32,
    virtual_voices: AtomicU32,
    hrtf_objects: AtomicU32,
    hrtf_fallback_objects: AtomicU32,
    acoustics_sources: AtomicU32,
    underruns: AtomicU64,
    output_peak_bits: AtomicU32,
    rendered_frames: AtomicU64,
}

impl SharedDiagnostics {
    fn publish(&self, diagnostics: AudioDiagnostics) {
        self.loaded_clips
            .store(diagnostics.loaded_clips, Ordering::Relaxed);
        self.logical_sources
            .store(diagnostics.logical_sources, Ordering::Relaxed);
        self.physical_voices
            .store(diagnostics.physical_voices, Ordering::Relaxed);
        self.virtual_voices
            .store(diagnostics.virtual_voices, Ordering::Relaxed);
        self.hrtf_objects
            .store(diagnostics.hrtf_objects, Ordering::Relaxed);
        self.hrtf_fallback_objects
            .store(diagnostics.hrtf_fallback_objects, Ordering::Relaxed);
        self.acoustics_sources
            .store(diagnostics.acoustics_sources, Ordering::Relaxed);
        self.output_peak_bits
            .store(diagnostics.output_peak.to_bits(), Ordering::Relaxed);
        self.rendered_frames
            .store(diagnostics.rendered_frames, Ordering::Relaxed);
    }

    fn snapshot(&self) -> AudioDiagnostics {
        AudioDiagnostics {
            loaded_clips: self.loaded_clips.load(Ordering::Relaxed),
            logical_sources: self.logical_sources.load(Ordering::Relaxed),
            physical_voices: self.physical_voices.load(Ordering::Relaxed),
            virtual_voices: self.virtual_voices.load(Ordering::Relaxed),
            hrtf_objects: self.hrtf_objects.load(Ordering::Relaxed),
            hrtf_fallback_objects: self.hrtf_fallback_objects.load(Ordering::Relaxed),
            acoustics_sources: self.acoustics_sources.load(Ordering::Relaxed),
            underruns: self.underruns.load(Ordering::Relaxed),
            output_peak: f32::from_bits(self.output_peak_bits.load(Ordering::Relaxed)),
            rendered_frames: self.rendered_frames.load(Ordering::Relaxed),
        }
    }
}

/// Production output backend using the operating system's default CPAL device.
pub struct DeviceAudioBackend {
    _stream: cpal::Stream,
    commands: SyncSender<AudioCommand>,
    events: Receiver<AudioEvent>,
    next_clip: u64,
    next_source: u64,
    clips: HashMap<ClipHandle, DeviceClip>,
    sources: HashMap<SourceHandle, DeviceSource>,
    streams: HashMap<ClipHandle, AudioStreamReader>,
    capabilities: AudioOutputCapabilities,
    diagnostics: Arc<SharedDiagnostics>,
    observed_backend_errors: u64,
}

#[derive(Clone)]
struct DeviceClip {
    info: AudioClipInfo,
    pcm: PcmClip,
    encoded_stream: Option<(String, Arc<[u8]>)>,
}

#[derive(Clone)]
struct DeviceSource {
    desc: AudioSourceDesc,
    state: PlaybackState,
    transform: AudioObjectTransform,
}

impl std::fmt::Debug for DeviceAudioBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeviceAudioBackend")
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

impl DeviceAudioBackend {
    /// Opens the default output device and starts its real-time stream.
    pub fn open_default() -> EngineResult<Self> {
        Self::open_with_settings(AudioOutputSettings::default())
    }

    /// Opens the default output device with explicit latency preferences.
    pub fn open_with_settings(settings: AudioOutputSettings) -> EngineResult<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| EngineError::other("no default audio output device"))?;
        let device_name = device
            .name()
            .unwrap_or_else(|_| "default output".to_string());
        let supported = device.default_output_config().map_err(|error| {
            EngineError::other(format!("query default audio output failed: {error}"))
        })?;
        let sample_format = supported.sample_format();
        let stream_configs =
            stream_config_candidates(&supported.config(), supported.buffer_size(), settings);
        let diagnostics = Arc::new(SharedDiagnostics::default());
        let opened = open_stream_from_candidates(
            &device,
            sample_format,
            &stream_configs,
            Arc::clone(&diagnostics),
        )?;
        let preferred_block_frames = match opened.stream_config.buffer_size {
            cpal::BufferSize::Fixed(frames) => Some(frames),
            cpal::BufferSize::Default => None,
        };
        let capabilities = AudioOutputCapabilities {
            device_name,
            sample_rate: opened.stream_config.sample_rate.0,
            channels: opened.stream_config.channels,
            preferred_block_frames,
            latency_profile: settings.latency_profile,
            estimated_latency_micros: preferred_block_frames
                .map(|frames| latency_micros(frames, opened.stream_config.sample_rate.0)),
            platform_spatial_audio: false,
            max_dynamic_objects: 0,
            output_mode: OutputMode::Stereo,
            hrtf_quality: HrtfQuality::Medium,
        };
        opened
            .stream
            .play()
            .map_err(|error| EngineError::other(format!("start audio output failed: {error}")))?;
        Ok(Self {
            _stream: opened.stream,
            commands: opened.commands,
            events: opened.events,
            next_clip: 1,
            next_source: 1,
            clips: HashMap::new(),
            sources: HashMap::new(),
            streams: HashMap::new(),
            capabilities,
            diagnostics,
            observed_backend_errors: 0,
        })
    }

    fn send(&self, command: AudioCommand) -> EngineResult<()> {
        match self.commands.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(EngineError::other("audio command queue is full")),
            Err(TrySendError::Disconnected(_)) => {
                Err(EngineError::other("audio output stream is disconnected"))
            }
        }
    }

    fn reopened(&self) -> EngineResult<Self> {
        let mut replacement = Self::open_with_settings(AudioOutputSettings {
            latency_profile: self.capabilities.latency_profile,
            preferred_buffer_frames: self.capabilities.preferred_block_frames,
        })?;
        replacement.next_clip = self.next_clip;
        replacement.next_source = self.next_source;
        for (handle, clip) in &self.clips {
            if let Some((name, bytes)) = &clip.encoded_stream {
                replacement.install_stream(*handle, name, Arc::clone(bytes))?;
            } else {
                replacement.send(AudioCommand::LoadClip {
                    handle: *handle,
                    clip: clip.pcm.clone(),
                })?;
                replacement.clips.insert(*handle, clip.clone());
            }
        }
        for (handle, source) in &self.sources {
            replacement.send(AudioCommand::SpawnSource {
                handle: *handle,
                desc: source.desc.clone(),
            })?;
            replacement.send(AudioCommand::SetSourceTransform {
                handle: *handle,
                transform: source.transform,
            })?;
            replacement.send(AudioCommand::SetPlayback {
                handle: *handle,
                state: source.state,
            })?;
            replacement.sources.insert(*handle, source.clone());
        }
        Ok(replacement)
    }

    fn install_stream(
        &mut self,
        handle: ClipHandle,
        name: &str,
        bytes: Arc<[u8]>,
    ) -> EngineResult<()> {
        let mut reader = AudioStreamReader::spawn(name, Arc::clone(&bytes), 8)?;
        let first = reader
            .next_block()?
            .ok_or_else(|| EngineError::other("streaming audio contains no samples"))?;
        let clip = PcmClip::streaming(first.channels, first.sample_rate)
            .ok_or_else(|| EngineError::other("invalid streaming audio format"))?;
        self.send(AudioCommand::LoadClip {
            handle,
            clip: clip.clone(),
        })?;
        self.send(AudioCommand::AppendStream {
            handle,
            samples: first.samples,
        })?;
        self.clips.insert(
            handle,
            DeviceClip {
                info: AudioClipInfo {
                    name: name.to_string(),
                    duration_secs: 0.0,
                    channels: first.channels,
                    sample_rate: first.sample_rate,
                },
                pcm: clip,
                encoded_stream: Some((name.to_string(), bytes)),
            },
        );
        self.streams.insert(handle, reader);
        Ok(())
    }
}

fn stream_config_candidates(
    default_config: &cpal::StreamConfig,
    supported_buffer_size: &cpal::SupportedBufferSize,
    settings: AudioOutputSettings,
) -> Vec<cpal::StreamConfig> {
    let mut configs = Vec::new();
    for frames in settings.buffer_frame_candidates() {
        if !buffer_size_supported(frames, supported_buffer_size) {
            continue;
        }
        let mut config = default_config.clone();
        config.buffer_size = cpal::BufferSize::Fixed(frames);
        if !configs.contains(&config) {
            configs.push(config);
        }
    }
    if !configs.contains(default_config) {
        configs.push(default_config.clone());
    }
    configs
}

fn buffer_size_supported(frames: u32, supported: &cpal::SupportedBufferSize) -> bool {
    match *supported {
        cpal::SupportedBufferSize::Range { min, max } => (min..=max).contains(&frames),
        cpal::SupportedBufferSize::Unknown => true,
    }
}

fn latency_micros(frames: u32, sample_rate: u32) -> u32 {
    if sample_rate == 0 {
        return 0;
    }
    ((frames as u64 * 1_000_000) / sample_rate as u64).min(u32::MAX as u64) as u32
}

struct OpenedStream {
    stream: cpal::Stream,
    stream_config: cpal::StreamConfig,
    commands: SyncSender<AudioCommand>,
    events: Receiver<AudioEvent>,
}

fn open_stream_from_candidates(
    device: &cpal::Device,
    sample_format: cpal::SampleFormat,
    configs: &[cpal::StreamConfig],
    diagnostics: Arc<SharedDiagnostics>,
) -> EngineResult<OpenedStream> {
    let mut last_error = None;
    for config in configs {
        let (commands, receiver) = sync_channel(COMMAND_CAPACITY);
        let (event_sender, events) = sync_channel(COMMAND_CAPACITY);
        let error_diagnostics = Arc::clone(&diagnostics);
        let error_callback = move |_error: cpal::StreamError| {
            error_diagnostics.underruns.fetch_add(1, Ordering::Relaxed);
        };
        let renderer_config = AudioRendererConfig {
            sample_rate: config.sample_rate.0,
            channels: config.channels,
            ..AudioRendererConfig::default()
        };
        let result = match sample_format {
            cpal::SampleFormat::F32 => build_stream_f32(
                device,
                config,
                receiver,
                renderer_config,
                Arc::clone(&diagnostics),
                event_sender,
                error_callback,
            ),
            cpal::SampleFormat::I16 => build_stream_i16(
                device,
                config,
                receiver,
                renderer_config,
                Arc::clone(&diagnostics),
                event_sender,
                error_callback,
            ),
            cpal::SampleFormat::U16 => build_stream_u16(
                device,
                config,
                receiver,
                renderer_config,
                Arc::clone(&diagnostics),
                event_sender,
                error_callback,
            ),
            format => Err(EngineError::other(format!(
                "unsupported output sample format: {format:?}"
            ))),
        };
        match result {
            Ok(stream) => {
                return Ok(OpenedStream {
                    stream,
                    stream_config: config.clone(),
                    commands,
                    events,
                });
            }
            Err(error) => {
                last_error = Some(error);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| EngineError::other("no audio stream configs available")))
}

impl AudioBackend for DeviceAudioBackend {
    fn load_clip(
        &mut self,
        name: &str,
        samples: &[f32],
        channels: u16,
        sample_rate: u32,
    ) -> EngineResult<ClipHandle> {
        let clip = PcmClip::new(Arc::from(samples), channels, sample_rate)
            .ok_or_else(|| EngineError::other("audio clip must have channels and sample rate"))?;
        let handle = ClipHandle(self.next_clip);
        self.next_clip = self.next_clip.saturating_add(1).max(1);
        let info = AudioClipInfo {
            name: name.to_string(),
            duration_secs: samples.len() as f32 / channels as f32 / sample_rate as f32,
            channels,
            sample_rate,
        };
        self.send(AudioCommand::LoadClip {
            handle,
            clip: clip.clone(),
        })?;
        self.clips.insert(
            handle,
            DeviceClip {
                info,
                pcm: clip,
                encoded_stream: None,
            },
        );
        Ok(handle)
    }

    fn load_streamed_clip(&mut self, name: &str, bytes: Arc<[u8]>) -> EngineResult<ClipHandle> {
        let handle = ClipHandle(self.next_clip);
        self.next_clip = self.next_clip.saturating_add(1).max(1);
        self.install_stream(handle, name, bytes)?;
        Ok(handle)
    }

    fn unload_clip(&mut self, clip: ClipHandle) -> EngineResult<()> {
        self.clips
            .remove(&clip)
            .ok_or_else(|| EngineError::invalid_handle("audio clip does not exist"))?;
        self.sources.retain(|_, source| source.desc.clip != clip);
        self.streams.remove(&clip);
        self.send(AudioCommand::UnloadClip { handle: clip })
    }

    fn clip_info(&self, clip: ClipHandle) -> EngineResult<AudioClipInfo> {
        self.clips
            .get(&clip)
            .map(|clip| clip.info.clone())
            .ok_or_else(|| EngineError::invalid_handle("audio clip does not exist"))
    }

    fn spawn_source(&mut self, desc: &AudioSourceDesc) -> EngineResult<SourceHandle> {
        if !self.clips.contains_key(&desc.clip) {
            return Err(EngineError::invalid_handle("audio clip does not exist"));
        }
        let handle = SourceHandle(self.next_source);
        self.next_source = self.next_source.saturating_add(1).max(1);
        self.send(AudioCommand::SpawnSource {
            handle,
            desc: desc.clone(),
        })?;
        self.sources.insert(
            handle,
            DeviceSource {
                desc: desc.clone(),
                state: if desc.auto_play {
                    PlaybackState::Playing
                } else {
                    PlaybackState::Stopped
                },
                transform: AudioObjectTransform {
                    position: desc.position.unwrap_or_default(),
                    ..AudioObjectTransform::default()
                },
            },
        );
        Ok(handle)
    }

    fn destroy_source(&mut self, source: SourceHandle) -> EngineResult<()> {
        self.sources
            .remove(&source)
            .ok_or_else(|| EngineError::invalid_handle("audio source does not exist"))?;
        self.send(AudioCommand::DestroySource { handle: source })
    }

    fn play(&mut self, source: SourceHandle) -> EngineResult<()> {
        self.sources
            .get_mut(&source)
            .ok_or_else(|| EngineError::invalid_handle("audio source does not exist"))?
            .state = PlaybackState::Playing;
        self.send(AudioCommand::SetPlayback {
            handle: source,
            state: PlaybackState::Playing,
        })
    }

    fn play_scheduled(&mut self, source: SourceHandle, delay_seconds: f32) -> EngineResult<()> {
        self.sources
            .get_mut(&source)
            .ok_or_else(|| EngineError::invalid_handle("audio source does not exist"))?
            .state = PlaybackState::Playing;
        self.send(AudioCommand::SchedulePlay {
            handle: source,
            delay_frames: (delay_seconds.max(0.0) * self.capabilities.sample_rate as f32) as u64,
        })
    }

    fn pause(&mut self, source: SourceHandle) -> EngineResult<()> {
        self.sources
            .get_mut(&source)
            .ok_or_else(|| EngineError::invalid_handle("audio source does not exist"))?
            .state = PlaybackState::Paused;
        self.send(AudioCommand::SetPlayback {
            handle: source,
            state: PlaybackState::Paused,
        })
    }

    fn stop(&mut self, source: SourceHandle) -> EngineResult<()> {
        self.sources
            .get_mut(&source)
            .ok_or_else(|| EngineError::invalid_handle("audio source does not exist"))?
            .state = PlaybackState::Stopped;
        self.send(AudioCommand::SetPlayback {
            handle: source,
            state: PlaybackState::Stopped,
        })
    }

    fn set_volume(&mut self, source: SourceHandle, volume: f32) -> EngineResult<()> {
        if !self.sources.contains_key(&source) {
            return Err(EngineError::invalid_handle("audio source does not exist"));
        }
        self.sources.get_mut(&source).unwrap().desc.volume = volume.clamp(0.0, 1.0);
        self.send(AudioCommand::SetVolume {
            handle: source,
            volume,
        })
    }

    fn set_pitch(&mut self, source: SourceHandle, pitch: f32) -> EngineResult<()> {
        let state = self
            .sources
            .get_mut(&source)
            .ok_or_else(|| EngineError::invalid_handle("audio source does not exist"))?;
        state.desc.pitch = pitch.max(0.0);
        self.send(AudioCommand::SetPitch {
            handle: source,
            pitch,
        })
    }

    fn seek(&mut self, source: SourceHandle, seconds: f32) -> EngineResult<()> {
        if !self.sources.contains_key(&source) {
            return Err(EngineError::invalid_handle("audio source does not exist"));
        }
        self.send(AudioCommand::Seek {
            handle: source,
            seconds,
        })
    }

    fn fade_to(
        &mut self,
        source: SourceHandle,
        volume: f32,
        duration_seconds: f32,
    ) -> EngineResult<()> {
        let state = self
            .sources
            .get_mut(&source)
            .ok_or_else(|| EngineError::invalid_handle("audio source does not exist"))?;
        state.desc.volume = volume.clamp(0.0, 1.0);
        self.send(AudioCommand::FadeTo {
            handle: source,
            volume,
            duration_seconds,
        })
    }

    fn set_looping(&mut self, source: SourceHandle, looping: bool) -> EngineResult<()> {
        if !self.sources.contains_key(&source) {
            return Err(EngineError::invalid_handle("audio source does not exist"));
        }
        self.sources.get_mut(&source).unwrap().desc.looping = looping;
        self.send(AudioCommand::SetLooping {
            handle: source,
            looping,
        })
    }

    fn playback_state(&self, source: SourceHandle) -> EngineResult<PlaybackState> {
        self.sources
            .get(&source)
            .map(|source| source.state)
            .ok_or_else(|| EngineError::invalid_handle("audio source does not exist"))
    }

    fn set_source_transform(
        &mut self,
        source: SourceHandle,
        transform: AudioObjectTransform,
    ) -> EngineResult<()> {
        if !self.sources.contains_key(&source) {
            return Err(EngineError::invalid_handle("audio source does not exist"));
        }
        self.sources.get_mut(&source).unwrap().transform = transform;
        self.send(AudioCommand::SetSourceTransform {
            handle: source,
            transform,
        })
    }

    fn set_source_propagation(
        &mut self,
        source: SourceHandle,
        propagation: PropagationFrame,
    ) -> EngineResult<()> {
        if !self.sources.contains_key(&source) {
            return Err(EngineError::invalid_handle("audio source does not exist"));
        }
        self.send(AudioCommand::SetSourcePropagation {
            handle: source,
            propagation,
        })
    }

    fn set_listener(&mut self, desc: &AudioListenerDesc) {
        let _ = self.send(AudioCommand::SetListener { listener: *desc });
    }

    fn capabilities(&self) -> AudioOutputCapabilities {
        self.capabilities.clone()
    }

    fn diagnostics(&self) -> AudioDiagnostics {
        self.diagnostics.snapshot()
    }

    fn set_bus_gain(&mut self, bus: &str, gain: f32) -> EngineResult<()> {
        self.send(AudioCommand::SetBusGain {
            bus: bus.to_string(),
            gain,
        })
    }

    fn update(&mut self, _dt: f32) {
        while let Ok(AudioEvent::SourceFinished(source)) = self.events.try_recv() {
            if let Some(state) = self.sources.get_mut(&source) {
                state.state = PlaybackState::Stopped;
            }
        }
        let mut stream_commands = Vec::new();
        let mut completed_streams = Vec::new();
        for (handle, stream) in &mut self.streams {
            for _ in 0..8 {
                match stream.try_next_block() {
                    Ok(AudioStreamPoll::Block(block)) => {
                        stream_commands.push(AudioCommand::AppendStream {
                            handle: *handle,
                            samples: block.samples,
                        });
                    }
                    Ok(AudioStreamPoll::Pending) => break,
                    Ok(AudioStreamPoll::End) => {
                        stream_commands.push(AudioCommand::EndStream { handle: *handle });
                        completed_streams.push(*handle);
                        break;
                    }
                    Err(_) => {
                        self.diagnostics.underruns.fetch_add(1, Ordering::Relaxed);
                        stream_commands.push(AudioCommand::EndStream { handle: *handle });
                        completed_streams.push(*handle);
                        break;
                    }
                }
            }
        }
        for command in stream_commands {
            let _ = self.send(command);
        }
        for handle in completed_streams {
            self.streams.remove(&handle);
        }
        let backend_errors = self.diagnostics.underruns.load(Ordering::Relaxed);
        if backend_errors > self.observed_backend_errors {
            self.observed_backend_errors = backend_errors;
            if let Ok(replacement) = self.reopened() {
                *self = replacement;
            }
        }
    }
}

fn drain_commands(receiver: &Receiver<AudioCommand>, renderer: &mut AudioRenderer) {
    while let Ok(command) = receiver.try_recv() {
        renderer.apply(command);
    }
}

fn build_stream_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    receiver: Receiver<AudioCommand>,
    renderer_config: AudioRendererConfig,
    diagnostics: Arc<SharedDiagnostics>,
    events: SyncSender<AudioEvent>,
    error_callback: impl FnMut(cpal::StreamError) + Send + 'static,
) -> EngineResult<cpal::Stream> {
    let mut renderer = AudioRenderer::new(renderer_config);
    device
        .build_output_stream(
            config,
            move |output: &mut [f32], _| {
                drain_commands(&receiver, &mut renderer);
                renderer.render(output);
                renderer.drain_events(|event| {
                    let _ = events.try_send(event);
                });
                diagnostics.publish(renderer.diagnostics());
            },
            error_callback,
            None,
        )
        .map_err(|error| EngineError::other(format!("create audio output failed: {error}")))
}

fn build_stream_i16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    receiver: Receiver<AudioCommand>,
    renderer_config: AudioRendererConfig,
    diagnostics: Arc<SharedDiagnostics>,
    events: SyncSender<AudioEvent>,
    error_callback: impl FnMut(cpal::StreamError) + Send + 'static,
) -> EngineResult<cpal::Stream> {
    let mut renderer = AudioRenderer::new(renderer_config);
    let mut scratch = vec![0.0_f32; 16_384];
    device
        .build_output_stream(
            config,
            move |output: &mut [i16], _| {
                drain_commands(&receiver, &mut renderer);
                for output_chunk in output.chunks_mut(scratch.len()) {
                    let scratch_chunk = &mut scratch[..output_chunk.len()];
                    renderer.render(scratch_chunk);
                    renderer.drain_events(|event| {
                        let _ = events.try_send(event);
                    });
                    for (target, sample) in output_chunk.iter_mut().zip(scratch_chunk) {
                        *target = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    }
                }
                diagnostics.publish(renderer.diagnostics());
            },
            error_callback,
            None,
        )
        .map_err(|error| EngineError::other(format!("create audio output failed: {error}")))
}

fn build_stream_u16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    receiver: Receiver<AudioCommand>,
    renderer_config: AudioRendererConfig,
    diagnostics: Arc<SharedDiagnostics>,
    events: SyncSender<AudioEvent>,
    error_callback: impl FnMut(cpal::StreamError) + Send + 'static,
) -> EngineResult<cpal::Stream> {
    let mut renderer = AudioRenderer::new(renderer_config);
    let mut scratch = vec![0.0_f32; 16_384];
    device
        .build_output_stream(
            config,
            move |output: &mut [u16], _| {
                drain_commands(&receiver, &mut renderer);
                for output_chunk in output.chunks_mut(scratch.len()) {
                    let scratch_chunk = &mut scratch[..output_chunk.len()];
                    renderer.render(scratch_chunk);
                    renderer.drain_events(|event| {
                        let _ = events.try_send(event);
                    });
                    for (target, sample) in output_chunk.iter_mut().zip(scratch_chunk) {
                        *target = ((sample.clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16;
                    }
                }
                diagnostics.publish(renderer.diagnostics());
            },
            error_callback,
            None,
        )
        .map_err(|error| EngineError::other(format!("create audio output failed: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> cpal::StreamConfig {
        cpal::StreamConfig {
            channels: 2,
            sample_rate: cpal::SampleRate(48_000),
            buffer_size: cpal::BufferSize::Default,
        }
    }

    #[test]
    fn default_latency_profile_keeps_platform_default_config() {
        let configs = stream_config_candidates(
            &default_config(),
            &cpal::SupportedBufferSize::Range { min: 64, max: 512 },
            AudioOutputSettings::default(),
        );

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].buffer_size, cpal::BufferSize::Default);
    }

    #[test]
    fn critical_latency_profile_prefers_small_supported_buffers_before_default() {
        let configs = stream_config_candidates(
            &default_config(),
            &cpal::SupportedBufferSize::Range { min: 128, max: 512 },
            AudioOutputSettings {
                latency_profile: crate::AudioLatencyProfile::Critical,
                preferred_buffer_frames: None,
            },
        );

        let buffer_sizes: Vec<_> = configs.iter().map(|config| config.buffer_size).collect();
        assert_eq!(
            buffer_sizes,
            vec![
                cpal::BufferSize::Fixed(256),
                cpal::BufferSize::Fixed(128),
                cpal::BufferSize::Fixed(512),
                cpal::BufferSize::Default,
            ]
        );
    }

    #[test]
    fn preferred_buffer_frames_are_deduplicated() {
        let configs = stream_config_candidates(
            &default_config(),
            &cpal::SupportedBufferSize::Range { min: 128, max: 512 },
            AudioOutputSettings {
                latency_profile: crate::AudioLatencyProfile::Critical,
                preferred_buffer_frames: Some(128),
            },
        );

        let fixed_128_count = configs
            .iter()
            .filter(|config| config.buffer_size == cpal::BufferSize::Fixed(128))
            .count();
        assert_eq!(fixed_128_count, 1);
    }

    #[test]
    fn latency_estimate_uses_frames_over_sample_rate() {
        assert_eq!(latency_micros(128, 48_000), 2_666);
        assert_eq!(latency_micros(0, 48_000), 0);
        assert_eq!(latency_micros(128, 0), 0);
    }
}
