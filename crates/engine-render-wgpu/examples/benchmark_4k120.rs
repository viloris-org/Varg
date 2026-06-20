//! Native 4K render-capacity benchmark.
//!
//! Run with:
//! `cargo run -p engine-render-wgpu --release --example benchmark_4k120`

use std::time::Instant;

use engine_core::math::{Transform, Vec3};
use engine_render::{RenderCamera, RenderDevice, RenderFrame, RenderObject, RenderWorld};
use engine_render_wgpu::{WgpuOffscreenConfig, WgpuRenderDevice};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let frames = std::env::var("ASTER_BENCH_FRAMES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(240);
    let mut renderer = WgpuRenderDevice::new_offscreen(WgpuOffscreenConfig {
        width: 3840,
        height: 2160,
        format: engine_render::ImageFormat::Rgba8Srgb,
    })?;
    let capabilities = renderer.output_capabilities();
    if !capabilities.supports_4k_render_targets {
        return Err(format!(
            "adapter {} only supports {}px 2D textures",
            capabilities.adapter_name, capabilities.max_texture_dimension_2d
        )
        .into());
    }

    let mut camera = Transform::IDENTITY;
    camera.translation = Vec3::new(0.0, 2.0, 6.0);
    let world = RenderWorld {
        camera: Some(RenderCamera {
            object: engine_core::EntityId::from_u128(1),
            transform: camera,
            projection: engine_render::RenderProjection::Perspective,
            vertical_fov_degrees: 60.0,
            near: 0.1,
            far: 1000.0,
            look_at_target: Some(Vec3::ZERO),
        }),
        objects: vec![RenderObject {
            object: engine_core::EntityId::from_u128(2),
            transform: Transform::IDENTITY,
            mesh: "debug/cube".to_owned(),
            material: "debug/default".to_owned(),
            casts_shadows: true,
            receive_shadows: true,
        }],
        ..RenderWorld::default()
    };

    for frame_index in 0..16 {
        renderer.submit_render_world(&world, RenderFrame { frame_index })?;
    }
    renderer.wait_idle()?;

    let started = Instant::now();
    for frame_index in 0..frames {
        renderer.submit_render_world(&world, RenderFrame { frame_index })?;
    }
    renderer.wait_idle()?;
    let elapsed = started.elapsed();
    let average_ms = elapsed.as_secs_f64() * 1000.0 / frames as f64;
    let fps = 1000.0 / average_ms;

    println!(
        "adapter={} resolution=3840x2160 frames={} average_ms={average_ms:.3} fps={fps:.1}",
        capabilities.adapter_name, frames
    );
    if average_ms > 8.333 {
        eprintln!(
            "result exceeds the 4K120 frame budget; use profiling and quality scaling on this adapter"
        );
    }
    Ok(())
}
