# Varg Language Family Specification

## Status

Draft. This document defines the target language direction for the Varg engine scripting and authoring rewrite.

This rewrite is intentionally incompatible with the current Aster script stack. Existing `.aster`, Rhai, Python, and JSON behavior assets are treated as legacy implementation artifacts, not compatibility constraints.

## Goals

Varg should expose one coherent authoring family instead of several unrelated languages. Human authors and AI agents should be able to read, patch, validate, and generate project files without learning a different syntax for every asset type.

The language family uses:

- Swift-like syntax for readability, modern feel, and typed declarations.
- three.js-like scene concepts for familiar 3D object composition.
- ECS as an internal runtime representation, not a user-facing scripting model.
- declarative files for scenes, prefabs, materials, models, and behaviors.
- imperative scripts only for dynamic gameplay logic.

## Non-Goals

Varg does not aim to implement Swift. Swift is syntax inspiration, not a compatibility target.

Varg does not expose JavaScript or three.js as the scripting language. The scene model can feel familiar to three.js users, but the file syntax remains Varg.

Varg does not preserve the old Aster script API, Rhai file format, Python subprocess script model, or JSON behavior format.

Varg scene files are not general-purpose programs. They should be parseable, diffable, visually editable, and deterministic.

## Design Principles

### One Surface Syntax

All user-authored Varg files use the same broad shape:

```varg
kind Name {
    property: value

    child {
        property: value
    }
}
```

The file role decides what top-level declarations are allowed. It should not introduce a new language style.

### Role-Specific Files

Varg keeps the public file set small. The goal is not one extension per engine concept; the goal is one extension per authoring mode.

| Extension | Role | Purpose | Turing Complete |
| --- | --- | --- | --- |
| `.varg` | Logic file | Scripts, reusable modules, dynamic gameplay logic, and declarative behaviors | Yes for `script` and `module`; no for `behavior` blocks |
| `.vscene` | World file | Scenes, prefabs, entity composition, layout intent, and network replication declarations | No |
| `.vasset` | Asset file | Models, materials, audio events, shader/material parameters, primitive resource recipes | No |

This gives authors and AI agents three mental buckets:

- **Logic:** what changes at runtime.
- **World:** what exists in a scene or reusable prefab.
- **Assets:** what resources are made of.

### AI Writes Intent First

AI agents should usually edit declarative files, not imperative scripts. Scene authoring should support high-level intent such as scattering trees, placing landmarks, or defining spawn zones without forcing the AI to emit hundreds of concrete entities.

Tools may compile intent into concrete scene graph data, but the source file should preserve the authoring intent where possible.

### Runtime Scripting Is the Escape Hatch

Use `.varg` only when the behavior needs computation, time, state, event handling, reusable helper code, or a behavior graph declaration. Static object composition belongs in `.vscene`. Static resource composition belongs in `.vasset`.

## Logic File: `.varg`

Audience: human-first, AI-assisted.

`.varg` is a Swift-like gameplay logic language with a safe engine API. The first implementation may interpret or transpile to an existing backend, but the public language is Varg.

`.varg` allows three top-level declarations:

| Declaration | Purpose |
| --- | --- |
| `script` | Entity-attached runtime logic with lifecycle hooks, exported properties, and state |
| `module` | Reusable code imported by other `.varg` files |
| `behavior` | Declarative behavior tree or state-machine logic compiled to behavior IR |

`script` and `module` may execute logic. `behavior` stays declarative even though it lives in a `.varg` file.

### Example

```varg
script PlayerController {
    @export var speed: Float = 6.0
    @export var jumpForce: Float = 8.0

    var jumpsLeft: Int = 1

    func start() {
        log("player ready")
    }

    func update(_ dt: Float) {
        let move = Input.axis("move")
        entity.translate(Vec3(move.x, 0, move.y) * speed * dt)

        if Input.pressed("jump") && entity.isGrounded {
            entity.velocity.y = jumpForce
            jumpsLeft -= 1
        }
    }

    func collisionEnter(_ other: Entity) {
        if other.tag == "coin" {
            emit("coin_collected", ["value": 1])
            other.destroy()
        }
    }
}
```

### Module Example

```varg
module Combat {
    let criticalMultiplier: Float = 1.5

    func damage(_ target: Entity, amount: Int) {
        if target.has(Health) {
            target.health.damage(amount)
        }
    }
}
```

