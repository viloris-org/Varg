//! Input buffer that stores a sliding window of frame snapshots.

use crate::input::{InputState, KeyCode};

/// Sliding-window input buffer for fighting-game-style input buffering.
///
/// Stores the last N frames of input state, allowing queries like
/// "was jump pressed in the last 6 frames?"
#[derive(Clone, Debug, Default)]
pub struct InputBuffer {
    frames: Vec<FrameSnapshot>,
    capacity: usize,
}

#[derive(Clone, Debug, Default)]
struct FrameSnapshot {
    pressed_keys: Vec<KeyCode>,
    pressed_actions: Vec<String>,
}

impl InputBuffer {
    /// Creates an input buffer with space for `capacity` frame snapshots.
    pub fn new(capacity: usize) -> Self {
        Self {
            frames: Vec::with_capacity(capacity),
            capacity: capacity.max(1),
        }
    }

    /// Records the current frame's transient pressed state.
    pub fn record_frame(&mut self, input: &InputState, actions: &[String]) {
        let keys: Vec<KeyCode> = input.pressed_keys();

        if self.frames.len() >= self.capacity {
            self.frames.remove(0);
        }
        self.frames.push(FrameSnapshot {
            pressed_keys: keys,
            pressed_actions: actions.to_vec(),
        });
    }

    /// Returns whether any of the given keys were pressed in the last `frames` frames.
    pub fn key_pressed_in_last(&self, key: KeyCode, frames: usize) -> bool {
        let start = self.frames.len().saturating_sub(frames);
        self.frames[start..]
            .iter()
            .any(|snapshot| snapshot.pressed_keys.contains(&key))
    }

    /// Returns whether any of the given actions were active in the last `frames` frames.
    pub fn action_in_last(&self, action: &str, frames: usize) -> bool {
        let start = self.frames.len().saturating_sub(frames);
        self.frames[start..]
            .iter()
            .any(|snapshot| snapshot.pressed_actions.iter().any(|a| a == action))
    }

    /// Clears all buffered frames.
    pub fn clear(&mut self) {
        self.frames.clear();
    }

    /// Returns the number of buffered frames.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Returns whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}
