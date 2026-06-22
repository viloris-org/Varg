use engine_core::math::{Transform, Vec3};
use engine_render::{
    GuiDrawCmd, GuiDrawList, GuiVertex, ImageDesc, ImageFormat, RenderBounds, RenderCamera,
    RenderDevice, RenderFrame, RenderGlobalIllumination, RenderGraphBuilder, RenderLightingMode,
    RenderObject, RenderParticleEmitter, RenderProbeVolume, RenderProjection,
    RenderShadowVirtualization, RenderWorld,
};
use engine_render_wgpu::{WgpuOffscreenConfig, WgpuRenderDevice};

fn renderer() -> Option<WgpuRenderDevice> {
    WgpuRenderDevice::new_offscreen(WgpuOffscreenConfig {
        width: 128,
        height: 72,
        format: ImageFormat::Rgba8Srgb,
    })
    .ok()
}

fn camera() -> RenderCamera {
    RenderCamera {
        object: engine_core::EntityId::from_u128(1),
        transform: Transform::IDENTITY,
        projection: RenderProjection::Perspective,
        vertical_fov_degrees: 60.0,
        near: 0.1,
        far: 100.0,
        look_at_target: Some(Vec3::new(0.0, 0.0, -1.0)),
    }
}

fn object(id: u128, position: Vec3) -> RenderObject {
    RenderObject {
        object: engine_core::EntityId::from_u128(id),
        transform: Transform {
            translation: position,
            ..Transform::IDENTITY
        },
        mesh: "debug/cube".to_owned(),
        material: "debug/default".to_owned(),
        casts_shadows: true,
        receive_shadows: true,
        bounds: RenderBounds::default(),
        lods: Vec::new(),
    }
}

#[test]
fn compiled_frame_pipeline_controls_wgpu_passes_and_visibility_metrics() {
    let Some(mut device) = renderer() else {
        eprintln!("skipping P0 graph test: no compatible adapter");
        return;
    };
    let world = RenderWorld {
        camera: Some(camera()),
        objects: vec![
            object(2, Vec3::new(0.0, 0.0, -4.0)),
            object(3, Vec3::new(0.0, 0.0, 4.0)),
        ],
        ..RenderWorld::default()
    };
    let mut builder = RenderGraphBuilder::new();
    let forward = builder.add_pass("forward");
    let post = builder.add_pass("post");
    builder.order_before(forward, post);
    let graph = builder.build();
    device
        .submit_render_world_with_graph(&world, &graph, RenderFrame { frame_index: 0 })
        .unwrap();
    let metrics = device.performance_metrics();
    assert_eq!(metrics.pipeline_passes, 2);
    assert_eq!(metrics.submitted_objects, 2);
    assert_eq!(metrics.visible_objects, 1);
    assert_eq!(metrics.culled_objects, 1);
    assert!(metrics.draw_calls >= 3);
    assert!(metrics.triangles > 0);
}

#[test]
fn hybrid_deferred_graph_reports_pipeline_metrics() {
    let Some(mut device) = renderer() else {
        eprintln!("skipping P3 hybrid deferred graph test: no compatible adapter");
        return;
    };
    let world = RenderWorld {
        camera: Some(camera()),
        objects: vec![object(2, Vec3::new(0.0, 0.0, -4.0))],
        lighting_mode: RenderLightingMode::HybridDeferred,
        ..RenderWorld::default()
    };
    let mut builder = RenderGraphBuilder::new();
    let gbuffer = builder.add_pass("gbuffer");
    let deferred = builder.add_pass("deferred-lighting");
    let post = builder.add_pass("post");
    builder.order_before(gbuffer, deferred);
    builder.order_before(deferred, post);
    let graph = builder.build();
    device
        .submit_render_world_with_graph(&world, &graph, RenderFrame { frame_index: 0 })
        .unwrap();
    let metrics = device.performance_metrics();
    assert_eq!(metrics.pipeline_passes, 3);
    assert!(metrics.hybrid_deferred);
    assert!(metrics.draw_calls >= 3);
}

#[test]
fn probe_volume_and_virtual_shadow_config_report_budget_metrics() {
    let Some(mut device) = renderer() else {
        eprintln!("skipping P4 lighting budget telemetry test: no compatible adapter");
        return;
    };
    let world = RenderWorld {
        camera: Some(camera()),
        global_illumination: RenderGlobalIllumination::ProbeVolume(RenderProbeVolume {
            counts: [2, 3, 4],
            ..RenderProbeVolume::default()
        }),
        shadow_virtualization: RenderShadowVirtualization::VirtualPages {
            page_size: 128,
            max_pages: 256,
        },
        ..RenderWorld::default()
    };
    device
        .submit_render_world(&world, RenderFrame { frame_index: 0 })
        .unwrap();
    let metrics = device.performance_metrics();
    assert_eq!(metrics.active_gi_probes, 24);
    assert_eq!(metrics.virtual_shadow_pages, 256);
}

