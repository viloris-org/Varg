//! Platform-independent render scaling capabilities and data contracts.

use serde::{Deserialize, Serialize};

use crate::ImageHandle;

/// User-facing render quality preset.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderQualityMode {
    /// Render at output resolution.
    Native,
    /// Highest quality upscale preset.
    UltraQuality,
    /// Quality-oriented upscale preset.
    Quality,
    /// Balanced quality and performance.
    #[default]
    Balanced,
    /// Performance-oriented upscale preset.
    Performance,
    /// Lowest internal resolution preset.
    UltraPerformance,
    /// Let capability negotiation choose a preset.
    Auto,
}

/// Upscaling implementation selected for a frame.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpscalerKind {
    /// No upscaling.
    #[default]
    Native,
    /// Aster's portable spatial fallback.
    BuiltInSpatial,
    /// Aster's portable temporal path.
    BuiltInTemporal,
    /// AMD FidelityFX Super Resolution.
    Fsr,
    /// NVIDIA Deep Learning Super Sampling.
    Dlss,
    /// Intel Xe Super Sampling.
    Xess,
    /// Apple MetalFX.
    MetalFx,
    /// Qualcomm Game Super Resolution.
    SnapdragonGsr,
    /// Microsoft DirectSR.
    DirectSr,
    /// NVIDIA Streamline integration.
    Streamline,
}

/// Frame generation implementation selected independently from upscaling.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FrameGenerationKind {
    /// Frame generation is disabled.
    #[default]
    Disabled,
    /// AMD frame generation.
    Fsr,
    /// NVIDIA frame generation.
    Dlss,
    /// Intel frame generation.
    Xess,
    /// Apple MetalFX frame interpolation.
    MetalFx,
}

/// Broad platform class used by automatic policy selection.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderPlatformClass {
    /// Desktop or workstation.
    #[default]
    Desktop,
    /// Battery-powered handheld PC.
    Handheld,
    /// Android device.
    Android,
    /// iPhone or iPad.
    AppleMobile,
    /// Windows on Arm device.
    WindowsOnArm,
    /// Headless runtime.
    Headless,
}

impl RenderPlatformClass {
    /// Returns whether this class uses mobile-first defaults.
    pub const fn is_mobile(self) -> bool {
        matches!(
            self,
            Self::Handheld | Self::Android | Self::AppleMobile | Self::WindowsOnArm
        )
    }
}

/// Current device thermal condition.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThermalState {
    /// No thermal constraint is reported.
    #[default]
    Nominal,
    /// Device is warming and should avoid increasing load.
    Warm,
    /// Device is throttling.
    Throttling,
    /// Device is under severe thermal pressure.
    Critical,
}

/// Battery policy supplied to automatic selection.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BatteryPolicy {
    /// Prefer image quality.
    Quality,
    /// Balance battery and image quality.
    #[default]
    Balanced,
    /// Prefer lower power consumption.
    Saver,
}

/// UI composition position relative to upscaling and frame generation.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UiCompositionPolicy {
    /// Compose UI after upscale and after generated frames.
    #[default]
    AfterFrameGeneration,
    /// Compose UI after upscale but include it in frame generation.
    BeforeFrameGeneration,
    /// Backend-specific UI separation is available.
    SeparateTexture,
}

/// Project-facing scaling and frame generation settings.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct RenderScalingSettings {
    /// Requested quality preset.
    pub quality: RenderQualityMode,
    /// Preferred upscaler, or `None` for automatic selection.
    pub preferred_upscaler: Option<UpscalerKind>,
    /// Whether dynamic resolution may adjust the internal size.
    pub dynamic_resolution: bool,
    /// Minimum permitted linear internal resolution scale.
    pub min_render_scale: f32,
    /// Maximum permitted linear internal resolution scale.
    pub max_render_scale: f32,
    /// Post-upscale sharpening strength in the inclusive range 0 to 1.
    pub sharpness: f32,
    /// Target frame rate.
    pub target_fps: u32,
    /// Battery policy.
    pub battery_policy: BatteryPolicy,
    /// Requested frame generation implementation.
    pub frame_generation: FrameGenerationKind,
    /// UI composition policy.
    pub ui_composition: UiCompositionPolicy,
}

