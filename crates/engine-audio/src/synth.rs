//! Web Audio API-style synthesis engine.
//!
//! Provides oscillators, gain nodes, filters, and a connectable node graph
//! for procedural audio generation from scripts.
//!
//! # Usage
//!
//! ```rust,ignore
//! use engine_audio::synth::*;
//!
//! let mut graph = SynthGraph::new(44100);
//! let osc = graph.add_node(SynthNode::oscillator(Waveform::Sine, 440.0));
//! let gain = graph.add_node(SynthNode::gain(0.5));
//! graph.connect(osc, gain);
//! graph.set_destination(gain);
//! graph.start_node(osc);
//!
//! let mut output = vec![0.0f32; 1024];
//! graph.render(&mut output);
//! ```

use std::collections::HashMap;

// ── Waveform types ──────────────────────────────────────────────────────────

/// Oscillator waveform shape.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Waveform {
    /// Sine wave.
    #[default]
    Sine,
    /// Square wave (duty cycle 0.5).
    Square,
    /// Sawtooth wave (rising).
    Sawtooth,
    /// Triangle wave.
    Triangle,
    /// White noise.
    Noise,
}

// ── Envelope (ADSR) ─────────────────────────────────────────────────────────

/// ADSR amplitude envelope.
#[derive(Clone, Debug, PartialEq)]
pub struct Envelope {
    /// Attack time in seconds.
    pub attack: f32,
    /// Decay time in seconds.
    pub decay: f32,
    /// Sustain level (0.0 to 1.0).
    pub sustain: f32,
    /// Release time in seconds.
    pub release: f32,
}

impl Default for Envelope {
    fn default() -> Self {
        Self {
            attack: 0.01,
            decay: 0.1,
            sustain: 0.7,
            release: 0.3,
        }
    }
}

/// Internal envelope state.
#[derive(Clone, Debug, Default)]
struct EnvelopeState {
    /// Current phase: 0=idle, 1=attack, 2=decay, 3=sustain, 4=release.
    phase: u8,
    /// Current envelope value (0.0 to 1.0).
    value: f32,
    /// Time within current phase.
    phase_time: f32,
    /// Whether a note-on has been received.
    gate: bool,
}

impl EnvelopeState {
    fn note_on(&mut self) {
        self.gate = true;
        self.phase = 1;
        self.phase_time = 0.0;
    }

    fn note_off(&mut self) {
        self.gate = false;
        if self.phase > 0 && self.phase < 4 {
            self.phase = 4;
            self.phase_time = 0.0;
        }
    }

    fn tick(&mut self, dt: f32, env: &Envelope) -> f32 {
        match self.phase {
            0 => {
                // Idle
                self.value = 0.0;
            }
            1 => {
                // Attack
                self.phase_time += dt;
                if env.attack > 0.0 {
                    self.value = (self.phase_time / env.attack).min(1.0);
                } else {
                    self.value = 1.0;
                }
                if self.phase_time >= env.attack {
                    self.phase = 2;
                    self.phase_time = 0.0;
                }
            }
            2 => {
                // Decay
                self.phase_time += dt;
                if env.decay > 0.0 {
                    let t = (self.phase_time / env.decay).min(1.0);
                    self.value = 1.0 + (env.sustain - 1.0) * t;
                } else {
                    self.value = env.sustain;
                }
                if self.phase_time >= env.decay {
                    self.phase = 3;
                    self.phase_time = 0.0;
                }
            }
            3 => {
                // Sustain
                self.value = env.sustain;
                if !self.gate {
                    self.phase = 4;
                    self.phase_time = 0.0;
                }
            }
            4 => {
                // Release
                self.phase_time += dt;
                if env.release > 0.0 {
                    let t = (self.phase_time / env.release).min(1.0);
                    self.value = self.value * (1.0 - t);
                } else {
                    self.value = 0.0;
                }
                if self.phase_time >= env.release {
                    self.phase = 0;
                    self.value = 0.0;
                }
            }
            _ => {}
        }
        self.value
    }

    fn is_active(&self) -> bool {
        self.phase > 0
    }
}

// ── Parameter automation ────────────────────────────────────────────────────

/// A scheduled parameter change.
#[derive(Clone, Debug)]
enum Automation {
    /// Hold current value until time.
    HoldValue {
        /// Target value.
        value: f32,
        /// Time to hold until.
        time: f32,
    },
    /// Linear ramp from current to target.
    LinearRamp {
        /// Target value.
        target: f32,
        /// Start time.
        start_time: f32,
        /// End time.
        end_time: f32,
        /// Value at start_time.
        start_value: f32,
    },
    /// Exponential ramp from current to target.
    ExponentialRamp {
        /// Target value.
        target: f32,
        /// Start time.
        start_time: f32,
        /// End time.
        end_time: f32,
        /// Value at start_time.
        start_value: f32,
    },
}

