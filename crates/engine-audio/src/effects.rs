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

/// Three-band equalizer using cascade biquad filters.
///
/// Low shelf (cutoff ~300 Hz), peaking (center ~1000 Hz), and high shelf
/// (cutoff ~5000 Hz). Each band has independent gain in [-1.0, 1.0].
#[derive(Debug, Clone)]
pub struct EqEffect {
    /// Low band gain.
    pub low_gain: f32,
    /// Mid band gain.
    pub mid_gain: f32,
    /// High band gain.
    pub high_gain: f32,
    // Biquad states for the three bands
    low_x1: f32,
    low_x2: f32,
    low_y1: f32,
    low_y2: f32,
    mid_x1: f32,
    mid_x2: f32,
    mid_y1: f32,
    mid_y2: f32,
    high_x1: f32,
    high_x2: f32,
    high_y1: f32,
    high_y2: f32,
    // Cached coefficients; recomputed in process() when sample rate context changes
    cached_coeffs: Option<EqCoefficients>,
}

#[derive(Debug, Clone, Copy)]
struct EqCoefficients {
    // Low shelf
    low_b0: f32,
    low_b1: f32,
    low_b2: f32,
    low_a1: f32,
    low_a2: f32,
    // Peaking
    mid_b0: f32,
    mid_b1: f32,
    mid_b2: f32,
    mid_a1: f32,
    mid_a2: f32,
    // High shelf
    high_b0: f32,
    high_b1: f32,
    high_b2: f32,
    high_a1: f32,
    high_a2: f32,
}

impl Default for EqEffect {
    fn default() -> Self {
        Self {
            low_gain: 0.0,
            mid_gain: 0.0,
            high_gain: 0.0,
            low_x1: 0.0,
            low_x2: 0.0,
            low_y1: 0.0,
            low_y2: 0.0,
            mid_x1: 0.0,
            mid_x2: 0.0,
            mid_y1: 0.0,
            mid_y2: 0.0,
            high_x1: 0.0,
            high_x2: 0.0,
            high_y1: 0.0,
            high_y2: 0.0,
            cached_coeffs: None,
        }
    }
}

/// Computes low-shelf biquad coefficients.
fn low_shelf_coeffs(cutoff: f32, gain_db: f32, sample_rate: f32) -> (f32, f32, f32, f32, f32) {
    let w0 = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
    let a = 10.0_f32.powf(gain_db / 40.0);
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let s = 1.0; // shelf slope parameter
    let s_alpha = sin_w0 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt().max(0.0) * 0.5;
    let two_sqrt_a_alpha = 2.0 * a.sqrt() * s_alpha;

    let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha);
    let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0);
    let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha);
    let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
    let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0);
    let a2 = (a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha;
    (b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
}

/// Computes peaking biquad coefficients.
fn peaking_coeffs(
    center: f32,
    gain_db: f32,
    q: f32,
    sample_rate: f32,
) -> (f32, f32, f32, f32, f32) {
    let w0 = 2.0 * std::f32::consts::PI * center / sample_rate;
    let a = 10.0_f32.powf(gain_db / 40.0);
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * q);
    let b0 = 1.0 + alpha * a;
    let b1 = -2.0 * cos_w0;
    let b2 = 1.0 - alpha * a;
    let a0 = 1.0 + alpha / a;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha / a;
    (b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
}

/// Computes high-shelf biquad coefficients.
fn high_shelf_coeffs(cutoff: f32, gain_db: f32, sample_rate: f32) -> (f32, f32, f32, f32, f32) {
    let w0 = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
    let a = 10.0_f32.powf(gain_db / 40.0);
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let s = 1.0; // shelf slope parameter
    let s_alpha = sin_w0 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt().max(0.0) * 0.5;
    let two_sqrt_a_alpha = 2.0 * a.sqrt() * s_alpha;

    let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha);
    let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
    let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha);
    let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
    let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
    let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha;
    (b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
}

