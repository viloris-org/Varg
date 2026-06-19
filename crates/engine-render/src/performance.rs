//! Runtime rendering performance policy and telemetry.

/// Presentation strategy for a native runtime surface.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PresentStrategy {
    /// Prefer mailbox, then immediate, then FIFO.
    LowLatency,
    /// Present without synchronization when supported.
    Uncapped,
    /// Synchronize presentation to the display refresh rate.
    #[default]
    VSync,
}

/// Dynamic internal-resolution policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DynamicResolutionConfig {
    /// Whether automatic scaling is active.
    pub enabled: bool,
    /// Target frames per second used to derive the frame budget.
    pub target_fps: u32,
    /// Minimum internal linear resolution scale.
    pub min_scale: f32,
    /// Maximum internal linear resolution scale.
    pub max_scale: f32,
    /// Scale adjustment applied after a sustained budget miss.
    pub step: f32,
    /// Frames sampled before changing scale.
    pub sample_window: u32,
}

impl Default for DynamicResolutionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            target_fps: 120,
            min_scale: 0.5,
            max_scale: 1.0,
            step: 0.05,
            sample_window: 30,
        }
    }
}

/// Native runtime output policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderPerformanceConfig {
    /// Presentation strategy.
    pub present_strategy: PresentStrategy,
    /// Maximum queued surface frames.
    pub maximum_frame_latency: u32,
    /// Initial internal linear resolution scale.
    pub render_scale: f32,
    /// Dynamic-resolution policy.
    pub dynamic_resolution: DynamicResolutionConfig,
}

impl Default for RenderPerformanceConfig {
    fn default() -> Self {
        Self {
            present_strategy: PresentStrategy::VSync,
            maximum_frame_latency: 2,
            render_scale: 1.0,
            dynamic_resolution: DynamicResolutionConfig::default(),
        }
    }
}

impl RenderPerformanceConfig {
    /// Competitive high-refresh policy targeting a 120 Hz output.
    pub fn competitive_120hz() -> Self {
        Self {
            present_strategy: PresentStrategy::LowLatency,
            maximum_frame_latency: 1,
            render_scale: 1.0,
            dynamic_resolution: DynamicResolutionConfig {
                enabled: true,
                ..DynamicResolutionConfig::default()
            },
        }
    }
}

/// Latest performance measurements reported by a render backend.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderPerformanceMetrics {
    /// CPU time spent preparing and submitting the most recent render frame.
    pub render_cpu_ms: f32,
    /// Native output width.
    pub output_width: u32,
    /// Native output height.
    pub output_height: u32,
    /// Internal rendering width.
    pub internal_width: u32,
    /// Internal rendering height.
    pub internal_height: u32,
    /// Active internal linear resolution scale.
    pub render_scale: f32,
}

/// Stateful controller for dynamic internal resolution.
#[derive(Clone, Debug)]
pub struct DynamicResolutionController {
    config: DynamicResolutionConfig,
    current_scale: f32,
    accumulated_ms: f32,
    samples: u32,
}

impl DynamicResolutionController {
    /// Creates a controller using the configured initial scale.
    pub fn new(config: DynamicResolutionConfig, initial_scale: f32) -> Self {
        Self {
            config,
            current_scale: clamp_scale(initial_scale, config),
            accumulated_ms: 0.0,
            samples: 0,
        }
    }

    /// Returns the current internal linear scale.
    pub fn scale(&self) -> f32 {
        self.current_scale
    }

    /// Records a frame duration and returns a new scale when it changes.
    pub fn record_frame(&mut self, frame_ms: f32) -> Option<f32> {
        if !self.config.enabled || !frame_ms.is_finite() || frame_ms <= 0.0 {
            return None;
        }
        self.accumulated_ms += frame_ms;
        self.samples += 1;
        if self.samples < self.config.sample_window.max(1) {
            return None;
        }

        let average_ms = self.accumulated_ms / self.samples as f32;
        self.accumulated_ms = 0.0;
        self.samples = 0;
        let budget_ms = 1000.0 / self.config.target_fps.max(1) as f32;
        let previous = self.current_scale;
        if average_ms > budget_ms * 1.05 {
            self.current_scale -= self.config.step;
        } else if average_ms < budget_ms * 0.85 {
            self.current_scale += self.config.step;
        }
        self.current_scale = clamp_scale(self.current_scale, self.config);
        (self.current_scale != previous).then_some(self.current_scale)
    }
}

fn clamp_scale(scale: f32, config: DynamicResolutionConfig) -> f32 {
    scale.clamp(
        config.min_scale.max(0.1),
        config.max_scale.max(config.min_scale),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn competitive_policy_targets_low_latency_120hz() {
        let config = RenderPerformanceConfig::competitive_120hz();
        assert_eq!(config.present_strategy, PresentStrategy::LowLatency);
        assert_eq!(config.maximum_frame_latency, 1);
        assert_eq!(config.dynamic_resolution.target_fps, 120);
        assert!(config.dynamic_resolution.enabled);
    }

    #[test]
    fn dynamic_resolution_reacts_to_sustained_budget_pressure() {
        let mut controller = DynamicResolutionController::new(
            DynamicResolutionConfig {
                enabled: true,
                sample_window: 2,
                step: 0.1,
                ..Default::default()
            },
            1.0,
        );
        assert_eq!(controller.record_frame(12.0), None);
        assert_eq!(controller.record_frame(12.0), Some(0.9));
        assert_eq!(controller.record_frame(4.0), None);
        assert_eq!(controller.record_frame(4.0), Some(1.0));
    }
}