/// Manages parameter automation for a value.
#[derive(Clone, Debug, Default)]
struct AutomationParam {
    /// Current base value.
    value: f32,
    /// Scheduled automations (sorted by end_time).
    automations: Vec<Automation>,
}

impl AutomationParam {
    fn tick(&mut self, current_time: f32) -> f32 {
        // Remove expired automations
        self.automations.retain(|a| match a {
            Automation::HoldValue { time, .. } => current_time <= *time,
            Automation::LinearRamp { end_time, .. } => current_time <= *end_time,
            Automation::ExponentialRamp { end_time, .. } => current_time <= *end_time,
        });

        let mut result = self.value;
        if let Some(auto) = self.automations.first() {
            match auto {
                Automation::HoldValue { value, .. } => {
                    result = *value;
                }
                Automation::LinearRamp {
                    target,
                    start_time,
                    end_time,
                    start_value,
                } => {
                    let duration = end_time - start_time;
                    if duration > 0.0 {
                        let t = ((current_time - start_time) / duration).clamp(0.0, 1.0);
                        result = start_value + (target - start_value) * t;
                    }
                    self.value = result;
                }
                Automation::ExponentialRamp {
                    target,
                    start_time,
                    end_time,
                    start_value,
                } => {
                    let duration = end_time - start_time;
                    if duration > 0.0 && *start_value > 0.0 && *target > 0.0 {
                        let t = ((current_time - start_time) / duration).clamp(0.0, 1.0);
                        result = start_value * (target / start_value).powf(t);
                    }
                    self.value = result;
                }
            }
        }
        result
    }
}

// ── Synth node ──────────────────────────────────────────────────────────────

/// Internal state of a synth node.
#[derive(Clone, Debug)]
enum NodeKind {
    /// Oscillator node.
    Oscillator {
        /// Waveform shape.
        waveform: Waveform,
        /// Frequency in Hz.
        frequency: AutomationParam,
        /// Phase accumulator.
        phase: f32,
    },
    /// Gain node.
    Gain {
        /// Gain value.
        gain: AutomationParam,
        /// Optional ADSR envelope.
        envelope: Option<Envelope>,
        /// Envelope state.
        env_state: EnvelopeState,
    },
    /// Filter node (biquad).
    Filter {
        /// Filter type.
        filter_type: FilterKind,
        /// Cutoff frequency.
        cutoff: AutomationParam,
        /// Resonance (Q factor).
        resonance: f32,
        /// Biquad state.
        x1: f32,
        x2: f32,
        y1: f32,
        y2: f32,
    },
    /// Delay node.
    Delay {
        /// Delay time in seconds.
        time: AutomationParam,
        /// Feedback amount.
        feedback: f32,
        /// Wet/dry mix (0.0=dry, 1.0=wet).
        mix: f32,
        /// Delay buffer.
        buffer: Vec<f32>,
        /// Write position.
        position: usize,
    },
    /// Mixer (sums all inputs).
    Mixer,
}

/// Filter kind for the filter node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FilterKind {
    /// Low-pass filter.
    LowPass,
    /// High-pass filter.
    HighPass,
    /// Band-pass filter.
    BandPass,
}

/// A node in the synthesis graph.
#[derive(Clone, Debug)]
pub struct SynthNode {
    /// Whether this node is currently playing.
    playing: bool,
    /// The kind of node.
    kind: NodeKind,
}

impl SynthNode {
    /// Creates an oscillator node.
    pub fn oscillator(waveform: Waveform, frequency: f32) -> Self {
        Self {
            playing: false,
            kind: NodeKind::Oscillator {
                waveform,
                frequency: AutomationParam {
                    value: frequency,
                    automations: Vec::new(),
                },
                phase: 0.0,
            },
        }
    }

    /// Creates a gain node with an optional ADSR envelope.
    pub fn gain(value: f32) -> Self {
        Self {
            playing: false,
            kind: NodeKind::Gain {
                gain: AutomationParam {
                    value,
                    automations: Vec::new(),
                },
                envelope: None,
                env_state: EnvelopeState::default(),
            },
        }
    }