Another `.varg` file can import it:

```varg
import "scripts/combat.varg"

script EnemyAttack {
    func collisionEnter(_ other: Entity) {
        if other.tag == "player" {
            Combat.damage(other, amount: 10)
        }
    }
}
```

### Behavior Example

```varg
behavior EnemyAI {
    selector {
        sequence {
            when player.distance < 10
            action chase(player)
            action attack(player)
        }

        sequence {
            action patrol(points: ["A", "B", "C"])
        }
    }
}
```

Behavior declarations compile to behavior IR. They do not support arbitrary loops, function definitions, or mutable script state.

### Core Syntax

The MVP language supports:

- `script Name { ... }`
- `module Name { ... }`
- `behavior Name { ... }`
- `import "path/to/module.varg"`
- `let` immutable locals
- `var` mutable locals and script state
- `@export var` editor-exposed properties
- `func name(...) { ... }`
- `if`, `else`, `for in`, `while`
- `return`, `break`, `continue`
- typed parameters and properties
- optional values with `Type?`
- `if let` and `guard let`
- method calls and property access
- arrays and dictionaries

The MVP should not support:

- `class`, `struct`, `enum`, `protocol`
- generics
- extensions
- operator overloading
- access control modifiers
- async/await
- macros
- arbitrary file, network, or process access

### Lifecycle Hooks

Lifecycle hooks are ordinary functions with reserved names:

```varg
func start()
func update(_ dt: Float)
func fixedUpdate(_ dt: Float)
func collisionEnter(_ other: Entity)
func collisionExit(_ other: Entity)
func event(_ name: String, _ data: EventData)
```

Missing hooks are valid. Invalid hook signatures are diagnostics.

### Built-In Bindings

Each script receives a small set of built-in bindings:

| Binding | Purpose |
| --- | --- |
| `entity` | The entity this script is attached to |
| `scene` | Query and spawn access to the current scene |
| `Input` | Action, axis, and pointer input |
| `Time` | Frame timing and timers |
| `Audio` | Safe audio commands |
| `Assets` | Safe asset references |

Scripts should not directly mutate engine internals. They use the safe API exposed by these bindings.

### Imports and Resource References

Use `import` only for Varg code modules:

```varg
import "scripts/combat.varg"
```

Use typed resource constructors for scene and asset references:

```varg
let dungeon = Scene("scenes/dungeon.vscene")
let crate = Asset("assets/props/crate.vasset")
let footstep = Asset("assets/audio/footsteps.vasset")

Game.loadScene(dungeon)
Audio.play(footstep, event: "dirt", at: entity.position)
scene.spawn(crate, at: entity.position)
```

Scripts must not import `.vscene` or `.vasset` as code.

### Component Access

The public API should be game-author friendly, not raw ECS.

Preferred:

```varg
if entity.has(Health) {
    entity.health.damage(10)
}

for enemy in scene.query(tag: "enemy", with: Health) {
    enemy.health.damage(5)
}
```

Avoid making users write Rust-like ECS queries in gameplay scripts.

## World File: `.vscene`

Audience: AI-first, human-readable.

`.vscene` describes scenes, prefabs, entity composition, layout intent, spawn points, and network replication declarations.

Scene files borrow three.js concepts: scene, camera, light, mesh, material, transform, and object hierarchy. They do not borrow JavaScript syntax.

`.vscene` allows three top-level declarations:

| Declaration | Purpose |
| --- | --- |
| `scene` | A loadable level or world segment |
| `prefab` | A reusable entity or entity group |
| `network` | Replication, ownership, RPC, and transport declarations |

### Concrete Scene Example

```varg
scene MainScene {
    camera "MainCamera" {
        transform {
            position: Vec3(0, 6, 10)
            rotation: Euler(-30, 0, 0)
        }

        perspective {
            fov: 60
            near: 0.1
            far: 1000
        }

        primary: true
    }

    light "Sun" {
        kind: directional
        intensity: 3.0
        rotation: Euler(-45, 35, 0)
    }

    entity "Player" {
        tag: "player"

        transform {
            position: Vec3(0, 1, 0)
        }

        model: Asset("assets/models/player.vasset")

        collider {
            shape: capsule(height: 1.8, radius: 0.35)
        }

        rigidbody {
            mode: kinematic
        }

        script PlayerController {
            source: "scripts/player_controller.varg"
            speed: 6.0
            jumpForce: 8.0
        }
    }

    entity "Ground" {
        mesh: Box(size: Vec3(20, 1, 20))

        material {
            baseColor: Color("#4f7f4a")
            roughness: 0.8
        }

        collider {
            shape: box
        }
    }
}
```

