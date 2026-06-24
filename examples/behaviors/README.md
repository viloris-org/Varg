# Declarative Behavior Examples

This directory contains example behavior trees in JSON format for the `engine-script-declarative` system.

## Files

### `player_controller.json`
Basic WASD + Space player controller using parallel behaviors.

**LLM Prompt to generate this:**
```
Create a player controller behavior for an Aster game engine entity with:
- WASD movement (forward at 5.0 speed, back at 3.0, strafe at 4.0)
- Space to jump with upward impulse of 15
- All controls should work simultaneously (parallel execution)
```

### `enemy_patrol.json`
Enemy AI that patrols waypoints and chases/attacks the player when close.

**LLM Prompt:**
```
Create an enemy AI behavior that:
- Patrols between 4 waypoints in a square at 2.0 speed
- When player is within 8 units, switch to combat mode
- In combat: if within 2 units, attack for 15 damage; otherwise chase at 4.5 speed
- Return to patrol when player leaves range
```

### `fleeing_npc.json`
NPC that runs away when threatened and heals when safe.

**LLM Prompt:**
```
Create an NPC behavior that:
- Flees from player at 6.0 speed when player is within 5 units
- When health is below 80 and player is far (>10 units), play heal sound and wait 2 seconds
- Otherwise idle
```

## Structure

All behaviors follow the JSON schema defined in `engine-script-declarative`. The basic structure is:

```json
{
  "entity": "EntityName",
  "description": "Human-readable description",
  "behaviors": [
    {
      "type": "NodeType",
      "children": [...]
    }
  ]
}
```

## Node Types

- **Sequence**: Execute children in order, fail on first failure
- **Selector**: Try children in order, succeed on first success
- **Parallel**: Execute all children simultaneously
- **Condition**: Check a condition (keyPressed, playerDistance, health, etc.)
- **Action**: Perform an action (moveForward, chase, attack, etc.)

## Using in Code

```rust
use engine_script_declarative::DeclarativeScriptBackend;

let mut backend = DeclarativeScriptBackend::new();

// Load behavior
backend.load_behavior(Path::new("behaviors/enemy_patrol.json"))?;

// In game loop:
backend.set_input_state(input_state);
backend.set_scene(scene);

// Execute for each entity
let result = backend.execute_mut(entity, behavior_path, delta_time)?;
```

## Benefits for LLMs

These JSON behaviors are significantly easier for LLMs to generate correctly compared to imperative scripts because:

1. **Structured**: Clear hierarchical tree format
2. **Declarative**: Describe "what" not "how"
3. **Composable**: Complex behaviors from simple blocks
4. **Validatable**: JSON Schema can catch errors
5. **Pattern-based**: Consistent, repeatable structures

## Comparison with Imperative Scripts

**Imperative**:
- 27+ lines of code for enemy AI
- Manual state management
- Loop logic, conditionals, math
- Error-prone for LLMs

**Declarative (JSON)**:
- Clear 72-line tree structure
- Zero manual state
- Visual hierarchy
- Much easier for LLMs to generate correctly
