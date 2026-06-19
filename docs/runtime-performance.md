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
