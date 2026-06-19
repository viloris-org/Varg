//! Audio bus graph with routing, volume, mute, and solo controls.

use crate::effects::AudioEffect;

/// A node in the audio bus graph.
#[derive(Debug)]
pub struct AudioBus {
    /// Bus name.
    pub name: String,
    /// Volume multiplier (0.0 to 1.0).
    pub volume: f32,
    /// Whether this bus is muted.
    pub muted: bool,
    /// Whether this bus is soloed.
    pub solo: bool,
    /// Effects applied to this bus.
    pub effects: Vec<Box<dyn AudioEffect>>,
    /// Child buses.
    pub children: Vec<AudioBus>,
    /// Optional sidechain source bus name.
    pub sidechain_source: Option<String>,
    /// Sidechain mix amount.
    pub sidechain_amount: f32,
}

impl AudioBus {
    /// Creates a new audio bus.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            volume: 1.0,
            muted: false,
            solo: false,
            effects: Vec::new(),
            children: Vec::new(),
            sidechain_source: None,
            sidechain_amount: 0.0,
        }
    }
}

/// Manages the audio bus hierarchy and processes audio through it.
#[derive(Debug)]
pub struct AudioBusGraph {
    root: AudioBus,
}

impl Default for AudioBusGraph {
    fn default() -> Self {
        let mut master = AudioBus::new("Master");
        master.children.push(AudioBus::new("Music"));
        master.children.push(AudioBus::new("SFX"));
        master.children.push(AudioBus::new("Voice"));
        master.children.push(AudioBus::new("Ambient"));
        Self { root: master }
    }
}

impl AudioBusGraph {
    /// Creates a bus graph with the default Master/Music/SFX/Voice/Ambient layout.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the master bus.
    pub fn master(&self) -> &AudioBus {
        &self.root
    }

    /// Returns a mutable reference to the master bus.
    pub fn master_mut(&mut self) -> &mut AudioBus {
        &mut self.root
    }

    /// Finds a bus by name in the hierarchy.
    pub fn bus(&self, name: &str) -> Option<&AudioBus> {
        find_bus(&self.root, name)
    }

    /// Finds a mutable bus by name.
    pub fn bus_mut(&mut self, name: &str) -> Option<&mut AudioBus> {
        find_bus_mut(&mut self.root, name)
    }

    /// Adds a child bus to a parent bus.
    pub fn add_bus(&mut self, parent_name: &str, child: AudioBus) -> bool {
        if let Some(parent) = find_bus_mut(&mut self.root, parent_name) {
            parent.children.push(child);
            true
        } else {
            false
        }
    }

    /// Removes a bus by name.
    pub fn remove_bus(&mut self, name: &str) -> bool {
        remove_bus_from(&mut self.root, name)
    }

    /// Processes audio samples through the bus graph.
    ///
    /// Audio flows from parent to children. Each bus applies volume,
    /// then its effects, then passes to children. Returns the mixed output.
    pub fn process(&mut self, samples: &mut [f32], dt: f32) {
        let has_solo = any_solo(&self.root);
        process_bus(&mut self.root, samples, dt, 1.0, has_solo, false);
    }

    /// Sets the volume of a bus.
    pub fn set_volume(&mut self, name: &str, volume: f32) {
        if let Some(bus) = find_bus_mut(&mut self.root, name) {
            bus.volume = volume.clamp(0.0, 1.0);
        }
    }

    /// Mutes or unmutes a bus.
    pub fn set_muted(&mut self, name: &str, muted: bool) {
        if let Some(bus) = find_bus_mut(&mut self.root, name) {
            bus.muted = muted;
        }
    }

    /// Sets solo state on a bus.
    pub fn set_solo(&mut self, name: &str, solo: bool) {
        if let Some(bus) = find_bus_mut(&mut self.root, name) {
            bus.solo = solo;
        }
    }

    /// Returns the effective linear gain of every bus.
    pub fn effective_gains(&self) -> Vec<(String, f32)> {
        let mut gains = Vec::new();
        collect_effective_gains(&self.root, 1.0, &mut gains);
        gains
    }
}

fn collect_effective_gains(bus: &AudioBus, parent_gain: f32, output: &mut Vec<(String, f32)>) {
    let gain = if bus.muted {
        0.0
    } else {
        parent_gain * bus.volume
    };
    output.push((bus.name.clone(), gain));
    for child in &bus.children {
        collect_effective_gains(child, gain, output);
    }
}

fn find_bus<'a>(bus: &'a AudioBus, name: &str) -> Option<&'a AudioBus> {
    if bus.name == name {
        return Some(bus);
    }
    for child in &bus.children {
        if let Some(found) = find_bus(child, name) {
            return Some(found);
        }
    }
    None
}

fn find_bus_mut<'a>(bus: &'a mut AudioBus, name: &str) -> Option<&'a mut AudioBus> {
    if bus.name == name {
        return Some(bus);
    }
    for child in &mut bus.children {
        if let Some(found) = find_bus_mut(child, name) {
            return Some(found);
        }
    }
    None
}

fn remove_bus_from(bus: &mut AudioBus, name: &str) -> bool {
    let before = bus.children.len();
    bus.children.retain(|child| child.name != name);
    if bus.children.len() != before {
        return true;
    }
    for child in &mut bus.children {
        if remove_bus_from(child, name) {
            return true;
        }
    }
    false
}

fn any_solo(bus: &AudioBus) -> bool {
    if bus.solo {
        return true;
    }
    bus.children.iter().any(any_solo)
}

fn process_bus(
    bus: &mut AudioBus,
    samples: &mut [f32],
    dt: f32,
    parent_volume: f32,
    has_solo: bool,
    solo_found: bool,
) {
    let effective_volume = if bus.muted || (has_solo && !bus.solo && !solo_found) {
        0.0
    } else {
        bus.volume * parent_volume
    };

    for sample in samples.iter_mut() {
        *sample *= effective_volume;
    }

    for effect in &mut bus.effects {
        effect.process(samples, dt);
    }

    let child_solo_found = solo_found || bus.solo;
    for child in &mut bus.children {
        let mut child_samples = samples.to_vec();
        process_bus(
            child,
            &mut child_samples,
            dt,
            effective_volume,
            has_solo,
            child_solo_found,
        );
        for (s, cs) in samples.iter_mut().zip(child_samples) {
            *s += cs;
        }
    }
}
