# Aster

Aster is an early-stage Rust game engine workspace focused on a small native
runtime, explicit engine subsystems, and editor-ready data formats.

## Workspace

- `engine-core`: shared IDs, handles, errors, logging, math, time, and runtime
  configuration.
- `engine-ecs`: scene, entity, transform, and schema primitives.
- `engine-platform`: platform boundaries for windows, input, filesystem,
  dynamic libraries, and callbacks.
- `engine-assets`: asset database, resource registry, manifests, dependency
  graph, import queues, hot reload tracking, and resource data formats.
- `engine-render`: renderer-facing abstractions and the headless render device.
- `runtime-min`: the minimal runtime profile used to keep core builds small.
- `xtask`: repository automation entry points.

## Build Profiles

Runtime composition is driven through Cargo features:

- `runtime-min`: minimal native runtime without editor, scripting, heavy
  importers, physics, audio, or concrete rendering.
- `runtime-game`: game runtime surface on top of the minimal profile.
- `editor`: editor-facing workflows and data.
- `agent-tools`: automation and agent integration surface.
- `script-python`: Python scripting backend integration.
- `dev-full`: full local development profile.

Heavy asset importers are feature-gated in `engine-assets` with
`fbx-importer`, `assimp-importer`, and `heavy-importers`, so disabling them
keeps their dependencies out of minimal runtime builds.

## Development

Run the full workspace tests:

```sh
cargo test --workspace
```

Check the minimal runtime feature path:

```sh
cargo test -p runtime-min --no-default-features --features runtime-min
```

## License

Aster is licensed under the Mozilla Public License 2.0. See `LICENSE`.