impl Default for RenderScalingSettings {
    fn default() -> Self {
        Self {
            quality: RenderQualityMode::Balanced,
            preferred_upscaler: None,
            dynamic_resolution: true,
            min_render_scale: 0.5,
            max_render_scale: 1.0,
            sharpness: 0.35,
            target_fps: 60,
            battery_policy: BatteryPolicy::Balanced,
            frame_generation: FrameGenerationKind::Disabled,
            ui_composition: UiCompositionPolicy::AfterFrameGeneration,
        }
    }
}

impl RenderScalingSettings {
    /// Conservative defaults for phones, tablets, and mobile-adjacent devices.
    pub fn mobile() -> Self {
        Self {
            min_render_scale: 0.4,
            battery_policy: BatteryPolicy::Saver,
            ..Self::default()
        }
    }

    /// Normalizes user-provided bounds and target values.
    pub fn normalized(mut self) -> Self {
        self.min_render_scale = finite_scale(self.min_render_scale, 0.5).clamp(0.25, 1.0);
        self.max_render_scale =
            finite_scale(self.max_render_scale, 1.0).clamp(self.min_render_scale, 1.0);
        self.sharpness = finite_scale(self.sharpness, 0.35).clamp(0.0, 1.0);
        self.target_fps = self.target_fps.max(1);
        self
    }
}

fn finite_scale(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

/// Runtime conditions that influence automatic selection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderScalingContext {
    /// Platform class.
    pub platform: RenderPlatformClass,
    /// Current thermal condition.
    pub thermal_state: ThermalState,
    /// Whether the OS reports battery saver mode.
    pub battery_saver: bool,
}

/// Capability reported by one upscaler adapter.
#[derive(Clone, Debug, PartialEq)]
pub struct UpscalerCapability {
    /// Implementation kind.
    pub kind: UpscalerKind,
    /// Whether the implementation can be selected.
    pub available: bool,
    /// Human-readable availability explanation.
    pub reason: String,
    /// Minimum supported linear render scale.
    pub min_render_scale: f32,
    /// Whether temporal frame data is required.
    pub requires_temporal_inputs: bool,
}

impl UpscalerCapability {
    /// Creates an available capability.
    pub fn available(kind: UpscalerKind, min_render_scale: f32, temporal: bool) -> Self {
        Self {
            kind,
            available: true,
            reason: "available".to_owned(),
            min_render_scale,
            requires_temporal_inputs: temporal,
        }
    }

    /// Creates an unavailable capability with an editor-facing reason.
    pub fn unavailable(kind: UpscalerKind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            available: false,
            reason: reason.into(),
            min_render_scale: 1.0,
            requires_temporal_inputs: false,
        }
    }
}

/// Capability reported by one frame generation adapter.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameGenerationCapability {
    /// Implementation kind.
    pub kind: FrameGenerationKind,
    /// Whether the implementation can be selected.
    pub available: bool,
    /// Human-readable availability explanation.
    pub reason: String,
    /// Maximum generated-frame multiplier.
    pub max_multiplier: u8,
}

/// Complete backend scaling capability snapshot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderScalingCapabilities {
    /// Upscaling implementations known to the backend.
    pub upscalers: Vec<UpscalerCapability>,
    /// Frame generation implementations known to the backend.
    pub frame_generators: Vec<FrameGenerationCapability>,
}

impl RenderScalingCapabilities {
    /// Portable capabilities guaranteed by Aster's base renderer.
    pub fn built_in() -> Self {
        Self {
            upscalers: vec![
                UpscalerCapability::available(UpscalerKind::Native, 1.0, false),
                UpscalerCapability::available(UpscalerKind::BuiltInSpatial, 0.25, false),
                UpscalerCapability::unavailable(
                    UpscalerKind::BuiltInTemporal,
                    "temporal upscaler is not implemented by this backend",
                ),
            ],
            frame_generators: Vec::new(),
        }
    }