### Intent Scene Example

AI agents should be allowed to express scene intent without enumerating every object.

```varg
scene ForestCamp {
    intent: "A small night camp in a forest clearing"

    layout {
        ground: circular(radius: 18)
        mood: night
        density: medium
    }

    place "Campfire" at center {
        prefab: Prefab("world/campfire.vscene#Campfire")
        light: warm(radius: 8, intensity: 2.5)
        audio: "fire_crackle"
    }

    scatter "PineTree" {
        prefab: Prefab("world/forest_props.vscene#PineTree")
        count: 32
        area: ring(inner: 8, outer: 18)
        scale: range(0.8, 1.4)
        avoid: ["Campfire", "PlayerSpawn"]
    }

    spawn "PlayerSpawn" {
        position: Vec3(0, 0, -5)
    }
}
```

Scene intent compiles into deterministic scene graph objects. The compiler owns placement algorithms, random seeds, validation, and conflict resolution.

### Scene Restrictions

World files must not contain arbitrary loops, function definitions, conditionals, or runtime event handlers.

Allowed repetition is declarative:

```varg
scatter "Tree" {
    count: 100
    area: rect(width: 50, depth: 50)
}
```

Forbidden:

```varg
for i in 0..<100 {
    spawnTree(i)
}
```

### Prefab Example

```varg
prefab Campfire {
    entity "Campfire" {
        model: Asset("assets/models/campfire.vasset")

        light "FireLight" {
            kind: point
            color: Color("#ff9f45")
            intensity: 2.5
            radius: 8
        }

        audio {
            source: Asset("assets/audio/campfire.vasset")
            event: "loop"
            playOnStart: true
        }
    }
}
```

### Network Example

Network declarations live with world composition because replication is about entities, ownership, fields, and RPC contracts. Scripts may call network RPCs, but they do not open raw sockets.

```varg
network GameNet {
    mode: clientServer
    tickRate: 30

    transport {
        kind: quic
    }

    authority {
        default: server
        playerInput: clientPredicted
    }

    replicate Player {
        source: Prefab("world/player.vscene#Player")
        owner: connection.player

        fields {
            position: Vec3 {
                mode: predicted
                interpolation: 100ms
                threshold: 0.02
            }

            health: Int {
                mode: server
                reliable: true
            }
        }

        rpc clientToServer fireWeapon(_ origin: Vec3, _ direction: Vec3)
        rpc serverToClients playHitEffect(_ position: Vec3)
    }
}
```

Network implementation remains an engine module. `.vscene` only defines the replication contract.

## Asset File: `.vasset`

Audience: AI-first, human-readable.

`.vasset` groups declarative resource recipes. It can contain model, material, and audio declarations in one file when that improves locality.

`.vasset` allows these top-level declarations:

| Declaration | Purpose |
| --- | --- |
| `model` | Mesh, primitive, collider, LOD, animation set, and attachment composition |
| `material` | Shader and material parameters |
| `audio` | Audio clips, events, layers, randomization, buses, spatial settings |

### Model Example

```varg
model Crate {
    mesh {
        kind: primitive.box
        size: Vec3(1, 1, 1)
    }

    material: Material("assets/props/crate.vasset#WoodCrate")

    collider {
        shape: box
        size: Vec3(1, 1, 1)
    }

    rigidbodyDefaults {
        mode: dynamic
        mass: 12
    }
}
```

### Material Example

```varg
material MossyRock {
    shader: "pbr"

    baseColor: Color("#6f7d58")
    roughness: 0.92
    metallic: 0.0

    texture albedo: "textures/mossy_rock_albedo.png"
    texture normal: "textures/mossy_rock_normal.png"
}
```

### Audio Example

```varg
audio FootstepDirt {
    event "play" {
        clips: [
            "audio/footstep_dirt_01.ogg",
            "audio/footstep_dirt_02.ogg",
            "audio/footstep_dirt_03.ogg"
        ]

        random {
            pitch: range(0.94, 1.06)
            volume: range(0.8, 1.0)
        }

        spatial {
            enabled: true
            radius: 12
            attenuation: inverse
        }

        bus: "SFX"
        cooldown: 0.08
    }
}
```