    /// Creates a gain node with an ADSR envelope.
    pub fn gain_with_envelope(value: f32, envelope: Envelope) -> Self {
        Self {
            playing: false,
            kind: NodeKind::Gain {
                gain: AutomationParam {
                    value,
                    automations: Vec::new(),
                },
                envelope: Some(envelope),
                env_state: EnvelopeState::default(),
            },
        }
    }

    /// Creates a filter node.
    pub fn filter(filter_type: FilterKind, cutoff: f32, resonance: f32) -> Self {
        Self {
            playing: false,
            kind: NodeKind::Filter {
                filter_type,
                cutoff: AutomationParam {
                    value: cutoff,
                    automations: Vec::new(),
                },
                resonance,
                x1: 0.0,
                x2: 0.0,
                y1: 0.0,
                y2: 0.0,
            },
        }
    }

    /// Creates a delay node.
    pub fn delay(time_secs: f32, feedback: f32, mix: f32) -> Self {
        let sample_rate = 44100.0;
        let buffer_len = (time_secs * sample_rate).max(1.0) as usize;
        Self {
            playing: false,
            kind: NodeKind::Delay {
                time: AutomationParam {
                    value: time_secs,
                    automations: Vec::new(),
                },
                feedback: feedback.clamp(0.0, 0.99),
                mix: mix.clamp(0.0, 1.0),
                buffer: vec![0.0; buffer_len],
                position: 0,
            },
        }
    }

    /// Creates a mixer node (sums inputs).
    pub fn mixer() -> Self {
        Self {
            playing: false,
            kind: NodeKind::Mixer,
        }
    }
}

// ── Node handle ─────────────────────────────────────────────────────────────

/// Opaque handle to a node in the synth graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NodeHandle(pub u32);

// ── Synth graph ─────────────────────────────────────────────────────────────

/// A Web Audio-style synthesis graph.
///
/// Nodes are connected in a directed graph. Audio flows from sources
/// (oscillators) through processors (gain, filter, delay) to the
/// destination node whose output is rendered into the output buffer.
#[derive(Debug)]
pub struct SynthGraph {
    /// All nodes by handle.
    nodes: HashMap<u32, SynthNode>,
    /// Adjacency list: node -> list of nodes it feeds into.
    connections: HashMap<u32, Vec<u32>>,
    /// The output node.
    destination: Option<u32>,
    /// Next handle counter.
    next_handle: u32,
    /// Sample rate.
    sample_rate: f32,
    /// Global time in seconds.
    time: f32,
    /// Simple LCG state for noise generation.
    noise_state: u32,
}

impl SynthGraph {
    /// Creates a new synth graph with the given sample rate.
    pub fn new(sample_rate: u32) -> Self {
        Self {
            nodes: HashMap::new(),
            connections: HashMap::new(),
            destination: None,
            next_handle: 1,
            sample_rate: sample_rate as f32,
            time: 0.0,
            noise_state: 12345,
        }
    }

    /// Adds a node to the graph and returns its handle.
    pub fn add_node(&mut self, node: SynthNode) -> NodeHandle {
        let handle = NodeHandle(self.next_handle);
        self.next_handle += 1;
        self.nodes.insert(handle.0, node);
        handle
    }

    /// Removes a node from the graph.
    pub fn remove_node(&mut self, handle: NodeHandle) {
        self.nodes.remove(&handle.0);
        self.connections.remove(&handle.0);
        for conns in self.connections.values_mut() {
            conns.retain(|&h| h != handle.0);
        }
        if self.destination == Some(handle.0) {
            self.destination = None;
        }
    }

    /// Connects `from` node's output to `to` node's input.
    pub fn connect(&mut self, from: NodeHandle, to: NodeHandle) {
        self.connections.entry(from.0).or_default().push(to.0);
    }

    /// Disconnects `from` from `to`.
    pub fn disconnect(&mut self, from: NodeHandle, to: NodeHandle) {
        if let Some(conns) = self.connections.get_mut(&from.0) {
            conns.retain(|&h| h != to.0);
        }
    }

    /// Sets the destination (output) node.
    pub fn set_destination(&mut self, handle: NodeHandle) {
        self.destination = Some(handle.0);
    }

    /// Starts a node (oscillator begins generating, envelope triggers note-on).
    pub fn start_node(&mut self, handle: NodeHandle) {
        if let Some(node) = self.nodes.get_mut(&handle.0) {
            node.playing = true;
            if let NodeKind::Gain {
                env_state,
                envelope,
                ..
            } = &mut node.kind
            {
                if envelope.is_some() {
                    env_state.note_on();
                }
            }
        }
    }

