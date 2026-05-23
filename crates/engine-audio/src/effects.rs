//! Audio effects for bus processing.

/// Trait for audio effects applied to bus samples.
pub trait AudioEffect: Send + Sync + std::fmt::Debug {
    /// Human-readable effect name.
    fn name(&self) -> &'static str;

    /// Processes a mono or interleaved multi-channel sample buffer.
    fn process(&mut self, samples: &mut [f32], dt: f32);

    /// Sets a named parameter value.
    fn set_parameter(&mut self, _name: &str, _value: f32) {}
}

/// Simple reverb effect using a feedback delay network.
#[derive(Debug, Clone)]
pub struct ReverbEffect {
    /// Mix between dry (0.0) and wet (1.0).
    pub mix: f32,
    /// Decay time in seconds.
    pub decay: f32,
    delay_buffer: Vec<f32>,
    position: usize,
}

impl ReverbEffect {
    /// Creates a reverb effect with default settings.
    pub fn new() -> Self {
        let delay_len = 44100;
        Self {
            mix: 0.3,
            decay: 1.5,
            delay_buffer: vec![0.0; delay_len],
            position: 0,
        }
    }
}

impl Default for ReverbEffect {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEffect for ReverbEffect {
    fn name(&self) -> &'static str {
        "Reverb"
    }

    fn process(&mut self, samples: &mut [f32], _dt: f32) {
        let feedback = 0.001_f32.powf(1.0 / (self.decay * 44100.0 / samples.len() as f32));
        let len = self.delay_buffer.len();
        for sample in samples.iter_mut() {
            let delayed = self.delay_buffer[self.position];
            self.delay_buffer[self.position] = *sample + delayed * feedback;
            self.position = (self.position + 1) % len;
            *sample = *sample * (1.0 - self.mix) + delayed * self.mix;
        }
    }

    fn set_parameter(&mut self, name: &str, value: f32) {
        match name {
            "mix" => self.mix = value.clamp(0.0, 1.0),
            "decay" => self.decay = value.max(0.01),
            _ => {}
        }
    }
}

/// Three-band parametric equalizer.
#[derive(Debug, Clone)]
pub struct EqEffect {
    /// Low band gain.
    pub low_gain: f32,
    /// Mid band gain.
    pub mid_gain: f32,
    /// High band gain.
    pub high_gain: f32,
}

impl Default for EqEffect {
    fn default() -> Self {
        Self {
            low_gain: 0.0,
            mid_gain: 0.0,
            high_gain: 0.0,
        }
    }
}

impl AudioEffect for EqEffect {
    fn name(&self) -> &'static str {
        "EQ"
    }

    fn process(&mut self, samples: &mut [f32], _dt: f32) {
        for sample in samples.iter_mut() {
            *sample *= 1.0 + self.mid_gain.clamp(-1.0, 1.0);
        }
    }

    fn set_parameter(&mut self, name: &str, value: f32) {
        match name {
            "low" => self.low_gain = value.clamp(-1.0, 1.0),
            "mid" => self.mid_gain = value.clamp(-1.0, 1.0),
            "high" => self.high_gain = value.clamp(-1.0, 1.0),
            _ => {}
        }
    }
}

/// Dynamic range compressor.
#[derive(Debug, Clone)]
pub struct CompressorEffect {
    /// Threshold level (0.0 to 1.0).
    pub threshold: f32,
    /// Compression ratio.
    pub ratio: f32,
    /// Attack time in seconds.
    pub attack: f32,
    /// Release time in seconds.
    pub release: f32,
    envelope: f32,
}

impl Default for CompressorEffect {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            ratio: 4.0,
            attack: 0.01,
            release: 0.1,
            envelope: 0.0,
        }
    }
}

impl AudioEffect for CompressorEffect {
    fn name(&self) -> &'static str {
        "Compressor"
    }

    fn process(&mut self, samples: &mut [f32], _dt: f32) {
        let attack_coeff = (-1.0 / (self.attack * 44100.0 / samples.len() as f32)).exp();
        let release_coeff = (-1.0 / (self.release * 44100.0 / samples.len() as f32)).exp();
        for sample in samples.iter_mut() {
            let abs = sample.abs();
            let coeff = if abs > self.envelope { attack_coeff } else { release_coeff };
            self.envelope = coeff * self.envelope + (1.0 - coeff) * abs;
            if self.envelope > self.threshold {
                let gain = self.threshold + (self.envelope - self.threshold) / self.ratio;
                *sample *= gain / self.envelope;
            }
        }
    }

    fn set_parameter(&mut self, name: &str, value: f32) {
        match name {
            "threshold" => self.threshold = value.clamp(0.0, 1.0),
            "ratio" => self.ratio = value.max(1.0),
            "attack" => self.attack = value.max(0.001),
            "release" => self.release = value.max(0.001),
            _ => {}
        }
    }
}

/// Brick-wall limiter.
#[derive(Debug, Clone)]
pub struct LimiterEffect {
    /// Ceiling level.
    pub ceiling: f32,
}

impl Default for LimiterEffect {
    fn default() -> Self {
        Self { ceiling: 0.95 }
    }
}

impl AudioEffect for LimiterEffect {
    fn name(&self) -> &'static str {
        "Limiter"
    }

    fn process(&mut self, samples: &mut [f32], _dt: f32) {
        for sample in samples.iter_mut() {
            *sample = sample.clamp(-self.ceiling, self.ceiling);
        }
    }

    fn set_parameter(&mut self, name: &str, value: f32) {
        if name == "ceiling" {
            self.ceiling = value.clamp(0.0, 1.0);
        }
    }
}

