//! Integration test for live built-in spatial upscaling.

use engine_render::{
    RenderDevice, RenderPlatformClass, RenderQualityMode, RenderScalingContext,
    RenderScalingSettings, RenderWorld, UpscalerKind,
};
use engine_render_wgpu::{WgpuOffscreenConfig, WgpuRenderDevice};

#[test]
fn live_spatial_upscaling_renders_internal_resolution_to_output_resolution() {
    let Ok(mut device) = WgpuRenderDevice::new_offscreen(WgpuOffscreenConfig {
        width: 160,
        height: 90,
        format: engine_render::ImageFormat::Rgba8Srgb,
    }) else {
        eprintln!("skipping wgpu upscaling test: no compatible adapter");
        return;
    };

    let selection = device.configure_render_scaling(
        &RenderScalingSettings {
            quality: RenderQualityMode::Performance,
            preferred_upscaler: Some(UpscalerKind::BuiltInSpatial),
            dynamic_resolution: false,
            sharpness: 0.5,
            ..Default::default()
        },
        RenderScalingContext {
            platform: RenderPlatformClass::Desktop,
            ..Default::default()
        },
    );
    assert_eq!(selection.render_scale, 0.5);

    device
        .render_world_offscreen(&RenderWorld::default())
        .expect("spatial upscale render should succeed");
    let metrics = device.performance_metrics();
    assert_eq!((metrics.internal_width, metrics.internal_height), (80, 45));
    assert_eq!((metrics.output_width, metrics.output_height), (160, 90));
    assert_eq!(metrics.upscaler, UpscalerKind::BuiltInSpatial);

    let (width, height, pixels) = device
        .readback_default_target()
        .expect("upscaled output should be readable");
    assert_eq!((width, height), (160, 90));
    assert_eq!(pixels.len(), 160 * 90 * 4);

    let native = device.configure_render_scaling(
        &RenderScalingSettings {
            quality: RenderQualityMode::Native,
            preferred_upscaler: Some(UpscalerKind::Native),
            dynamic_resolution: false,
            ..Default::default()
        },
        RenderScalingContext {
            platform: RenderPlatformClass::Desktop,
            ..Default::default()
        },
    );
    assert_eq!(native.render_scale, 1.0);
    device
        .render_world_offscreen(&RenderWorld::default())
        .expect("native render after live settings change should succeed");
    let metrics = device.performance_metrics();
    assert_eq!((metrics.internal_width, metrics.internal_height), (160, 90));
    assert_eq!(metrics.upscaler, UpscalerKind::Native);
}