    /// Stops a node (envelope triggers note-off; oscillator stops after release).
    pub fn stop_node(&mut self, handle: NodeHandle) {
        if let Some(node) = self.nodes.get_mut(&handle.0) {
            if let NodeKind::Gain {
                env_state,
                envelope,
                ..
            } = &mut node.kind
            {
                if envelope.is_some() {
                    env_state.note_off();
                    // Don't set playing=false yet; let release finish
                } else {
                    node.playing = false;
                }
            } else {
                node.playing = false;
            }
        }
    }

    /// Sets a node parameter value.
    pub fn set_param(&mut self, handle: NodeHandle, param: &str, value: f32) {
        if let Some(node) = self.nodes.get_mut(&handle.0) {
            match &mut node.kind {
                NodeKind::Oscillator { frequency, .. } => {
                    if param == "frequency" {
                        frequency.value = value;
                    }
                }
                NodeKind::Gain { gain, .. } => {
                    if param == "gain" {
                        gain.value = value;
                    }
                }
                NodeKind::Filter {
                    cutoff, resonance, ..
                } => match param {
                    "cutoff" => cutoff.value = value,
                    "resonance" => *resonance = value.max(0.1),
                    _ => {}
                },
                NodeKind::Delay {
                    time,
                    feedback,
                    mix,
                    ..
                } => match param {
                    "time" => time.value = value.max(0.001),
                    "feedback" => *feedback = value.clamp(0.0, 0.99),
                    "mix" => *mix = value.clamp(0.0, 1.0),
                    _ => {}
                },
                NodeKind::Mixer => {}
            }
        }
    }

    /// Gets a node parameter value.
    pub fn get_param(&self, handle: NodeHandle, param: &str) -> f32 {
        if let Some(node) = self.nodes.get(&handle.0) {
            match &node.kind {
                NodeKind::Oscillator { frequency, .. } => {
                    if param == "frequency" {
                        return frequency.value;
                    }
                }
                NodeKind::Gain { gain, .. } => {
                    if param == "gain" {
                        return gain.value;
                    }
                }
                NodeKind::Filter {
                    cutoff, resonance, ..
                } => match param {
                    "cutoff" => return cutoff.value,
                    "resonance" => return *resonance,
                    _ => {}
                },
                NodeKind::Delay {
                    time,
                    feedback,
                    mix,
                    ..
                } => match param {
                    "time" => return time.value,
                    "feedback" => return *feedback,
                    "mix" => return *mix,
                    _ => {}
                },
                NodeKind::Mixer => {}
            }
        }
        0.0
    }

    /// Schedules a linear ramp on a node parameter.
    pub fn linear_ramp(&mut self, handle: NodeHandle, param: &str, target: f32, duration: f32) {
        if let Some(node) = self.nodes.get_mut(&handle.0) {
            let p = match (&mut node.kind, param) {
                (NodeKind::Oscillator { frequency, .. }, "frequency") => Some(frequency),
                (NodeKind::Gain { gain, .. }, "gain") => Some(gain),
                (NodeKind::Filter { cutoff, .. }, "cutoff") => Some(cutoff),
                (NodeKind::Delay { time, .. }, "time") => Some(time),
                _ => None,
            };
            if let Some(p) = p {
                let start_value = p.value;
                p.automations.push(Automation::LinearRamp {
                    target,
                    start_time: self.time,
                    end_time: self.time + duration,
                    start_value,
                });
            }
        }
    }

    /// Schedules an exponential ramp on a node parameter.
    pub fn exponential_ramp(
        &mut self,
        handle: NodeHandle,
        param: &str,
        target: f32,
        duration: f32,
    ) {
        if let Some(node) = self.nodes.get_mut(&handle.0) {
            let p = match (&mut node.kind, param) {
                (NodeKind::Oscillator { frequency, .. }, "frequency") => Some(frequency),
                (NodeKind::Gain { gain, .. }, "gain") => Some(gain),
                (NodeKind::Filter { cutoff, .. }, "cutoff") => Some(cutoff),
                (NodeKind::Delay { time, .. }, "time") => Some(time),
                _ => None,
            };
            if let Some(p) = p {
                let start_value = p.value;
                p.automations.push(Automation::ExponentialRamp {
                    target,
                    start_time: self.time,
                    end_time: self.time + duration,
                    start_value,
                });
            }
        }
    }

    /// Returns whether a node is currently playing/active.
    pub fn is_playing(&self, handle: NodeHandle) -> bool {
        self.nodes
            .get(&handle.0)
            .map(|n| n.playing)
            .unwrap_or(false)
    }

