# Aster Script Family Specification

## Status

Draft. This document records the current direction for Aster's AI-first scripting and asset declaration languages.

## Motivation

Aster should not expose one universal script language for every kind of game authoring task. An AI creating a game needs to declare scenes, models, audio, lighting, physics, behavior, and runtime logic. These concerns have different safety, validation, reuse, and execution requirements.

The scripting system is therefore split into a small language family:

- declarative languages for assets, scenes, and behavior;
- one graph/runtime-capable language for logic that actually needs computation.

The main design constraint is that AI should modify structured declarative files whenever possible, and only use a Turing-complete script when the task requires dynamic logic.

## Core language tiers

| Tier | Language | Extension | Purpose | Turing complete |
| --- | --- | --- | --- | --- |
| Model declaration | Aster Model | `.amdl` | Reusable model, mesh, material, collider, mass, LOD, and asset composition declarations. | No |
| Scene declaration | Aster Scene | `.ascn` | Scene graph, entities, transforms, light, audio, physics environment, initial state, and references to models, prefabs, behaviors, and scripts. | No |
| Behavior declaration | Aster Behavior | `.abv` | Behavior trees, state machines, selectors, sequences, conditions, and high-level AI actions. | No by default |
| Runtime logic | Aster Script | `.as` | Event handlers, gameplay rules, dynamic systems, procedural logic, quest rules, UI logic, and other runtime computation. | Yes |

The MVP language set is:

```text
.amdl  Aster Model
.ascn  Aster Scene
.abv   Aster Behavior
.as    Aster Script
```

## Reserved future extensions

These extensions are reserved but should not be implemented until the corresponding domain becomes complex enough to justify a separate language.

| Language | Extension | Purpose |
| --- | --- | --- |
| Aster Prefab | `.apfb` | Reusable entity or entity-group declarations. |
| Aster Material | `.amat` | Standalone material and shader-parameter declarations. |
| Aster Audio | `.aud` | Audio events, emitters, mixers, buses, ambience, and spatial audio declarations. |
| Aster World | `.awld` | Multi-scene world layout, level streaming, global rules, and campaign structure. |

## File role boundaries

### `.amdl` — Aster Model

Use `.amdl` for reusable asset composition. A model file describes what an object is, not where it is placed in a scene.

Allowed responsibilities:

- mesh or primitive references;
- material references or inline simple material parameters;
- collider presets;
- mass and physical material defaults;
- attachment points;
- LOD declarations;
- animation set references.

Disallowed responsibilities:

- scene placement;
- gameplay event handlers;
- per-level state;
- arbitrary loops or runtime control flow.

Example:

```astermodel
model Crate {
  mesh = primitive.box(size: [1, 1, 1])

  material = material {
    base_color = "#8a5a2b"
    roughness = 0.75
    metallic = 0.0
  }

  collider = collider.box {
    size = [1, 1, 1]
  }

  rigidbody = dynamic {
    mass = 12kg
  }
}
```

### `.ascn` — Aster Scene

Use `.ascn` for world composition. A scene file declares entities, transforms, light, audio, physics environment, and references to reusable assets.

Allowed responsibilities:

- entities and hierarchy;
- transforms;
- model, prefab, behavior, and script references;
- lights;
- audio sources and listeners;
- physics world parameters;
- initial component values.

Disallowed responsibilities:

- procedural generation loops;
- complex gameplay logic;
- long-running behavior logic;
- reusable model definitions that should live in `.amdl`.

Example:

```asterscene
scene ForestArena {
  gravity = [0, -9.81, 0]

  light Sun {
    kind = directional
    direction = [-0.4, -1.0, -0.2]
    intensity = 4.0
  }

  audio AmbientForest {
    clip = "audio/forest_loop.ogg"
    loop = true
    volume = 0.4
  }

  entity Crate01 {
    model = "models/crate.amdl"
    transform = position([3, 0.5, 2])
    rigidbody = dynamic
  }
}
```

### `.abv` — Aster Behavior

Use `.abv` for structured AI and entity behavior. This is the evolution path for the existing declarative behavior-tree work.

Allowed responsibilities:

- selectors, sequences, parallels, decorators, and conditions;
- state-machine transitions;
- high-level engine actions;
- references to reusable presets;
- bounded parameters for patrol, chase, flee, attack, interact, and similar behaviors.

Disallowed responsibilities:

- arbitrary computation as the default path;
- asset declarations;
- scene layout;
- low-level frame-by-frame movement code when a high-level action exists.

Example:

```asterbehavior
behavior EnemyGuard {
  selector {
    sequence {
      condition player.distance < 12
      action chase(player)
      action attack_if_in_range(player)
    }

    action patrol(points: ["A", "B", "C"])
  }
}
```

### `.as` — Aster Script

Use `.as` only for runtime logic that requires Turing-complete computation.

Allowed responsibilities:

- event handlers;
- gameplay rules;
- custom calculations;
- procedural generation;
- quest and mission rules;
- UI logic;
- integration glue that cannot be represented declaratively.

Disallowed responsibilities:

- bulk scene declaration;
- static model declaration;
- behavior that can be safely represented in `.abv`;
- unbounded authority over engine systems without an explicit capability grant.

Example:

```asterscript
on update(dt) {
  if input.key_down("Space") && player.is_grounded {
    player.velocity.y = 6.5
  }
}
```

## Reference rules

Files should reference lower-level or runtime files by path or stable asset ID:

```text
.ascn -> .amdl
.ascn -> .abv
.ascn -> .as
.abv  -> .as only for explicit custom action hooks
```

Model files should not reference scenes. Scene files may reference models, behaviors, scripts, and future prefabs. Behavior files may call named engine actions and may reference `.as` hooks only when a declarative action is insufficient.

## Compilation model

Each language should compile into a stable intermediate representation before runtime use.

Expected pipeline:

```text
.amdl  -> Asset/model IR -> engine-assets / engine-render / engine-physics
.ascn  -> Scene IR       -> engine-ecs scene file / runtime world
.abv   -> Behavior IR    -> engine-script-declarative behavior tree
.as    -> Runtime IR     -> Rhai today, possible AsterScript VM later
```

JSON remains useful as an interchange or generated artifact format, but it should not be the primary authoring format for AI-facing game creation.

## Compatibility with current implementation

Current repository state already contains:

- `engine-script-declarative` for behavior-tree style scripting;
- JSON behavior examples under `examples/behaviors/`;
- `engine-script-rhai` for runtime-capable scripting;
- JSON scene and prefab examples under `examples/project/`.

This specification does not require deleting those formats. The migration path is:

1. keep existing JSON scene, prefab, and behavior files as supported interchange formats;
2. add parsers for `.amdl`, `.ascn`, and `.abv` that compile to the existing Rust data structures;
3. keep Rhai as the first implementation backend for `.as` until a dedicated Aster Script runtime is justified.

## Design rules

- Prefer declarative files for AI-generated assets, scenes, and behavior.
- Keep `.amdl`, `.ascn`, and `.abv` non-Turing-complete by default.
- Keep `.as` Turing-complete, sandboxed, capability-gated, and visibly distinct from declarative files.
- Do not use `.ast` as the primary extension; it is too generic and does not preserve the language tier.
- Avoid long extensions such as `.astermodel` for normal project authoring; short suffixes are the canonical form.
- Preserve clear ownership: a file should describe one kind of thing.
- Make every declarative language statically checkable before runtime.
- Design syntax for AI generation first: regular structure, explicit names, few hidden defaults, and stable diagnostics.
