//! Editor viewport presentation path benchmark.
//!
//! Measures the two important local presentation paths:
//! - `canvas-readback`: render offscreen, synchronously read RGBA pixels to CPU.
//! - `native-surface`: render to a real window surface without CPU readback.
//!
//! Run with:
//! `cargo run -p engine-render-wgpu --release --example viewport_path_perf`
//!
//! Useful environment variables:
//! - `ASTER_VIEWPORT_PERF_WIDTH`, default `1280`
//! - `ASTER_VIEWPORT_PERF_HEIGHT`, default `720`
//! - `ASTER_VIEWPORT_PERF_FRAMES`, default `180`
//! - `ASTER_VIEWPORT_PERF_WARMUP`, default `16`

use std::time::{Duration, Instant};

use engine_core::math::{Transform, Vec3};
use engine_render::{
    RenderCamera, RenderDevice, RenderFrame, RenderLight, RenderLightKind, RenderObject,
    RenderPerformanceConfig, RenderWorld,
};
use engine_render_wgpu::{WgpuOffscreenConfig, WgpuRenderDevice};
use winit::dpi::PhysicalSize;
use winit::window::WindowAttributes;

#[derive(Clone, Copy, Debug)]
struct BenchConfig {
    width: u32,
    height: u32,
    frames: u64,
    warmup: u64,
}

#[derive(Clone, Debug)]
struct PathResult {
    path: &'static str,
    status: &'static str,
    average_ms: Option<f64>,
    p95_ms: Option<f64>,
    fps: Option<f64>,
    bytes_per_frame: u64,
    frames: u64,
    reason: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = BenchConfig {
        width: env_u32("ASTER_VIEWPORT_PERF_WIDTH", 1280),
        height: env_u32("ASTER_VIEWPORT_PERF_HEIGHT", 720),
        frames: env_u64("ASTER_VIEWPORT_PERF_FRAMES", 180),
        warmup: env_u64("ASTER_VIEWPORT_PERF_WARMUP", 16),
    };
    let world = benchmark_world();

    let mut results = Vec::new();
    results.push(measure_canvas_readback(config, &world));
    results.push(measure_native_surface(config, &world));
    print_json(config, &results);
    Ok(())
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn benchmark_world() -> RenderWorld {
    let mut camera = Transform::IDENTITY;
    camera.translation = Vec3::new(0.0, 2.0, 6.0);

    let mut objects = Vec::new();
    for z in 0..6 {
        for x in 0..8 {
            let mut transform = Transform::IDENTITY;
            transform.translation = Vec3::new(x as f32 - 3.5, 0.0, z as f32 - 2.5);
            objects.push(RenderObject {
                object: engine_core::EntityId::from_u128(100 + (z * 8 + x) as u128),
                transform,
                mesh: "debug/cube".to_owned(),
                material: "debug/default".to_owned(),
                casts_shadows: true,
                receive_shadows: true,
                bounds: engine_render::RenderBounds::default(),
                lods: Vec::new(),
            });
        }
    }

    RenderWorld {
        camera: Some(RenderCamera {
            object: engine_core::EntityId::from_u128(1),
            transform: camera,
            projection: engine_render::RenderProjection::Perspective,
            vertical_fov_degrees: 60.0,
            near: 0.1,
            far: 1000.0,
            look_at_target: Some(Vec3::ZERO),
        }),
        objects,
        lights: vec![RenderLight {
            object: engine_core::EntityId::from_u128(2),
            kind: RenderLightKind::Directional,
            color: Vec3::new(1.0, 0.96, 0.88),
            intensity: 3.0,
            range: 32.0,
            spot_angle: 45.0,
            transform: Transform::IDENTITY,
        }],
        ..RenderWorld::default()
    }
}

fn measure_canvas_readback(config: BenchConfig, world: &RenderWorld) -> PathResult {
    let mut renderer = match WgpuRenderDevice::new_offscreen(WgpuOffscreenConfig {
        width: config.width,
        height: config.height,
        format: engine_render::ImageFormat::Rgba8Srgb,
    }) {
        Ok(renderer) => renderer,
        Err(error) => {
            return skipped(
                "canvas-readback",
                config,
                format!("offscreen renderer: {error}"),
            );
        }
    };

    for frame_index in 0..config.warmup {
        if let Err(error) = renderer.render_world_offscreen(world) {
            return skipped("canvas-readback", config, format!("warmup render: {error}"));
        }
        if let Err(error) = renderer.readback_default_target() {
            return skipped(
                "canvas-readback",
                config,
                format!("warmup readback: {error}"),
            );
        }
        let _ = frame_index;
    }

    let mut samples = Vec::with_capacity(config.frames as usize);
    let mut bytes_per_frame = 0;
    for _ in 0..config.frames {
        let started = Instant::now();
        if let Err(error) = renderer.render_world_offscreen(world) {
            return skipped("canvas-readback", config, format!("render: {error}"));
        }
        match renderer.readback_default_target() {
            Ok((_width, _height, pixels)) => {
                bytes_per_frame = pixels.len() as u64;
            }
            Err(error) => return skipped("canvas-readback", config, format!("readback: {error}")),
        }
        samples.push(started.elapsed());
    }

    completed("canvas-readback", config, bytes_per_frame, samples)
}

fn measure_native_surface(config: BenchConfig, world: &RenderWorld) -> PathResult {
    #[cfg(target_os = "linux")]
    if !has_linux_display() {
        return skipped(
            "native-surface",
            config,
            "no Linux display is available".to_owned(),
        );
    }

    let mut builder = winit::event_loop::EventLoop::builder();
    #[cfg(target_os = "linux")]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_any_thread(true);
    }
    #[cfg(target_os = "windows")]
    {
        use winit::platform::windows::EventLoopBuilderExtWindows;
        builder.with_any_thread(true);
    }
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        builder.with_any_thread(true);
    }

    let event_loop = match builder.build() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            return skipped("native-surface", config, format!("event loop: {error}"));
        }
    };
    let window = match event_loop.create_window(
        WindowAttributes::default()
            .with_title("Aster viewport path benchmark")
            .with_visible(false)
            .with_inner_size(PhysicalSize::new(config.width, config.height)),
    ) {
        Ok(window) => window,
        Err(error) => {
            return skipped("native-surface", config, format!("window: {error}"));
        }
    };
    let mut renderer = match WgpuRenderDevice::new_with_performance(
        &window,
        RenderPerformanceConfig::editor_1080p75(),
    ) {
        Ok(renderer) => renderer,
        Err(error) => {
            return skipped(
                "native-surface",
                config,
                format!("surface renderer: {error}"),
            );
        }
    };

    for frame_index in 0..config.warmup {
        if let Err(error) = renderer.submit_render_world(world, RenderFrame { frame_index }) {
            return skipped("native-surface", config, format!("warmup present: {error}"));
        }
    }
    let _ = renderer.wait_idle();

    let mut samples = Vec::with_capacity(config.frames as usize);
    for frame_index in 0..config.frames {
        let started = Instant::now();
        if let Err(error) = renderer.submit_render_world(world, RenderFrame { frame_index }) {
            return skipped("native-surface", config, format!("present: {error}"));
        }
        samples.push(started.elapsed());
    }
    let _ = renderer.wait_idle();

    completed("native-surface", config, 0, samples)
}