    /// Returns the current global time.
    pub fn time(&self) -> f32 {
        self.time
    }

    /// Renders audio into the output buffer.
    ///
    /// Traverses the graph from the destination node backwards, computing
    /// each node's output. The result is written to `output`.
    pub fn render(&mut self, output: &mut [f32]) {
        let num_samples = output.len();
        let dt = num_samples as f32 / self.sample_rate;

        // Build a topological order starting from destination
        let order = if let Some(dest) = self.destination {
            self.topo_order(dest)
        } else {
            vec![]
        };

        // Compute each node's output into a temporary buffer
        let mut node_outputs: HashMap<u32, Vec<f32>> = HashMap::new();

        for &node_id in &order {
            let input = self.sum_inputs(node_id, &node_outputs);
            let output_buf = self.render_node(node_id, &input, num_samples);
            node_outputs.insert(node_id, output_buf);
        }

        // Write destination output to the output buffer
        if let Some(dest) = self.destination {
            if let Some(dest_buf) = node_outputs.get(&dest) {
                output.copy_from_slice(dest_buf);
            } else {
                output.fill(0.0);
            }
        } else {
            output.fill(0.0);
        }

        self.time += dt;
    }

    /// Renders a single node given its input buffer.
    fn render_node(&mut self, node_id: u32, input: &[f32], num_samples: usize) -> Vec<f32> {
        let mut output = vec![0.0f32; num_samples];
        let dt = 1.0 / self.sample_rate;

        if let Some(node) = self.nodes.get_mut(&node_id) {
            // Source nodes (oscillators) only produce output when playing.
            if matches!(node.kind, NodeKind::Oscillator { .. }) && !node.playing {
                return output;
            }

            // For gain nodes with an envelope that has fully released, skip.
            if let NodeKind::Gain {
                env_state,
                envelope,
                ..
            } = &node.kind
            {
                if envelope.is_some() && !node.playing && !env_state.is_active() {
                    return output;
                }
            }

            match &mut node.kind {
                NodeKind::Oscillator {
                    waveform,
                    frequency,
                    phase,
                } => {
                    for i in 0..num_samples {
                        let freq = frequency.tick(self.time + i as f32 * dt);
                        let phase_inc = freq / self.sample_rate;
                        output[i] = match waveform {
                            Waveform::Sine => (*phase * 2.0 * std::f32::consts::PI).sin(),
                            Waveform::Square => {
                                if *phase < 0.5 {
                                    1.0
                                } else {
                                    -1.0
                                }
                            }
                            Waveform::Sawtooth => *phase * 2.0 - 1.0,
                            Waveform::Triangle => {
                                if *phase < 0.5 {
                                    *phase * 4.0 - 1.0
                                } else {
                                    3.0 - *phase * 4.0
                                }
                            }
                            Waveform::Noise => {
                                self.noise_state = self
                                    .noise_state
                                    .wrapping_mul(1103515245)
                                    .wrapping_add(12345);
                                (self.noise_state as f32 / u32::MAX as f32) * 2.0 - 1.0
                            }
                        };
                        *phase += phase_inc;
                        if *phase >= 1.0 {
                            *phase -= 1.0;
                        }
                    }
                }
                NodeKind::Gain {
                    gain,
                    envelope,
                    env_state,
                } => {
                    let has_envelope = envelope.is_some();
                    for i in 0..num_samples {
                        let g = gain.tick(self.time + i as f32 * dt);
                        let env_val = if let Some(env) = envelope {
                            env_state.tick(dt, env)
                        } else {
                            1.0
                        };
                        output[i] = input.get(i).copied().unwrap_or(0.0) * g * env_val;
                    }
                    // Check if envelope finished
                    if has_envelope && !env_state.is_active() {
                        node.playing = false;
                    }
                }
                NodeKind::Filter {
                    filter_type,
                    cutoff,
                    resonance,
                    x1,
                    x2,
                    y1,
                    y2,
                } => {
                    for i in 0..num_samples {
                        let freq = cutoff.tick(self.time + i as f32 * dt);
                        let w0 = 2.0 * std::f32::consts::PI * freq / self.sample_rate;
                        let alpha = w0.sin() / (2.0 * (*resonance).max(0.01));
                        let cos_w0 = w0.cos();

                        let (b0, b1, b2, a1, a2) = match filter_type {
                            FilterKind::LowPass => {
                                let b1 = 1.0 - cos_w0;
                                let b0 = b1 / 2.0;
                                let a0 = 1.0 + alpha;
                                (
                                    b0 / a0,
                                    b1 / a0,
                                    b0 / a0,
                                    -2.0 * cos_w0 / a0,
                                    (1.0 - alpha) / a0,
                                )
                            }
                            FilterKind::HighPass => {
                                let b1 = -(1.0 + cos_w0);
                                let b0 = (1.0 + cos_w0) / 2.0;
                                let a0 = 1.0 + alpha;
                                (
                                    b0 / a0,
                                    b1 / a0,
                                    b0 / a0,
                                    -2.0 * cos_w0 / a0,
                                    (1.0 - alpha) / a0,
                                )
                            }
                            FilterKind::BandPass => {
                                let a0 = 1.0 + alpha;
                                (
                                    alpha / a0,
                                    0.0,
                                    -alpha / a0,
                                    -2.0 * cos_w0 / a0,
                                    (1.0 - alpha) / a0,
                                )
                            }
                        };

                        let x0 = input.get(i).copied().unwrap_or(0.0);
                        let y0 = b0 * x0 + b1 * *x1 + b2 * *x2 - a1 * *y1 - a2 * *y2;
                        *x2 = *x1;
                        *x1 = x0;
                        *y2 = *y1;
                        *y1 = y0;
                        output[i] = y0;
                    }
                }
                NodeKind::Delay {
                    time,
                    feedback,
                    mix,
                    buffer,
                    position,
                } => {
                    let buf_len = buffer.len();
                    for i in 0..num_samples {
                        let delay_time = time.tick(self.time + i as f32 * dt);
                        let delay_samples = (delay_time * self.sample_rate).max(1.0) as usize;
                        let read_pos =
                            (*position + buf_len - delay_samples.min(buf_len - 1)) % buf_len;
                        let delayed = buffer[read_pos];
                        let input_sample = input.get(i).copied().unwrap_or(0.0);
                        buffer[*position] = input_sample + delayed * *feedback;
                        *position = (*position + 1) % buf_len;
                        output[i] = input_sample * (1.0 - *mix) + delayed * *mix;
                    }
                }
                NodeKind::Mixer => {
                    // Mixer just passes through the summed input
                    output.copy_from_slice(input);
                }
            }
        }

        output
    }