    /// Finds an upscaler capability.
    pub fn upscaler(&self, kind: UpscalerKind) -> Option<&UpscalerCapability> {
        self.upscalers.iter().find(|entry| entry.kind == kind)
    }
}

/// Result of runtime capability negotiation.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderScalingSelection {
    /// Selected upscaler.
    pub upscaler: UpscalerKind,
    /// Selected quality preset.
    pub quality: RenderQualityMode,
    /// Initial normalized linear render scale.
    pub render_scale: f32,
    /// Frame generation selection.
    pub frame_generation: FrameGenerationKind,
    /// Human-readable fallback or selection explanation.
    pub reason: String,
}

/// Chooses a supported implementation and conservative render scale.
pub fn negotiate_render_scaling(
    settings: &RenderScalingSettings,
    capabilities: &RenderScalingCapabilities,
    context: RenderScalingContext,
) -> RenderScalingSelection {
    let settings = settings.clone().normalized();
    let requested = settings.preferred_upscaler.unwrap_or_else(|| {
        if settings.quality == RenderQualityMode::Native {
            UpscalerKind::Native
        } else {
            UpscalerKind::BuiltInSpatial
        }
    });
    let requested_capability = capabilities.upscaler(requested);
    let (upscaler, reason) = match requested_capability {
        Some(capability) if capability.available => (requested, capability.reason.clone()),
        Some(capability) => (
            fallback_upscaler(capabilities),
            format!(
                "{requested:?} unavailable: {}; using portable fallback",
                capability.reason
            ),
        ),
        None => (
            fallback_upscaler(capabilities),
            format!("{requested:?} was not reported; using portable fallback"),
        ),
    };

    let mut render_scale = quality_scale(settings.quality);
    if context.platform.is_mobile()
        && (context.battery_saver
            || settings.battery_policy == BatteryPolicy::Saver
            || context.thermal_state != ThermalState::Nominal)
    {
        render_scale -= 0.1;
    }
    if matches!(
        context.thermal_state,
        ThermalState::Throttling | ThermalState::Critical
    ) {
        render_scale -= 0.1;
    }
    let capability_min = capabilities
        .upscaler(upscaler)
        .map_or(settings.min_render_scale, |capability| {
            capability.min_render_scale
        });
    let effective_min = settings
        .min_render_scale
        .max(capability_min)
        .min(settings.max_render_scale);
    render_scale = render_scale.clamp(effective_min, settings.max_render_scale);
    if upscaler == UpscalerKind::Native {
        render_scale = 1.0;
    }

    let frame_generation = if settings.frame_generation == FrameGenerationKind::Disabled {
        FrameGenerationKind::Disabled
    } else {
        capabilities
            .frame_generators
            .iter()
            .find(|entry| entry.kind == settings.frame_generation && entry.available)
            .map_or(FrameGenerationKind::Disabled, |entry| entry.kind)
    };

    RenderScalingSelection {
        upscaler,
        quality: settings.quality,
        render_scale,
        frame_generation,
        reason,
    }
}

fn fallback_upscaler(capabilities: &RenderScalingCapabilities) -> UpscalerKind {
    for kind in [UpscalerKind::BuiltInSpatial, UpscalerKind::Native] {
        if capabilities
            .upscaler(kind)
            .is_some_and(|capability| capability.available)
        {
            return kind;
        }
    }
    UpscalerKind::Native
}

fn quality_scale(quality: RenderQualityMode) -> f32 {
    match quality {
        RenderQualityMode::Native => 1.0,
        RenderQualityMode::UltraQuality => 0.77,
        RenderQualityMode::Quality => 0.67,
        RenderQualityMode::Balanced | RenderQualityMode::Auto => 0.59,
        RenderQualityMode::Performance => 0.5,
        RenderQualityMode::UltraPerformance => 0.33,
    }
}

