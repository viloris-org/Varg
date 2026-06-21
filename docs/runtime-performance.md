# Runtime Performance Contract

Aster's native runtime is designed to output up to 3840×2160 at high refresh
rates when the selected adapter, display, scene, and quality budget permit it.
This is an output capability, not an unconditional frame-rate guarantee.

## Competitive 120 Hz policy

The native runtime defaults to:

- Native surface output with no CPU readback.
- Mailbox presentation when supported, otherwise immediate, then FIFO.
- One queued surface frame for reduced latency.
- 120 FPS dynamic-resolution budget.
- Native output resolution separated from internal HDR, SSAO, and bloom
  resolution.

Environment overrides:

```text
ASTER_OUTPUT_WIDTH=3840
ASTER_OUTPUT_HEIGHT=2160
ASTER_PRESENT_MODE=low-latency|uncapped|vsync
ASTER_TARGET_FPS=120
ASTER_RENDER_SCALE=1.0
ASTER_DYNAMIC_RESOLUTION=true|false
```

Dynamic resolution preserves the native output dimensions while scaling the
internal render targets between 50% and 100% linearly.

## Verification

Build and run the synchronized 4K GPU benchmark:

```bash
cargo run -p engine-render-wgpu --release --example benchmark_4k120
```

The benchmark waits for submitted GPU work to complete. Its result is specific
to the reported adapter and the benchmark scene. Software adapters such as
llvmpipe are valid correctness checks but not hardware performance results.

Runtime telemetry exposes:

- Native output dimensions.
- Internal rendering dimensions.
- Active render scale.
- CPU render preparation/submission time.
- Adapter 4K texture capability.
- GPU timestamp-query capability.

Actual pass-level GPU timing remains a required follow-up before performance
budgets can be enforced per shadow, forward, SSAO, bloom, and post-processing
pass.

## Physics stress policy

The MVP physics target is not "full open world simulation everywhere". It is a
bounded bad-case scene that keeps the active physics set healthy when gameplay,
queries, triggers, and collision events spike around the player.

Run the Rapier stress benchmark:

```bash
cargo run -p engine-physics --features rapier --release --example stress_benchmark
```

The default scenario creates:

- 1 large ground collider.
- 32×32 static obstacle colliders.
- 768 dynamic rigid bodies, with CCD enabled on every tenth body.
- 16×16 trigger volumes.
- 512 raycast/overlap/sweep queries per fixed step.
- 240 measured fixed steps after warmup.

Environment overrides:

```text
ASTER_PHYSICS_BENCH_FRAMES=240
ASTER_PHYSICS_STATIC_GRID=32
ASTER_PHYSICS_DYNAMIC_BODIES=768
ASTER_PHYSICS_TRIGGER_GRID=16
ASTER_PHYSICS_QUERY_COUNT=512
ASTER_PHYSICS_DT=0.016666667
```

Interpretation:

- `step_us p95` should stay below the fixed-step budget on the target machine.
- `max` is useful for spotting pathological stalls but should not be treated as
  a deterministic CI threshold.
- `frame_wall_ms` includes benchmark query work around the physics step.
- `stats sleeping` should rise once the pile settles; if it does not, inspect
  damping, rest thresholds, and constantly-woken contact/event paths.

For open-world work, scale active bodies by streaming radius rather than total
map size. Static terrain and far-away objects should be chunked, unloaded,
represented by coarse colliders, or left outside the active physics world.