    /// Sums the outputs of all nodes connected to `node_id`.
    fn sum_inputs(&self, node_id: u32, node_outputs: &HashMap<u32, Vec<f32>>) -> Vec<f32> {
        let num_samples = node_outputs.values().next().map(|v| v.len()).unwrap_or(0);
        let mut sum = vec![0.0f32; num_samples];

        // Find all nodes that feed into this node
        for (&from_id, to_ids) in &self.connections {
            if to_ids.contains(&node_id) {
                if let Some(buf) = node_outputs.get(&from_id) {
                    for (s, input) in sum.iter_mut().zip(buf.iter()) {
                        *s += input;
                    }
                }
            }
        }
        sum
    }

    /// Topological sort from a target node (BFS backwards through connections).
    fn topo_order(&self, target: u32) -> Vec<u32> {
        let mut visited = std::collections::HashSet::new();
        let mut order = Vec::new();
        let mut stack = vec![target];

        while let Some(node_id) = stack.pop() {
            if visited.contains(&node_id) {
                continue;
            }
            visited.insert(node_id);
            order.push(node_id);

            // Find all nodes that feed into this node
            for (&from_id, to_ids) in &self.connections {
                if to_ids.contains(&node_id) && !visited.contains(&from_id) {
                    stack.push(from_id);
                }
            }
        }

        order.reverse();
        order
    }
}

// ── Convenience functions ───────────────────────────────────────────────────

