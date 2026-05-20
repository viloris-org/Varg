//! Frame and time primitives.

use std::time::Duration;

/// Monotonic frame counter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameCounter(u64);

impl FrameCounter {
    /// Current frame index.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Advances by one frame.
    pub fn advance(&mut self) {
        self.0 = self.0.saturating_add(1);
    }
}

/// Time elapsed for a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeStep {
    delta: Duration,
}

impl TimeStep {
    /// Creates a timestep from a duration.
    pub const fn new(delta: Duration) -> Self {
        Self { delta }
    }

    /// Delta duration.
    pub const fn delta(self) -> Duration {
        self.delta
    }

    /// Delta seconds as `f32`.
    pub fn seconds_f32(self) -> f32 {
        self.delta.as_secs_f32()
    }
}

/// Aggregated time state for the game loop, tracking delta time, fixed timestep,
/// total elapsed time, frame counting, and time scale.
#[derive(Clone, Debug)]
pub struct TimeState {
    /// Wall-clock delta seconds for the current frame (before time scale).
    pub delta_seconds: f32,
    /// Target duration for each fixed timestep tick (default 1.0 / 60.0).
    pub fixed_delta_seconds: f32,
    /// Total elapsed time since the game loop started (time-scaled).
    pub total_time: f32,
    /// Monotonic frame index.
    pub frame_index: u64,
    /// Multiplier applied to delta time each frame (default 1.0).
    pub time_scale: f32,
}

impl Default for TimeState {
    fn default() -> Self {
        Self {
            delta_seconds: 0.0,
            fixed_delta_seconds: 1.0 / 60.0,
            total_time: 0.0,
            frame_index: 0,
            time_scale: 1.0,
        }
    }
}

impl TimeState {
    /// Creates a new `TimeState` with sensible defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Advances the time state by `dt` seconds (wall-clock). The scaled delta is
    /// accumulated into `total_time` and `frame_index` is incremented.
    pub fn update(&mut self, dt: f32) {
        self.delta_seconds = dt;
        let scaled = dt * self.time_scale;
        self.total_time += scaled;
        self.frame_index = self.frame_index.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_state_defaults() {
        let ts = TimeState::new();
        assert_eq!(ts.delta_seconds, 0.0);
        assert!((ts.fixed_delta_seconds - 1.0 / 60.0).abs() < f32::EPSILON);
        assert_eq!(ts.total_time, 0.0);
        assert_eq!(ts.frame_index, 0);
        assert_eq!(ts.time_scale, 1.0);
    }

    #[test]
    fn time_state_update_accumulates_time() {
        let mut ts = TimeState::new();
        ts.update(0.016);
        assert_eq!(ts.delta_seconds, 0.016);
        assert!((ts.total_time - 0.016).abs() < f32::EPSILON);
        assert_eq!(ts.frame_index, 1);

        ts.update(0.032);
        assert_eq!(ts.delta_seconds, 0.032);
        assert!((ts.total_time - 0.048).abs() < f32::EPSILON);
        assert_eq!(ts.frame_index, 2);
    }

    #[test]
    fn time_state_respects_time_scale() {
        let mut ts = TimeState::new();
        ts.time_scale = 0.5;
        ts.update(1.0);
        assert!((ts.total_time - 0.5).abs() < f32::EPSILON);

        ts.time_scale = 2.0;
        ts.update(1.0);
        assert!((ts.total_time - 2.5).abs() < f32::EPSILON);
    }

    #[test]
    fn time_state_frame_index_saturates() {
        let mut ts = TimeState::new();
        ts.frame_index = u64::MAX;
        ts.update(0.0);
        assert_eq!(ts.frame_index, u64::MAX);
    }
}