#[cfg(target_os = "linux")]
fn has_linux_display() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var_os("WAYLAND_SOCKET").is_some()
        || std::env::var_os("DISPLAY").is_some()
}

fn completed(
    path: &'static str,
    config: BenchConfig,
    bytes_per_frame: u64,
    mut samples: Vec<Duration>,
) -> PathResult {
    samples.sort();
    let total_ms = samples
        .iter()
        .map(|sample| sample.as_secs_f64() * 1000.0)
        .sum::<f64>();
    let average_ms = total_ms / samples.len() as f64;
    let p95_index = ((samples.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
    let p95_ms = samples[p95_index].as_secs_f64() * 1000.0;
    PathResult {
        path,
        status: "ok",
        average_ms: Some(average_ms),
        p95_ms: Some(p95_ms),
        fps: Some(1000.0 / average_ms),
        bytes_per_frame,
        frames: config.frames,
        reason: None,
    }
}

fn skipped(path: &'static str, config: BenchConfig, reason: String) -> PathResult {
    PathResult {
        path,
        status: "skipped",
        average_ms: None,
        p95_ms: None,
        fps: None,
        bytes_per_frame: 0,
        frames: config.frames,
        reason: Some(reason),
    }
}

fn print_json(config: BenchConfig, results: &[PathResult]) {
    println!("{{");
    println!(
        "  \"resolution\": {{ \"width\": {}, \"height\": {} }},",
        config.width, config.height
    );
    println!("  \"frames\": {},", config.frames);
    println!("  \"warmup\": {},", config.warmup);
    println!("  \"paths\": [");
    for (index, result) in results.iter().enumerate() {
        let comma = if index + 1 == results.len() { "" } else { "," };
        print!(
            "    {{ \"path\": \"{}\", \"status\": \"{}\"",
            result.path, result.status
        );
        if let Some(average_ms) = result.average_ms {
            print!(", \"average_ms\": {average_ms:.4}");
        }
        if let Some(p95_ms) = result.p95_ms {
            print!(", \"p95_ms\": {p95_ms:.4}");
        }
        if let Some(fps) = result.fps {
            print!(", \"fps\": {fps:.2}");
        }
        print!(
            ", \"bytes_per_frame\": {}, \"frames\": {}",
            result.bytes_per_frame, result.frames
        );
        if let Some(reason) = result.reason.as_ref() {
            print!(", \"reason\": \"{}\"", json_escape(reason));
        }
        println!(" }}{comma}");
    }
    println!("  ]");
    println!("}}");
}

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