/// Generates a one-shot tone into a buffer (Web Audio `playTone` style).
///
/// Returns the generated samples.
pub fn generate_tone(
    waveform: Waveform,
    frequency: f32,
    duration: f32,
    volume: f32,
    sample_rate: u32,
) -> Vec<f32> {
    let num_samples = (duration * sample_rate as f32) as usize;
    let mut samples = Vec::with_capacity(num_samples);
    let mut phase = 0.0f32;
    let phase_inc = frequency / sample_rate as f32;

    for i in 0..num_samples {
        // Simple fade out in last 10% to avoid clicks
        let t = i as f32 / num_samples as f32;
        let fade = if t > 0.9 { (1.0 - t) / 0.1 } else { 1.0 };

        let sample = match waveform {
            Waveform::Sine => (phase * 2.0 * std::f32::consts::PI).sin(),
            Waveform::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Waveform::Sawtooth => phase * 2.0 - 1.0,
            Waveform::Triangle => {
                if phase < 0.5 {
                    phase * 4.0 - 1.0
                } else {
                    3.0 - phase * 4.0
                }
            }
            Waveform::Noise => {
                // Simple LCG noise
                let state = (i as u32).wrapping_mul(1103515245).wrapping_add(12345);
                (state as f32 / u32::MAX as f32) * 2.0 - 1.0
            }
        };

        samples.push(sample * volume * fade);
        phase += phase_inc;
        if phase >= 1.0 {
            phase -= 1.0;
        }
    }

    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oscillator_generates_samples() {
        let mut graph = SynthGraph::new(44100);
        let osc = graph.add_node(SynthNode::oscillator(Waveform::Sine, 440.0));
        let gain = graph.add_node(SynthNode::gain(0.5));
        graph.connect(osc, gain);
        graph.set_destination(gain);
        graph.start_node(osc);

        // Debug: check gain node state
        let gain_node = graph.nodes.get(&gain.0).unwrap();
        assert!(
            !gain_node.playing,
            "gain should not be playing (it's a processor)"
        );
        assert!(
            matches!(gain_node.kind, NodeKind::Gain { .. }),
            "should be gain node"
        );

        // Debug: manually call render_node for gain with input
        let input = vec![0.5f32; 1024];
        let gain_output = graph.render_node(gain.0, &input, 1024);
        let gain_max = gain_output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            gain_max > 0.01,
            "gain render_node should work, got max={}",
            gain_max
        );

        // Now test full graph
        let mut output = vec![0.0f32; 1024];
        graph.render(&mut output);
        let max_abs = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max_abs > 0.01,
            "full graph should produce output, got max={}",
            max_abs
        );
    }

    #[test]
    fn oscillator_silence_when_not_started() {
        let mut graph = SynthGraph::new(44100);
        let osc = graph.add_node(SynthNode::oscillator(Waveform::Sine, 440.0));
        graph.set_destination(osc);

        let mut output = vec![0.5f32; 1024];
        graph.render(&mut output);

        let all_zero = output.iter().all(|&s| s.abs() < 0.001);
        assert!(all_zero, "unstarted oscillator should produce silence");
    }

    #[test]
    fn gain_node_multiplies() {
        let mut graph = SynthGraph::new(44100);
        let osc = graph.add_node(SynthNode::oscillator(Waveform::Sine, 440.0));
        let gain = graph.add_node(SynthNode::gain(0.1));
        graph.connect(osc, gain);
        graph.set_destination(gain);
        graph.start_node(osc);

        let mut output = vec![0.0f32; 1024];
        graph.render(&mut output);

        // All samples should be <= 0.1 (gain)
        let max = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max <= 0.11, "gain should limit output, got max={}", max);
    }

    #[test]
    fn envelope_triggers_and_releases() {
        let mut graph = SynthGraph::new(44100);
        let osc = graph.add_node(SynthNode::oscillator(Waveform::Sine, 440.0));
        let gain = graph.add_node(SynthNode::gain_with_envelope(
            1.0,
            Envelope {
                attack: 0.01,
                decay: 0.01,
                sustain: 0.5,
                release: 0.01,
            },
        ));
        graph.connect(osc, gain);
        graph.set_destination(gain);
        graph.start_node(osc);
        graph.start_node(gain); // Trigger envelope note-on

        // Render a bit — should have output (attack + decay + sustain)
        let mut output1 = vec![0.0f32; 2048];
        graph.render(&mut output1);
        let has_signal = output1.iter().any(|&s| s.abs() > 0.01);
        assert!(has_signal, "envelope should allow signal through");

        // Stop — release phase
        graph.stop_node(gain);
        let mut output2 = vec![0.0f32; 44100]; // 1 second
        graph.render(&mut output2);

        // After release, node should be inactive
        assert!(
            !graph.is_playing(gain),
            "gain should be inactive after release"
        );
    }

    #[test]
    fn filter_node_processes() {
        let mut graph = SynthGraph::new(44100);
        let osc = graph.add_node(SynthNode::oscillator(Waveform::Sawtooth, 1000.0));
        let filter = graph.add_node(SynthNode::filter(FilterKind::LowPass, 500.0, 0.7));
        graph.connect(osc, filter);
        graph.set_destination(filter);
        graph.start_node(osc);

        let mut output = vec![0.0f32; 1024];
        graph.render(&mut output);

        let has_signal = output.iter().any(|&s| s.abs() > 0.01);
        assert!(has_signal, "filter should produce output");
    }

    #[test]
    fn delay_node_processes() {
        let mut graph = SynthGraph::new(44100);
        let osc = graph.add_node(SynthNode::oscillator(Waveform::Sine, 440.0));
        let delay = graph.add_node(SynthNode::delay(0.1, 0.3, 0.5));
        graph.connect(osc, delay);
        graph.set_destination(delay);
        graph.start_node(osc);

        let mut output = vec![0.0f32; 8192];
        graph.render(&mut output);

        // Should have signal
        let has_signal = output.iter().any(|&s| s.abs() > 0.01);
        assert!(has_signal, "delay should produce output");
    }

    #[test]
    fn mixer_sums_inputs() {
        let mut graph = SynthGraph::new(44100);
        let osc1 = graph.add_node(SynthNode::oscillator(Waveform::Sine, 440.0));
        let osc2 = graph.add_node(SynthNode::oscillator(Waveform::Sine, 880.0));
        let mixer = graph.add_node(SynthNode::mixer());
        graph.connect(osc1, mixer);
        graph.connect(osc2, mixer);
        graph.set_destination(mixer);
        graph.start_node(osc1);
        graph.start_node(osc2);

        let mut output = vec![0.0f32; 1024];
        graph.render(&mut output);

        // Sum of two oscillators should have higher amplitude than one
        let max = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max > 0.1, "mixer should sum inputs, got max={}", max);
    }

    #[test]
    fn linear_ramp_changes_param() {
        // Test automation param directly
        let mut param = AutomationParam {
            value: 440.0,
            automations: vec![Automation::LinearRamp {
                target: 880.0,
                start_time: 0.0,
                end_time: 0.5,
                start_value: 440.0,
            }],
        };
        let v = param.tick(0.5);
        assert!(
            (v - 880.0).abs() < 1.0,
            "tick(0.5) should return ~880, got {}",
            v
        );
        assert!(
            (param.value - 880.0).abs() < 1.0,
            "value should be 880, got {}",
            param.value
        );

        // Test node mutation: tick through render_node's exact code path
        let mut graph = SynthGraph::new(44100);
        let osc = graph.add_node(SynthNode::oscillator(Waveform::Sine, 440.0));
        graph.linear_ramp(osc, "frequency", 880.0, 0.5);

        // Simulate what render_node does
        let num_samples = 22050usize;
        let dt = 1.0f32 / 44100.0;
        {
            let node = graph.nodes.get_mut(&osc.0).unwrap();
            if let NodeKind::Oscillator { frequency, .. } = &mut node.kind {
                for i in 0..num_samples {
                    let _freq = frequency.tick(i as f32 * dt);
                }
            }
        }
        let freq = graph.get_param(osc, "frequency");
        assert!(
            (freq - 880.0).abs() < 1.0,
            "after manual tick loop, freq should be 880, got {}",
            freq
        );
    }

    #[test]
    fn generate_tone_produces_samples() {
        let samples = generate_tone(Waveform::Square, 440.0, 0.1, 0.5, 44100);
        assert_eq!(samples.len(), 4410);
        let has_signal = samples.iter().any(|&s| s.abs() > 0.01);
        assert!(has_signal, "generate_tone should produce samples");
    }

    #[test]
    fn remove_node_cleans_up() {
        let mut graph = SynthGraph::new(44100);
        let osc = graph.add_node(SynthNode::oscillator(Waveform::Sine, 440.0));
        let gain = graph.add_node(SynthNode::gain(0.5));
        graph.connect(osc, gain);
        graph.set_destination(gain);

        graph.remove_node(osc);

        // Rendering should not panic
        let mut output = vec![0.0f32; 1024];
        graph.render(&mut output);

        // Output should be silence since osc was removed
        let all_zero = output.iter().all(|&s| s.abs() < 0.001);
        assert!(all_zero, "removed node should produce silence");
    }

    #[test]
    fn all_waveforms_generate() {
        for wf in [
            Waveform::Sine,
            Waveform::Square,
            Waveform::Sawtooth,
            Waveform::Triangle,
            Waveform::Noise,
        ] {
            let samples = generate_tone(wf, 440.0, 0.05, 0.5, 44100);
            assert!(samples.len() > 0, "{:?} should produce samples", wf);
            let has_signal = samples.iter().any(|&s| s.abs() > 0.001);
            assert!(has_signal, "{:?} should have non-zero output", wf);
        }
    }
}