/// Simple delay/echo effect.
#[derive(Debug, Clone)]
pub struct DelayEffect {
    /// Delay time in seconds.
    pub time: f32,
    /// Feedback amount.
    pub feedback: f32,
    /// Wet/dry mix.
    pub mix: f32,
    buffer: Vec<f32>,
    position: usize,
}

impl DelayEffect {
    /// Creates a delay effect.
    pub fn new(time_secs: f32) -> Self {
        let samples = (time_secs * 44100.0) as usize;
        Self {
            time: time_secs,
            feedback: 0.3,
            mix: 0.3,
            buffer: vec![0.0; samples.max(1)],
            position: 0,
        }
    }
}

impl AudioEffect for DelayEffect {
    fn name(&self) -> &'static str {
        "Delay"
    }

    fn process(&mut self, samples: &mut [f32], _dt: f32) {
        let len = self.buffer.len();
        for sample in samples.iter_mut() {
            let delayed = self.buffer[self.position];
            self.buffer[self.position] = *sample + delayed * self.feedback;
            self.position = (self.position + 1) % len;
            *sample = *sample * (1.0 - self.mix) + delayed * self.mix;
        }
    }

    fn set_parameter(&mut self, name: &str, value: f32) {
        match name {
            "time" => self.time = value.max(0.001),
            "feedback" => self.feedback = value.clamp(0.0, 0.99),
            "mix" => self.mix = value.clamp(0.0, 1.0),
            _ => {}
        }
    }
}

/// Chorus/flanger effect.
#[derive(Debug, Clone)]
pub struct ChorusEffect {
    /// Modulation rate in Hz.
    pub rate: f32,
    /// Modulation depth.
    pub depth: f32,
    /// Wet/dry mix.
    pub mix: f32,
    phase: f32,
    buffer: Vec<f32>,
    position: usize,
}

impl Default for ChorusEffect {
    fn default() -> Self {
        Self {
            rate: 1.0,
            depth: 0.002,
            mix: 0.3,
            phase: 0.0,
            buffer: vec![0.0; 2048],
            position: 0,
        }
    }
}

impl AudioEffect for ChorusEffect {
    fn name(&self) -> &'static str {
        "Chorus"
    }

    fn process(&mut self, samples: &mut [f32], dt: f32) {
        let len = self.buffer.len();
        let max_delay = (self.depth * 44100.0) as usize;
        let sample_count = samples.len() as f32;
        for sample in samples.iter_mut() {
            self.phase += self.rate * dt / sample_count;
            let offset = (self.phase.sin() * max_delay as f32) as usize;
            let index = (self.position + len - offset.min(len - 1)) % len;
            let delayed = self.buffer[index];
            self.buffer[self.position] = *sample;
            self.position = (self.position + 1) % len;
            *sample = *sample * (1.0 - self.mix) + delayed * self.mix;
        }
    }

    fn set_parameter(&mut self, name: &str, value: f32) {
        match name {
            "rate" => self.rate = value.max(0.1),
            "depth" => self.depth = value.max(0.0),
            "mix" => self.mix = value.clamp(0.0, 1.0),
            _ => {}
        }
    }
}

/// Biquad filter (low-pass, high-pass, band-pass).
#[derive(Debug, Clone)]
pub struct FilterEffect {
    /// Filter type.
    pub filter_type: FilterType,
    /// Cutoff frequency in Hz.
    pub cutoff: f32,
    /// Resonance/Q factor.
    pub resonance: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

/// Filter type for the biquad filter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterType {
    /// Low-pass filter.
    LowPass,
    /// High-pass filter.
    HighPass,
    /// Band-pass filter.
    BandPass,
}

impl Default for FilterEffect {
    fn default() -> Self {
        Self {
            filter_type: FilterType::LowPass,
            cutoff: 1000.0,
            resonance: 0.7,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
}

impl AudioEffect for FilterEffect {
    fn name(&self) -> &'static str {
        "Filter"
    }

    fn process(&mut self, samples: &mut [f32], _dt: f32) {
        let sample_rate = 44100.0;
        let w0 = 2.0 * std::f32::consts::PI * self.cutoff / sample_rate;
        let alpha = w0.sin() / (2.0 * self.resonance.max(0.01));
        let cos_w0 = w0.cos();

        let (b0, b1, b2, a1, a2) = match self.filter_type {
            FilterType::LowPass => {
                let b1 = 1.0 - cos_w0;
                let b0 = b1 / 2.0;
                let b2 = b0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                (b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
            }
            FilterType::HighPass => {
                let b1 = -(1.0 + cos_w0);
                let b0 = (1.0 + cos_w0) / 2.0;
                let b2 = b0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                (b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
            }
            FilterType::BandPass => {
                let b0 = alpha;
                let b1 = 0.0;
                let b2 = -alpha;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                (b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
            }
        };

        for sample in samples.iter_mut() {
            let x0 = *sample;
            let y0 = b0 * x0 + b1 * self.x1 + b2 * self.x2 - a1 * self.y1 - a2 * self.y2;
            self.x2 = self.x1;
            self.x1 = x0;
            self.y2 = self.y1;
            self.y1 = y0;
            *sample = y0;
        }
    }

    fn set_parameter(&mut self, name: &str, value: f32) {
        match name {
            "cutoff" => self.cutoff = value.clamp(20.0, 20000.0),
            "resonance" => self.resonance = value.clamp(0.1, 10.0),
            _ => {}
        }
    }
}