impl AudioEffect for EqEffect {
    fn name(&self) -> &'static str {
        "EQ"
    }

    fn process(&mut self, samples: &mut [f32], _dt: f32) {
        let sample_rate = 44100.0;

        // Recompute coefficients if gains changed
        let low_g = self.low_gain.clamp(-1.0, 1.0) * 12.0; // ±12 dB range
        let mid_g = self.mid_gain.clamp(-1.0, 1.0) * 12.0;
        let high_g = self.high_gain.clamp(-1.0, 1.0) * 12.0;

        let need_recompute = match &self.cached_coeffs {
            None => true,
            Some(_) => true, // Always recompute for simplicity; could compare gains
        };

        if need_recompute {
            let (low_b0, low_b1, low_b2, low_a1, low_a2) =
                low_shelf_coeffs(300.0, low_g, sample_rate);
            let (mid_b0, mid_b1, mid_b2, mid_a1, mid_a2) =
                peaking_coeffs(1000.0, mid_g, 0.7, sample_rate);
            let (high_b0, high_b1, high_b2, high_a1, high_a2) =
                high_shelf_coeffs(5000.0, high_g, sample_rate);
            self.cached_coeffs = Some(EqCoefficients {
                low_b0,
                low_b1,
                low_b2,
                low_a1,
                low_a2,
                mid_b0,
                mid_b1,
                mid_b2,
                mid_a1,
                mid_a2,
                high_b0,
                high_b1,
                high_b2,
                high_a1,
                high_a2,
            });
        }

        let coeffs = self.cached_coeffs.as_ref().unwrap();

        // Apply three cascaded biquads: low shelf → peaking → high shelf
        for sample in samples.iter_mut() {
            let x = *sample;

            // Low shelf
            let y_low =
                coeffs.low_b0 * x + coeffs.low_b1 * self.low_x1 + coeffs.low_b2 * self.low_x2
                    - coeffs.low_a1 * self.low_y1
                    - coeffs.low_a2 * self.low_y2;
            self.low_x2 = self.low_x1;
            self.low_x1 = x;
            self.low_y2 = self.low_y1;
            self.low_y1 = y_low;

            // Peaking
            let y_mid =
                coeffs.mid_b0 * y_low + coeffs.mid_b1 * self.mid_x1 + coeffs.mid_b2 * self.mid_x2
                    - coeffs.mid_a1 * self.mid_y1
                    - coeffs.mid_a2 * self.mid_y2;
            self.mid_x2 = self.mid_x1;
            self.mid_x1 = y_low;
            self.mid_y2 = self.mid_y1;
            self.mid_y1 = y_mid;

            // High shelf
            let y_high = coeffs.high_b0 * y_mid
                + coeffs.high_b1 * self.high_x1
                + coeffs.high_b2 * self.high_x2
                - coeffs.high_a1 * self.high_y1
                - coeffs.high_a2 * self.high_y2;
            self.high_x2 = self.high_x1;
            self.high_x1 = y_mid;
            self.high_y2 = self.high_y1;
            self.high_y1 = y_high;

            *sample = y_high;
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
            let coeff = if abs > self.envelope {
                attack_coeff
            } else {
                release_coeff
            };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_finite(samples: &[f32]) {
        for &s in samples {
            assert!(s.is_finite(), "non-finite sample value: {s}");
        }
    }

    // ── Biquad coefficient tests ──

    #[test]
    fn low_shelf_coeffs_are_finite() {
        let (b0, b1, b2, a1, a2) = low_shelf_coeffs(300.0, 0.0, 44100.0);
        assert!(b0.is_finite() && b1.is_finite() && b2.is_finite());
        assert!(a1.is_finite() && a2.is_finite());
    }

    #[test]
    fn low_shelf_coeffs_with_gain() {
        // ±12 dB should produce different b0 coefficients
        let (b0_boost, ..) = low_shelf_coeffs(300.0, 12.0, 44100.0);
        let (b0_cut, ..) = low_shelf_coeffs(300.0, -12.0, 44100.0);
        assert!(b0_boost > 1.0, "boost should amplify");
        assert!(b0_cut < 1.0, "cut should attenuate");
    }

    #[test]
    fn peaking_coeffs_are_finite() {
        let (b0, b1, b2, a1, a2) = peaking_coeffs(1000.0, 6.0, 0.7, 44100.0);
        assert!(b0.is_finite() && b1.is_finite() && b2.is_finite());
        assert!(a1.is_finite() && a2.is_finite());
    }

    #[test]
    fn high_shelf_coeffs_are_finite() {
        let (b0, b1, b2, a1, a2) = high_shelf_coeffs(5000.0, 0.0, 44100.0);
        assert!(b0.is_finite() && b1.is_finite() && b2.is_finite());
        assert!(a1.is_finite() && a2.is_finite());
    }

    #[test]
    fn high_shelf_coeffs_with_gain() {
        let (b0_boost, ..) = high_shelf_coeffs(5000.0, 12.0, 44100.0);
        let (b0_cut, ..) = high_shelf_coeffs(5000.0, -12.0, 44100.0);
        assert!(b0_boost > 1.0, "boost should amplify");
        assert!(b0_cut < 1.0, "cut should attenuate");
    }

    #[test]
    fn coeffs_handle_edge_frequencies() {
        // Very low frequency
        let (b0, b1, b2, a1, a2) = low_shelf_coeffs(20.0, 0.0, 44100.0);
        assert!(b0.is_finite() && b1.is_finite() && b2.is_finite());
        assert!(a1.is_finite() && a2.is_finite());

        // Near Nyquist
        let (b0, b1, b2, a1, a2) = low_shelf_coeffs(20000.0, 0.0, 44100.0);
        assert!(b0.is_finite() && b1.is_finite() && b2.is_finite());
        assert!(a1.is_finite() && a2.is_finite());
    }

    // ── Effect smoke tests ──

    #[test]
    fn reverb_process_produces_finite_output() {
        let mut reverb = ReverbEffect::new();
        let mut samples = vec![0.5; 128];
        reverb.process(&mut samples, 1.0 / 60.0);
        assert_finite(&samples);
    }

    #[test]
    fn eq_effect_processes_without_panic() {
        let mut eq = EqEffect::default();
        let mut samples = vec![0.25; 128];
        eq.process(&mut samples, 1.0 / 60.0);
        assert_finite(&samples);
    }

    #[test]
    fn eq_effect_responds_to_gain_changes() {
        let mut eq = EqEffect::default();
        let mut flat = vec![0.5; 256];
        eq.process(&mut flat, 1.0 / 60.0);

        let mut boosted = vec![0.5; 256];
        eq.low_gain = 1.0; // +12 dB low shelf
        eq.process(&mut boosted, 1.0 / 60.0);

        // Boosted low shelf should change at least one sample
        let diff = flat
            .iter()
            .zip(boosted.iter())
            .any(|(a, b)| (a - b).abs() > 1e-6);
        assert!(diff, "EQ with low_gain=1.0 should alter the signal");
    }

    #[test]
    fn compressor_process_produces_finite_output() {
        let mut comp = CompressorEffect::default();
        let mut samples = vec![0.8; 128];
        comp.process(&mut samples, 1.0 / 60.0);
        assert_finite(&samples);
    }

    #[test]
    fn compressor_reduces_loud_signals() {
        let mut comp = CompressorEffect::default();
        comp.threshold = 0.3;
        comp.ratio = 4.0;
        let mut samples = vec![0.9; 512];
        comp.process(&mut samples, 1.0 / 60.0);
        // Envelope needs time to build up; test the tail
        let tail_rms = (samples[256..].iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();
        assert!(
            tail_rms < 0.9,
            "compressor should reduce amplitude of loud signal"
        );
    }

    #[test]
    fn limiter_clamps_to_ceiling() {
        let mut limiter = LimiterEffect::default();
        limiter.ceiling = 0.5;
        let mut samples = vec![1.0, -1.0, 0.3, -0.3];
        limiter.process(&mut samples, 1.0 / 60.0);
        for &s in &samples {
            assert!(s.abs() <= 0.51, "sample {s} exceeds ceiling");
        }
    }

    #[test]
    fn delay_process_produces_finite_output() {
        let mut delay = DelayEffect::new(0.1);
        let mut samples = vec![0.3; 128];
        delay.process(&mut samples, 1.0 / 60.0);
        assert_finite(&samples);
    }

    #[test]
    fn chorus_process_produces_finite_output() {
        let mut chorus = ChorusEffect::default();
        let mut samples = vec![0.3; 128];
        chorus.process(&mut samples, 1.0 / 60.0);
        assert_finite(&samples);
    }

    #[test]
    fn filter_low_pass_attenuates_high_frequencies() {
        let mut filter = FilterEffect::default();
        filter.filter_type = FilterType::LowPass;
        filter.cutoff = 500.0;
        filter.resonance = 0.7;

        // Generate a tone near cutoff (should pass)
        let low_tone: Vec<f32> = (0..256)
            .map(|i| (2.0 * std::f32::consts::PI * 400.0 * i as f32 / 44100.0).sin())
            .collect();
        let low_rms = {
            let mut sig = low_tone.clone();
            filter.process(&mut sig, 1.0 / 60.0);
            (sig.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt()
        };

        // Generate a tone far above cutoff (should be attenuated)
        let mut reset = FilterEffect::default();
        reset.filter_type = FilterType::LowPass;
        reset.cutoff = 500.0;
        reset.resonance = 0.7;
        let high_tone: Vec<f32> = (0..256)
            .map(|i| (2.0 * std::f32::consts::PI * 4000.0 * i as f32 / 44100.0).sin())
            .collect();
        let high_rms = {
            let mut sig = high_tone;
            reset.process(&mut sig, 1.0 / 60.0);
            (sig.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt()
        };

        // Low tone should have more energy than high tone through LPF
        assert!(
            low_rms > high_rms,
            "LPF should pass low frequencies more than high: low={low_rms}, high={high_rms}"
        );
    }

    #[test]
    fn filter_high_pass_attenuates_low_frequencies() {
        let mut filter = FilterEffect::default();
        filter.filter_type = FilterType::HighPass;
        filter.cutoff = 2000.0;
        filter.resonance = 0.7;

        let low_tone: Vec<f32> = (0..256)
            .map(|i| (2.0 * std::f32::consts::PI * 200.0 * i as f32 / 44100.0).sin())
            .collect();
        let low_rms = {
            let mut sig = low_tone.clone();
            filter.process(&mut sig, 1.0 / 60.0);
            (sig.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt()
        };

        let mut reset = FilterEffect::default();
        reset.filter_type = FilterType::HighPass;
        reset.cutoff = 2000.0;
        reset.resonance = 0.7;
        let high_tone: Vec<f32> = (0..256)
            .map(|i| (2.0 * std::f32::consts::PI * 4000.0 * i as f32 / 44100.0).sin())
            .collect();
        let high_rms = {
            let mut sig = high_tone;
            reset.process(&mut sig, 1.0 / 60.0);
            (sig.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt()
        };

        assert!(
            low_rms < high_rms,
            "HPF should pass high frequencies more than low: low={low_rms}, high={high_rms}"
        );
    }

    #[test]
    fn filter_band_pass_rejects_extremes() {
        let mut filter = FilterEffect::default();
        filter.filter_type = FilterType::BandPass;
        filter.cutoff = 1000.0;
        filter.resonance = 0.7;

        let mid_tone: Vec<f32> = (0..256)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 44100.0).sin())
            .collect();
        let mid_rms = {
            let mut sig = mid_tone.clone();
            filter.process(&mut sig, 1.0 / 60.0);
            (sig.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt()
        };

        let mut reset = FilterEffect::default();
        reset.filter_type = FilterType::BandPass;
        reset.cutoff = 1000.0;
        reset.resonance = 0.7;
        let low_tone: Vec<f32> = (0..256)
            .map(|i| (2.0 * std::f32::consts::PI * 50.0 * i as f32 / 44100.0).sin())
            .collect();
        let low_rms = {
            let mut sig = low_tone;
            reset.process(&mut sig, 1.0 / 60.0);
            (sig.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt()
        };

        assert!(
            mid_rms > low_rms,
            "BPF should pass center frequencies more than low: mid={mid_rms}, low={low_rms}"
        );
    }

    #[test]
    fn set_parameter_on_all_effects() {
        let effects: &mut [&mut dyn AudioEffect] = &mut [
            &mut ReverbEffect::new(),
            &mut EqEffect::default(),
            &mut CompressorEffect::default(),
            &mut LimiterEffect::default(),
            &mut DelayEffect::new(0.1),
            &mut ChorusEffect::default(),
            &mut FilterEffect::default(),
        ];
        for effect in effects.iter_mut() {
            effect.set_parameter("nonexistent", 0.5); // must not panic
        }
    }

    #[test]
    fn zero_input_produces_zero_output_for_linear_effects() {
        // Effects with no internal excitation (delay, reverb with 0 feedback)
        // should output silence for silence input once the buffer drains.
        let mut delay = DelayEffect::new(0.1);
        delay.feedback = 0.0;
        delay.mix = 0.5;
        let mut zeros = vec![0.0; 4096]; // long enough to flush delay buffer
        delay.process(&mut zeros, 1.0 / 60.0);
        // Tail should be silent after the delay line is flushed
        let tail_silent = zeros[2048..].iter().all(|&s| s.abs() < 1e-6);
        assert!(
            tail_silent,
            "delay with zero feedback should eventually be silent"
        );
    }
}
