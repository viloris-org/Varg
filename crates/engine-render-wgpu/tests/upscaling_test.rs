//! Integration test for live built-in spatial upscaling.

use engine_core::math::{Transform, Vec3};
use engine_render::{
    RenderCamera, RenderDevice, RenderFrame, RenderLight, RenderLightKind, RenderPlatformClass,
    RenderQualityMode, RenderScalingContext, RenderScalingSettings, RenderWorld, UpscalerKind,
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
    assert_eq!(device.motion_vector_target_size(), Some((80, 45)));

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
    assert_eq!(device.motion_vector_target_size(), Some((160, 90)));
}

#[test]
fn offscreen_render_updates_temporal_camera_metadata() {
    let Ok(mut device) = WgpuRenderDevice::new_offscreen(WgpuOffscreenConfig {
        width: 160,
        height: 90,
        format: engine_render::ImageFormat::Rgba8Srgb,
    }) else {
        eprintln!("skipping wgpu temporal test: no compatible adapter");
        return;
    };

    let mut world = RenderWorld {
        camera: Some(RenderCamera {
            object: engine_core::EntityId::from_u128(1),
            transform: Transform {
                translation: Vec3::new(0.0, 0.0, 5.0),
                ..Transform::default()
            },
            projection: engine_render::RenderProjection::Perspective,
            vertical_fov_degrees: 60.0,
            near: 0.25,
            far: 250.0,
            look_at_target: Some(Vec3::ZERO),
        }),
        ..RenderWorld::default()
    };

    device
        .submit_render_world(&world, RenderFrame { frame_index: 0 })
        .expect("first temporal render should succeed");
    let (first, first_reset) = device.latest_temporal_camera();
    assert!(first_reset);
    assert_eq!(first.near, 0.25);
    assert_eq!(first.far, 250.0);

    if let Some(camera) = world.camera.as_mut() {
        camera.look_at_target = Some(Vec3::new(1.0, 0.0, 0.0));
    }
    device
        .submit_render_world(&world, RenderFrame { frame_index: 1 })
        .expect("second temporal render should succeed");
    let (second, second_reset) = device.latest_temporal_camera();
    assert!(!second_reset);
    assert_eq!(second.previous_view_projection, first.view_projection);

    device
        .resize_default_target(80, 45)
        .expect("target resize should succeed");
    device
        .submit_render_world(&world, RenderFrame { frame_index: 2 })
        .expect("resized temporal render should succeed");
    let (_, resize_reset) = device.latest_temporal_camera();
    assert!(resize_reset);
    assert_eq!(device.motion_vector_target_size(), Some((80, 45)));
}

#[test]
fn mobile_vendor_upscaler_request_uses_portable_wgpu_fallback() {
    let Ok(mut device) = WgpuRenderDevice::new_offscreen(WgpuOffscreenConfig {
        width: 160,
        height: 90,
        format: engine_render::ImageFormat::Rgba8Srgb,
    }) else {
        eprintln!("skipping wgpu mobile fallback test: no compatible adapter");
        return;
    };

    let selection = device.configure_render_scaling(
        &RenderScalingSettings {
            preferred_upscaler: Some(UpscalerKind::MetalFx),
            ..RenderScalingSettings::mobile()
        },
        RenderScalingContext {
            platform: RenderPlatformClass::AppleMobile,
            ..Default::default()
        },
    );

    assert_eq!(selection.upscaler, UpscalerKind::BuiltInSpatial);
    assert!(selection.reason.contains("MetalFx unavailable"));
}

#[test]
fn light_budget_pressure_is_reported_in_metrics() {
    let Ok(mut device) = WgpuRenderDevice::new_offscreen(WgpuOffscreenConfig {
        width: 160,
        height: 90,
        format: engine_render::ImageFormat::Rgba8Srgb,
    }) else {
        eprintln!("skipping wgpu light metrics test: no compatible adapter");
        return;
    };

    let lights = (0..40)
        .map(|index| RenderLight {
            object: engine_core::EntityId::from_u128(index + 1),
            transform: Transform {
                translation: Vec3::new(index as f32 * 0.25, 0.0, -3.0),
                ..Transform::default()
            },
            kind: RenderLightKind::Point,
            color: Vec3::ONE,
            intensity: 1.0,
            range: 6.0,
            spot_angle: 45.0,
        })
        .collect();
    let world = RenderWorld {
        lights,
        ..RenderWorld::default()
    };

    device
        .submit_render_world(&world, RenderFrame { frame_index: 0 })
        .expect("light-budget render should succeed");
    let metrics = device.performance_metrics();
    assert_eq!(metrics.submitted_lights, 40);
    assert_eq!(metrics.visible_lights, 32);
    assert_eq!(metrics.culled_lights, 8);
}