#[test]
fn compiled_frame_pipeline_rejects_unsupported_passes() {
    let Some(mut device) = renderer() else {
        eprintln!("skipping P0 unsupported graph test: no compatible adapter");
        return;
    };
    let mut builder = RenderGraphBuilder::new();
    builder.add_pass("custom");
    let graph = builder.build();
    let err = device
        .submit_render_world_with_graph(
            &RenderWorld::default(),
            &graph,
            RenderFrame { frame_index: 0 },
        )
        .unwrap_err();
    assert!(err.to_string().contains("unsupported pass"));
}

#[test]
fn gui_and_gpu_skinning_encode_real_draws() {
    let Some(mut device) = renderer() else {
        eprintln!("skipping P0 GUI/skinning test: no compatible adapter");
        return;
    };
    device
        .render_world_offscreen(&RenderWorld::default())
        .unwrap();
    let texture = device
        .upload_gui_texture(
            ImageDesc::color_2d(1, 1, ImageFormat::Rgba8Unorm),
            &[255, 255, 255, 255],
        )
        .unwrap();
    device
        .draw_gui(&GuiDrawList {
            vertices: vec![
                GuiVertex {
                    pos: [4.0, 4.0],
                    uv: [0.0, 0.0],
                    color: u32::MAX,
                },
                GuiVertex {
                    pos: [40.0, 4.0],
                    uv: [1.0, 0.0],
                    color: u32::MAX,
                },
                GuiVertex {
                    pos: [4.0, 40.0],
                    uv: [0.0, 1.0],
                    color: u32::MAX,
                },
            ],
            indices: vec![0, 1, 2],
            commands: vec![GuiDrawCmd {
                texture,
                scissor: [0, 0, 128, 72],
                index_offset: 0,
                index_count: 3,
            }],
        })
        .unwrap();

    device
        .upload_skinned_mesh_data(
            "test/skinned",
            &[[-0.5, -0.5, -2.0], [0.5, -0.5, -2.0], [0.0, 0.5, -2.0]],
            &[[0.0, 0.0, 1.0]; 3],
            &[[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]],
            &[[0, 0, 0, 0]; 3],
            &[[1.0, 0.0, 0.0, 0.0]; 3],
            &[0, 1, 2],
        )
        .unwrap();
    let identity = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let bones = device.upload_bone_matrices(&[identity]).unwrap();
    let err = device
        .draw_skinned_mesh("test/skinned", "debug/default", bones, 2)
        .unwrap_err();
    assert!(err.to_string().contains("exceeds uploaded palette size"));
    device
        .draw_skinned_mesh("test/skinned", "debug/default", bones, 1)
        .unwrap();
    device.wait_idle().unwrap();
}

#[test]
fn gpu_particle_emitter_simulates_and_renders() {
    let Some(mut device) = renderer() else {
        eprintln!("skipping P0 particle test: no compatible adapter");
        return;
    };
    let world = RenderWorld {
        camera: Some(camera()),
        particle_emitters: vec![RenderParticleEmitter {
            object: engine_core::EntityId::from_u128(4),
            transform: Transform {
                translation: Vec3::new(0.0, 0.0, -3.0),
                ..Transform::IDENTITY
            },
            max_particles: 32,
            emission_rate: 16.0,
            lifetime: 2.0,
            start_speed: 1.0,
            size_range: [0.2, 0.05],
            start_color: [1.0, 0.8, 0.2, 1.0],
            end_color: [1.0, 0.1, 0.0, 0.0],
            gravity: Vec3::new(0.0, -1.0, 0.0),
            spread_degrees: 30.0,
            looping: true,
            seed: 7,
            elapsed: 1.0,
        }],
        ..RenderWorld::default()
    };
    device
        .submit_render_world(&world, RenderFrame { frame_index: 0 })
        .unwrap();
    device.wait_idle().unwrap();
    assert!(device.performance_metrics().triangles >= 64);
}

#[test]
fn gpu_timestamp_is_reported_when_supported() {
    let Some(mut device) = renderer() else {
        eprintln!("skipping P0 timestamp test: no compatible adapter");
        return;
    };
    if !device.output_capabilities().supports_timestamp_queries {
        return;
    }
    device
        .submit_render_world(&RenderWorld::default(), RenderFrame { frame_index: 0 })
        .unwrap();
    device.wait_idle().unwrap();
    device
        .submit_render_world(&RenderWorld::default(), RenderFrame { frame_index: 1 })
        .unwrap();
    assert!(device.performance_metrics().gpu_frame_ms.is_some());
}