/// Per-frame camera data required by temporal upscalers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TemporalCameraData {
    /// Current sub-pixel jitter in render pixels.
    pub jitter: [f32; 2],
    /// Current view-projection matrix.
    pub view_projection: [f32; 16],
    /// Previous frame view-projection matrix.
    pub previous_view_projection: [f32; 16],
    /// Camera near plane.
    pub near: f32,
    /// Camera far plane.
    pub far: f32,
}

/// Complete image and metadata contract for an upscaler invocation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UpscalerFrameData {
    /// Low-resolution color input.
    pub color: ImageHandle,
    /// Full-resolution output target.
    pub output: ImageHandle,
    /// Low-resolution depth input.
    pub depth: ImageHandle,
    /// Per-pixel motion vectors, when required.
    pub motion_vectors: Option<ImageHandle>,
    /// Reactive mask, when supported.
    pub reactive_mask: Option<ImageHandle>,
    /// Transparency mask, when supported.
    pub transparency_mask: Option<ImageHandle>,
    /// Exposure multiplier or luminance metadata.
    pub exposure: f32,
    /// Camera and history transform metadata.
    pub camera: TemporalCameraData,
    /// Monotonic frame index.
    pub frame_index: u64,
    /// Internal render size.
    pub render_size: (u32, u32),
    /// Display output size.
    pub output_size: (u32, u32),
    /// Invalidate accumulated history before processing this frame.
    pub reset_history: bool,
}

/// Deep adapter boundary for built-in and vendor upscalers.
pub trait UpscalerBackend {
    /// Reports adapter availability and requirements.
    fn capability(&self) -> UpscalerCapability;

    /// Processes one frame using the common data contract.
    fn upscale(&mut self, frame: &UpscalerFrameData) -> engine_core::EngineResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_json() {
        let settings = RenderScalingSettings::mobile();
        let json = serde_json::to_string(&settings).unwrap();
        let decoded: RenderScalingSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, settings);
    }

    #[test]
    fn unsupported_vendor_falls_back_with_reason() {
        let settings = RenderScalingSettings {
            preferred_upscaler: Some(UpscalerKind::Dlss),
            ..RenderScalingSettings::default()
        };
        let capabilities = RenderScalingCapabilities::built_in();
        let selected =
            negotiate_render_scaling(&settings, &capabilities, RenderScalingContext::default());
        assert_eq!(selected.upscaler, UpscalerKind::BuiltInSpatial);
        assert!(selected.reason.contains("was not reported"));
    }

    #[test]
    fn mobile_thermal_pressure_uses_more_conservative_scale() {
        let settings = RenderScalingSettings::mobile();
        let capabilities = RenderScalingCapabilities::built_in();
        let nominal = negotiate_render_scaling(
            &settings,
            &capabilities,
            RenderScalingContext {
                platform: RenderPlatformClass::Android,
                ..Default::default()
            },
        );
        let throttled = negotiate_render_scaling(
            &settings,
            &capabilities,
            RenderScalingContext {
                platform: RenderPlatformClass::Android,
                thermal_state: ThermalState::Throttling,
                battery_saver: true,
            },
        );
        assert!(throttled.render_scale < nominal.render_scale);
        assert!(throttled.render_scale >= settings.min_render_scale);
    }

    #[test]
    fn frame_generation_is_negotiated_separately() {
        let settings = RenderScalingSettings {
            frame_generation: FrameGenerationKind::Dlss,
            ..Default::default()
        };
        let selected = negotiate_render_scaling(
            &settings,
            &RenderScalingCapabilities::built_in(),
            RenderScalingContext::default(),
        );
        assert_eq!(selected.frame_generation, FrameGenerationKind::Disabled);
    }

    #[test]
    fn capability_minimum_above_project_max_does_not_panic() {
        let settings = RenderScalingSettings {
            quality: RenderQualityMode::Native,
            max_render_scale: 0.8,
            ..Default::default()
        };
        let selected = negotiate_render_scaling(
            &settings,
            &RenderScalingCapabilities::built_in(),
            RenderScalingContext::default(),
        );
        assert_eq!(selected.upscaler, UpscalerKind::Native);
        assert_eq!(selected.render_scale, 1.0);
    }
}
