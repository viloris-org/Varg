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

/// Anti-aliasing algorithm applied before final post-processing.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AntiAliasingMode {
    /// Disable post-process anti-aliasing.
    Off,
    /// Temporal anti-aliasing using jitter, motion vectors, and history reprojection.
    #[default]
    Taa,
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
    /// Anti-aliasing algorithm.
    pub anti_aliasing: AntiAliasingMode,
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
            anti_aliasing: AntiAliasingMode::Taa,
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
    if value.is_finite() { value } else { fallback }
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

/// Backend family used by mobile vendor prototype adapters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MobileVendorAdapter {
    /// Apple MetalFX upscaling.
    MetalFx,
    /// Qualcomm Snapdragon Game Super Resolution.
    SnapdragonGsr,
}

impl MobileVendorAdapter {
    /// Reports capability for a platform-gated mobile vendor integration shell.
    pub fn capability(self, context: RenderScalingContext) -> UpscalerCapability {
        match self {
            Self::MetalFx if context.platform == RenderPlatformClass::AppleMobile => {
                UpscalerCapability::unavailable(
                    UpscalerKind::MetalFx,
                    "MetalFX requires a native Metal adapter; wgpu handle integration is not available",
                )
            }
            Self::MetalFx => UpscalerCapability::unavailable(
                UpscalerKind::MetalFx,
                "MetalFX is only available on Apple mobile platforms",
            ),
            Self::SnapdragonGsr
                if matches!(
                    context.platform,
                    RenderPlatformClass::Android | RenderPlatformClass::WindowsOnArm
                ) =>
            {
                UpscalerCapability::unavailable(
                    UpscalerKind::SnapdragonGsr,
                    "Snapdragon GSR SDK/runtime detection is not configured",
                )
            }
            Self::SnapdragonGsr => UpscalerCapability::unavailable(
                UpscalerKind::SnapdragonGsr,
                "Snapdragon GSR is only considered for Android and Windows on Arm",
            ),
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

    /// Built-in capabilities plus mobile vendor prototype adapter boundaries.
    pub fn mobile_prototype(context: RenderScalingContext) -> Self {
        let mut capabilities = Self::built_in();
        capabilities
            .upscalers
            .push(MobileVendorAdapter::MetalFx.capability(context));
        capabilities
            .upscalers
            .push(MobileVendorAdapter::SnapdragonGsr.capability(context));
        capabilities
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

impl Default for TemporalCameraData {
    fn default() -> Self {
        Self {
            jitter: [0.0, 0.0],
            view_projection: identity_matrix(),
            previous_view_projection: identity_matrix(),
            near: 0.1,
            far: 1000.0,
        }
    }
}

/// Stateful temporal metadata generator for TAA and temporal upscalers.
#[derive(Clone, Debug, PartialEq)]
pub struct TemporalFrameState {
    previous_view_projection: Option<[f32; 16]>,
    previous_render_size: Option<(u32, u32)>,
    sequence_index: u32,
}

impl Default for TemporalFrameState {
    fn default() -> Self {
        Self::new()
    }
}

impl TemporalFrameState {
    /// Creates temporal state with no frame history.
    pub const fn new() -> Self {
        Self {
            previous_view_projection: None,
            previous_render_size: None,
            sequence_index: 0,
        }
    }

    /// Invalidates accumulated history before the next frame.
    pub fn reset(&mut self) {
        self.previous_view_projection = None;
        self.previous_render_size = None;
        self.sequence_index = 0;
    }

    /// Builds temporal camera metadata and advances history.
    pub fn next_camera_data(
        &mut self,
        view_projection: [f32; 16],
        render_size: (u32, u32),
        near: f32,
        far: f32,
    ) -> (TemporalCameraData, bool) {
        let render_size = (render_size.0.max(1), render_size.1.max(1));
        let reset_history = self
            .previous_render_size
            .is_none_or(|previous| previous != render_size);
        let jitter = halton_jitter(self.sequence_index, render_size);
        let jittered_view_projection = jitter_view_projection(view_projection, jitter);
        let previous_view_projection = self
            .previous_view_projection
            .unwrap_or(jittered_view_projection);
        self.previous_view_projection = Some(jittered_view_projection);
        self.previous_render_size = Some(render_size);
        self.sequence_index = self.sequence_index.wrapping_add(1);
        (
            TemporalCameraData {
                jitter,
                view_projection: jittered_view_projection,
                previous_view_projection,
                near,
                far,
            },
            reset_history,
        )
    }
}

fn jitter_view_projection(mut view_projection: [f32; 16], jitter: [f32; 2]) -> [f32; 16] {
    view_projection[8] += jitter[0] * 2.0;
    view_projection[9] += jitter[1] * 2.0;
    view_projection
}

fn halton_jitter(index: u32, render_size: (u32, u32)) -> [f32; 2] {
    let sample = index + 1;
    [
        (halton(sample, 2) - 0.5) / render_size.0.max(1) as f32,
        (halton(sample, 3) - 0.5) / render_size.1.max(1) as f32,
    ]
}

fn halton(mut index: u32, base: u32) -> f32 {
    let mut result = 0.0;
    let mut fraction = 1.0 / base as f32;
    while index > 0 {
        result += (index % base) as f32 * fraction;
        index /= base;
        fraction /= base as f32;
    }
    result
}

fn identity_matrix() -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ]
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

    #[test]
    fn temporal_frame_state_tracks_previous_matrix_and_resize_resets_history() {
        let mut state = TemporalFrameState::new();
        let first_vp = [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ];
        let second_vp = [
            2.0, 0.0, 0.0, 0.0, //
            0.0, 2.0, 0.0, 0.0, //
            0.0, 0.0, 2.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ];

        let (first, first_reset) = state.next_camera_data(first_vp, (1920, 1080), 0.1, 1000.0);
        assert!(first_reset);
        assert_eq!(first.previous_view_projection, first.view_projection);
        assert_eq!(first.jitter[0], 0.0);
        assert!(first.jitter[1].abs() > 0.0);
        assert_eq!(
            first.view_projection[8],
            first_vp[8] + first.jitter[0] * 2.0
        );
        assert_eq!(
            first.view_projection[9],
            first_vp[9] + first.jitter[1] * 2.0
        );

        let (second, second_reset) = state.next_camera_data(second_vp, (1920, 1080), 0.1, 1000.0);
        assert!(!second_reset);
        assert_eq!(second.previous_view_projection, first.view_projection);

        let (_, resize_reset) = state.next_camera_data(second_vp, (1280, 720), 0.1, 1000.0);
        assert!(resize_reset);
    }

    #[test]
    fn mobile_vendor_prototype_reports_platform_specific_reasons() {
        let apple = RenderScalingCapabilities::mobile_prototype(RenderScalingContext {
            platform: RenderPlatformClass::AppleMobile,
            ..Default::default()
        });
        assert!(
            apple
                .upscaler(UpscalerKind::MetalFx)
                .unwrap()
                .reason
                .contains("native Metal adapter")
        );

        let android = RenderScalingCapabilities::mobile_prototype(RenderScalingContext {
            platform: RenderPlatformClass::Android,
            ..Default::default()
        });
        assert!(
            android
                .upscaler(UpscalerKind::SnapdragonGsr)
                .unwrap()
                .reason
                .contains("SDK/runtime detection")
        );
    }

    #[test]
    fn mobile_vendor_request_falls_back_to_built_in_spatial() {
        let context = RenderScalingContext {
            platform: RenderPlatformClass::Android,
            thermal_state: ThermalState::Warm,
            battery_saver: true,
        };
        let settings = RenderScalingSettings {
            preferred_upscaler: Some(UpscalerKind::SnapdragonGsr),
            ..RenderScalingSettings::mobile()
        };
        let selected = negotiate_render_scaling(
            &settings,
            &RenderScalingCapabilities::mobile_prototype(context),
            context,
        );

        assert_eq!(selected.upscaler, UpscalerKind::BuiltInSpatial);
        assert!(selected.reason.contains("SnapdragonGsr unavailable"));
        assert!(selected.render_scale >= settings.min_render_scale);
    }
}