`.vasset` must not define entity placement, lifecycle hooks, or runtime event handlers.

## Naming and Style

Varg source uses:

- four-space indentation
- `camelCase` properties and functions
- `PascalCase` script, module, behavior, scene, prefab, model, material, and audio names
- double-quoted strings
- explicit type annotations on exported properties and function parameters
- trailing commas only where the grammar explicitly allows lists

Prefer:

```varg
@export var moveSpeed: Float = 5.0
func fixedUpdate(_ dt: Float)
```

Avoid:

```varg
@export var move_speed = 5
func fixed_update(dt)
```

## Diagnostics

Diagnostics should use stable codes. Suggested prefixes:

| Prefix | Area |
| --- | --- |
| `VARG1xxx` | Parse errors |
| `VARG2xxx` | Type errors |
| `VARG3xxx` | Script lifecycle errors |
| `VARG4xxx` | World, scene, prefab, and network validation |
| `VARG5xxx` | Asset reference errors |
| `VARG6xxx` | Asset declaration and behavior validation |

Diagnostics must include:

- file path
- line and column when available
- message
- expected shape
- suggested fix
- whether the issue blocks compilation

Example:

```text
VARG3001 error at scripts/player_controller.varg:8:10
update hook has 0 parameters; expected `func update(_ dt: Float)`.
```

## Compiler Targets

Source files should compile into explicit internal representations:

| Source | Target |
| --- | --- |
| `.varg` | Script AST or bytecode |
| `.vscene` | Scene graph IR plus placement plan |
| `.vasset` | Asset IR for models, materials, and audio |

The runtime should consume IR, not re-interpret high-level authoring files everywhere.

## Runtime Architecture Direction

The script runtime should expose one user-facing script component:

```varg
script PlayerController {
    source: "scripts/player_controller.varg"
    speed: 6.0
}
```

The serialized engine component should not require users to choose `rhai`, `python`, or another backend. If a backend exists, it is an implementation detail behind the Varg script module.

Recommended internal shape:

```rust
pub struct ScriptComponent {
    pub source: AssetRef,
    pub exported_values: Map<String, Value>,
    pub state: Map<String, Value>,
}
```

The old dual model of object-level script lists plus script components should be removed. Script attachment should have one source of truth.

## Rewrite Plan

### Phase 1: Specification and Fixtures

- Add this language specification.
- Add parser fixtures for `.varg`, `.vscene`, and `.vasset`.
- Add golden diagnostics for invalid lifecycle hooks, invalid scene loops, and missing asset references.

### Phase 2: Data Model Cleanup

- Keep `ScriptComponent { source, exported_values, state }` as the Varg-first script component.
- Remove object-level `scripts` as a public scene model.
- Keep Python out of the runtime path.
- Hide Rhai or any other execution backend behind a Varg runtime adapter.

### Phase 3: Parser and Validator

- Implement a real parser for the MVP grammar.
- Add AST types for script and declarative files.
- Validate exported properties, lifecycle hooks, scene declarations, and asset references.
- Emit stable diagnostics.

### Phase 4: Execution

- Start with interpretation or transpilation if that gets the editor usable quickly.
- Keep the public API Varg-native even if the first backend uses an existing interpreter.
- Compile declarative files to IR before runtime consumption.

### Phase 5: Editor Integration

- New script files default to `.varg`.
- New scenes default to `.vscene`.
- New asset declaration files default to `.vasset`.
- The editor displays exported script variables from the parsed script AST.
- AI tools operate on role-specific files, not raw scene JSON.

## Open Questions

- Should behavior declarations stay in `.varg`, or should large behavior graphs get extracted later if they become noisy?
- Should scene intent remain in `.vscene`, or compile into a separate generated `.vscene.ir` artifact?
- Should the first script runtime be a custom interpreter, a Rhai transpiler, Rune, Lua, or WASM?
- How much Swift optional syntax should MVP support beyond `Type?`, `if let`, and `guard let`?
- Should Varg allow module imports in MVP, or keep scripts one-file until shared code is necessary?
- Should network declarations live in `.vscene` permanently, or split only when multiplayer features outgrow world files?
