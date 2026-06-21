//! Physics stress benchmark for bad-case MVP scenes.
//!
//! Run with:
//! `cargo run -p engine-physics --features rapier --release --example stress_benchmark`

use std::time::Instant;

use engine_physics::{
    BodyKind, CcdMode, ColliderDesc, ColliderShape, PhysicsBackend, QueryFilter,
    RapierPhysicsBackend, RigidbodyDesc, Transform, Vec3,
};

#[derive(Clone, Copy, Debug)]
struct BenchConfig {
    frames: u64,
    static_grid: u32,
    dynamic_bodies: u32,
    trigger_grid: u32,
    query_count: u32,
    dt: f32,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            frames: 240,
            static_grid: 32,
            dynamic_bodies: 768,
            trigger_grid: 16,
            query_count: 512,
            dt: 1.0 / 60.0,
        }
    }
}

impl BenchConfig {
    fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            frames: env_u64("ASTER_PHYSICS_BENCH_FRAMES", defaults.frames),
            static_grid: env_u32("ASTER_PHYSICS_STATIC_GRID", defaults.static_grid),
            dynamic_bodies: env_u32("ASTER_PHYSICS_DYNAMIC_BODIES", defaults.dynamic_bodies),
            trigger_grid: env_u32("ASTER_PHYSICS_TRIGGER_GRID", defaults.trigger_grid),
            query_count: env_u32("ASTER_PHYSICS_QUERY_COUNT", defaults.query_count),
            dt: env_f32("ASTER_PHYSICS_DT", defaults.dt),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct QueryTotals {
    ray_hits: u64,
    overlap_hits: u64,
    sweep_hits: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = BenchConfig::from_env();
    let mut backend = RapierPhysicsBackend::new();
    build_scene(&mut backend, config)?;

    for frame in 0..16 {
        run_queries(&backend, config.query_count, frame);
        backend.fixed_update(config.dt);
        let _ = backend.drain_contacts();
    }

    let started = Instant::now();
    let mut step_us = Vec::with_capacity(config.frames as usize);
    let mut contacts = 0u64;
    let mut query_totals = QueryTotals::default();
    for frame in 0..config.frames {
        query_totals.accumulate(run_queries(&backend, config.query_count, frame));
        backend.fixed_update(config.dt);
        contacts = contacts.saturating_add(backend.drain_contacts().len() as u64);
        step_us.push(backend.stats().step_us);
    }

    step_us.sort_unstable();
    let elapsed = started.elapsed();
    let stats = backend.stats();
    let average_step_us = step_us.iter().sum::<u64>() as f64 / step_us.len().max(1) as f64;
    let average_frame_ms = elapsed.as_secs_f64() * 1000.0 / config.frames as f64;
    let physics_budget_ms = config.dt as f64 * 1000.0;
    let p50 = percentile(&step_us, 0.50);
    let p95 = percentile(&step_us, 0.95);
    let p99 = percentile(&step_us, 0.99);
    let max = *step_us.last().unwrap_or(&0);

    println!(
        "config frames={} static_colliders={} dynamic_bodies={} triggers={} queries_per_frame={} dt_ms={:.3}",
        config.frames,
        config.static_grid.saturating_mul(config.static_grid),
        config.dynamic_bodies,
        config.trigger_grid.saturating_mul(config.trigger_grid),
        config.query_count,
        config.dt * 1000.0
    );
    println!(
        "step_us avg={average_step_us:.1} p50={p50} p95={p95} p99={p99} max={max} frame_wall_ms={average_frame_ms:.3} budget_ms={physics_budget_ms:.3}"
    );
    println!(
        "stats bodies={} colliders={} contacts={} sleeping={} joints={} vehicles={} drained_events={}",
        stats.body_count,
        stats.collider_count,
        stats.contact_count,
        stats.sleeping_count,
        stats.joint_count,
        stats.vehicle_count,
        contacts
    );
    println!(
        "queries ray_hits={} overlap_hits={} sweep_hits={}",
        query_totals.ray_hits, query_totals.overlap_hits, query_totals.sweep_hits
    );

    if p95 as f64 > physics_budget_ms * 1000.0 {
        eprintln!("warning: p95 physics step exceeds the fixed-step budget on this machine");
    }

    Ok(())
}

fn build_scene(
    backend: &mut RapierPhysicsBackend,
    config: BenchConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let ground = backend.create_body(&RigidbodyDesc {
        transform: transform(Vec3::new(0.0, -0.55, 0.0)),
        kind: BodyKind::Static,
        ..RigidbodyDesc::default()
    })?;
    backend.add_collider(
        ground,
        &ColliderDesc {
            shape: ColliderShape::Box {
                half_extents: Vec3::new(120.0, 0.5, 120.0),
            },
            ..ColliderDesc::default()
        },
    )?;

    let grid_radius = config.static_grid as f32 * 0.5;
    for x in 0..config.static_grid {
        for z in 0..config.static_grid {
            let px = (x as f32 - grid_radius) * 2.5;
            let pz = (z as f32 - grid_radius) * 2.5;
            let body = backend.create_body(&RigidbodyDesc {
                transform: transform(Vec3::new(px, 0.25, pz)),
                kind: BodyKind::Static,
                ..RigidbodyDesc::default()
            })?;
            backend.add_collider(
                body,
                &ColliderDesc {
                    shape: ColliderShape::Box {
                        half_extents: Vec3::new(0.35, 0.35, 0.35),
                    },
                    friction: 0.8,
                    ..ColliderDesc::default()
                },
            )?;
        }
    }

    let trigger_radius = config.trigger_grid as f32 * 0.5;
    for x in 0..config.trigger_grid {
        for z in 0..config.trigger_grid {
            let px = (x as f32 - trigger_radius) * 5.0 + 1.25;
            let pz = (z as f32 - trigger_radius) * 5.0 + 1.25;
            let body = backend.create_body(&RigidbodyDesc {
                transform: transform(Vec3::new(px, 1.5, pz)),
                kind: BodyKind::Static,
                ..RigidbodyDesc::default()
            })?;
            backend.add_collider(
                body,
                &ColliderDesc {
                    shape: ColliderShape::Sphere { radius: 1.25 },
                    is_trigger: true,
                    ..ColliderDesc::default()
                },
            )?;
        }
    }

    let columns = (config.dynamic_bodies as f32).sqrt().ceil() as u32;
    for index in 0..config.dynamic_bodies {
        let x = index % columns;
        let z = index / columns;
        let layer = z / columns.max(1);
        let px = (x as f32 - columns as f32 * 0.5) * 1.15;
        let pz = (z as f32 - columns as f32 * 0.5) * 1.15;
        let py = 4.0 + layer as f32 * 1.05 + ((index % 7) as f32 * 0.03);
        let ccd = if index % 10 == 0 {
            CcdMode::Enabled
        } else {
            CcdMode::Disabled
        };
        let body = backend.create_body(&RigidbodyDesc {
            transform: transform(Vec3::new(px, py, pz)),
            kind: BodyKind::Dynamic,
            linear_damping: 0.02,
            angular_damping: 0.02,
            ccd,
            ..RigidbodyDesc::default()
        })?;
        let shape = if index % 3 == 0 {
            ColliderShape::Sphere { radius: 0.4 }
        } else {
            ColliderShape::Box {
                half_extents: Vec3::new(0.35, 0.35, 0.35),
            }
        };
        backend.add_collider(
            body,
            &ColliderDesc {
                shape,
                friction: 0.7,
                restitution: 0.05,
                active_contact_events: index % 32 == 0,
                ..ColliderDesc::default()
            },
        )?;
    }

    Ok(())
}

fn run_queries(backend: &RapierPhysicsBackend, query_count: u32, frame: u64) -> QueryTotals {
    let mut totals = QueryTotals::default();
    for query in 0..query_count {
        let seed = frame
            .wrapping_mul(1_103_515_245)
            .wrapping_add(query as u64 * 12_345);
        let x = pseudo_range(seed, -45.0, 45.0);
        let z = pseudo_range(seed.rotate_left(17), -45.0, 45.0);
        let origin = Vec3::new(x, 40.0, z);
        if backend
            .raycast(
                origin,
                Vec3::new(0.1, -1.0, 0.05),
                90.0,
                QueryFilter::default(),
            )
            .is_some()
        {
            totals.ray_hits = totals.ray_hits.saturating_add(1);
        }
        totals.overlap_hits = totals.overlap_hits.saturating_add(
            backend
                .overlap_sphere(Vec3::new(x, 1.2, z), 1.4, QueryFilter::default())
                .len() as u64,
        );
        if backend
            .sweep_sphere(
                Vec3::new(x, 2.0, z),
                0.25,
                Vec3::new(1.0, 0.0, 0.35),
                4.0,
                QueryFilter::default(),
            )
            .is_some()
        {
            totals.sweep_hits = totals.sweep_hits.saturating_add(1);
        }
    }
    totals
}

impl QueryTotals {
    fn accumulate(&mut self, other: Self) {
        self.ray_hits = self.ray_hits.saturating_add(other.ray_hits);
        self.overlap_hits = self.overlap_hits.saturating_add(other.overlap_hits);
        self.sweep_hits = self.sweep_hits.saturating_add(other.sweep_hits);
    }
}

fn transform(translation: Vec3) -> Transform {
    Transform {
        translation,
        ..Transform::IDENTITY
    }
}

fn percentile(sorted: &[u64], percentile: f32) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) as f32 * percentile).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn pseudo_range(seed: u64, min: f32, max: f32) -> f32 {
    let value = splitmix64(seed) as f64 / u64::MAX as f64;
    min + (max - min) * value as f32
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut result = value;
    result = (result ^ (result >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    result = (result ^ (result >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    result ^ (result >> 31)
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_f32(name: &str, default: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}
