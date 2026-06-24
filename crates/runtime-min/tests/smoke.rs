use engine_core::EngineConfig;
use engine_ecs::{BuildConfiguration, BuildRenderSettings};
use engine_render::{
    AntiAliasingMode, FrameGenerationKind, RenderFrame, RenderGraphBuilder, RenderPlatformClass,
    RenderQualityMode, RenderScalingContext, RenderScalingSettings, UiCompositionPolicy,
    UpscalerKind,
};
use runtime_min::{
    RuntimeServices, build_default_render_graph, render_scaling_settings_from_build,
    smoke_runtime_min,
};

#[test]
fn native_runtime_min_smoke_test() {
    assert_eq!(smoke_runtime_min().unwrap(), 1);
}

#[test]
fn default_render_graph_pass_order() {
    let graph = build_default_render_graph();
    assert_eq!(graph.pass_count(), 6);
    assert_eq!(graph.passes[0].name, "shadow");
    assert_eq!(graph.passes[1].name, "forward");
    assert_eq!(graph.passes[2].name, "temporal-inputs");
    assert_eq!(graph.passes[3].name, "upscale");
    assert_eq!(graph.passes[4].name, "post");
    assert_eq!(graph.passes[5].name, "ui");
}

#[test]
fn runtime_services_ticks_multiple_frames() {
    let mut services = RuntimeServices::minimal(EngineConfig::default());
    for _ in 0..5 {
        services.tick().unwrap();
    }
    assert_eq!(services.frame_index(), 5);
}

#[test]
fn player_render_settings_apply_without_recreating_runtime() {
    let mut services = RuntimeServices::minimal(EngineConfig::default());
    let selection = services.set_render_scaling(
        RenderScalingSettings {
            quality: RenderQualityMode::Performance,
            preferred_upscaler: Some(UpscalerKind::BuiltInSpatial),
            dynamic_resolution: false,
            ..Default::default()
        },
        RenderScalingContext {
            platform: RenderPlatformClass::Desktop,
            ..Default::default()
        },
    );
    assert_eq!(selection.upscaler, UpscalerKind::BuiltInSpatial);
    assert_eq!(selection.render_scale, 0.5);
    assert_eq!(services.stats.upscaler, UpscalerKind::BuiltInSpatial);
    assert_eq!(
        services.render_scaling_settings.quality,
        RenderQualityMode::Performance
    );
}

#[test]
fn build_render_settings_map_frame_generation_and_ui_policy() {
    let mut build = BuildConfiguration::runtime_min();
    build.render = BuildRenderSettings {
        quality: "quality".to_string(),
        upscaler: "dlss".to_string(),
        frame_generation: "dlss".to_string(),
        ui_composition: "separate-texture".to_string(),
        anti_aliasing: "off".to_string(),
        ..BuildRenderSettings::default()
    };

    let settings = render_scaling_settings_from_build(&build);
    assert_eq!(settings.quality, RenderQualityMode::Quality);
    assert_eq!(settings.preferred_upscaler, Some(UpscalerKind::Dlss));
    assert_eq!(settings.frame_generation, FrameGenerationKind::Dlss);
    assert_eq!(
        settings.ui_composition,
        UiCompositionPolicy::SeparateTexture
    );
    assert_eq!(settings.anti_aliasing, AntiAliasingMode::Off);

    let mut services = RuntimeServices::minimal(EngineConfig::default());
    let selection = services.set_render_scaling(settings, RenderScalingContext::default());
    assert_eq!(selection.upscaler, UpscalerKind::BuiltInSpatial);
    assert_eq!(selection.frame_generation, FrameGenerationKind::Disabled);
}

#[test]
fn mobile_vendor_request_falls_back_in_headless_runtime() {
    let mut services = RuntimeServices::minimal(EngineConfig::default());
    let selection = services.set_render_scaling(
        RenderScalingSettings {
            preferred_upscaler: Some(UpscalerKind::SnapdragonGsr),
            ..RenderScalingSettings::mobile()
        },
        RenderScalingContext {
            platform: RenderPlatformClass::Android,
            ..Default::default()
        },
    );

    assert_eq!(selection.upscaler, UpscalerKind::BuiltInSpatial);
    assert!(selection.reason.contains("SnapdragonGsr unavailable"));
}

#[test]
fn custom_render_graph_replaces_default() {
    let mut services = RuntimeServices::minimal(EngineConfig::default());
    let mut builder = RenderGraphBuilder::new();
    let a = builder.add_pass("deferred");
    let b = builder.add_pass("lighting");
    builder.order_before(a, b);
    services.set_render_graph(builder.build());
    assert_eq!(services.render_graph.pass_count(), 2);
    services.tick().unwrap();
}

/// Script-driven render path: a graph built from a description table.
#[test]
fn script_driven_render_path() {
    // Simulate what a script backend would do: build a graph from a list of
    // pass names and ordering rules, then execute it.
    let pass_names = ["shadow", "forward", "post"];
    let edges = [(0usize, 1usize), (1, 2)];

    let mut builder = RenderGraphBuilder::new();
    let ids: Vec<_> = pass_names.iter().map(|n| builder.add_pass(*n)).collect();
    for (before, after) in edges {
        builder.order_before(ids[before], ids[after]);
    }
    let graph = builder.build();

    assert_eq!(graph.pass_count(), 3);
    let names: Vec<&str> = graph.passes.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["shadow", "forward", "post"]);

    // Execute via headless device.
    use engine_render::{HeadlessRenderDevice, RenderDevice};
    let mut device = HeadlessRenderDevice::default();
    device
        .execute_graph(&graph, RenderFrame { frame_index: 0 })
        .unwrap();
}
