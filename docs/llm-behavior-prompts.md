# LLM Prompt Template for Generating Aster Behaviors

Use this template when asking an LLM to generate behavior trees for the Aster game engine.

## Basic Template

```
Create a behavior tree in JSON format for the Aster game engine with the following requirements:

Entity: [ENTITY_NAME]
Description: [BEHAVIOR_DESCRIPTION]

Requirements:
- [REQUIREMENT_1]
- [REQUIREMENT_2]
- [REQUIREMENT_3]

The JSON should follow the Aster BehaviorSchema format with:
- "entity": string (entity name)
- "description": string (optional)
- "behaviors": array of BehaviorNode objects

Available node types:
- Sequence: Execute children in order until one fails
- Selector: Try children in order until one succeeds
- Parallel: Execute all children simultaneously
- Condition: Check a condition (keyPressed, playerDistance, health, etc.)
- Action: Perform an action (moveForward, chase, patrol, attack, etc.)
```

## Example Prompts

### Example 1: Enemy AI

```
Create a behavior tree in JSON format for the Aster game engine with the following requirements:

Entity: TurretEnemy
Description: Stationary turret that shoots at player when in range

Requirements:
- When player is within 15 units, rotate to face player
- If player is within 10 units and facing player, shoot every 1 second
- Otherwise, rotate slowly (30 degrees per second) scanning for targets
- Play "turret_fire.ogg" sound when shooting

Use Selector for choosing between combat and idle, Sequence for action chains.
```

### Example 2: Advanced NPC

```
Create a behavior tree in JSON format for the Aster game engine with the following requirements:

Entity: MerchantNPC
Description: Friendly merchant that follows player when called

Requirements:
- If player presses "E" key within 3 units, toggle follow mode
- In follow mode: chase player at 4.0 speed, stop when within 2 units
- When not following and player is far (>5 units), patrol between 3 waypoints: [0,0,0], [5,0,0], [5,0,5]
- If health drops below 30, flee from player at 6.0 speed and play "merchant_scared.ogg"

Use Parallel for simultaneously checking health while doing other behaviors.
```

### Example 3: Environmental Hazard

```
Create a behavior tree in JSON format for the Aster game engine with the following requirements:

Entity: SpikeTrap
Description: Trap that activates periodically and damages nearby players

Requirements:
- Wait 3 seconds
- Play "trap_activate.ogg" sound
- For 1 second, check if player is within 2 units; if so, deal 25 damage
- Wait 2 seconds
- Repeat indefinitely

Use Repeat node with Sequence for the cycle.
```

## Tips for LLMs

1. **Always start with entity and description fields**
2. **Use appropriate control nodes**:
   - Sequence for "do A, then B, then C"
   - Selector for "try A, if fails try B"
   - Parallel for "do A and B simultaneously"
3. **Nest conditions before actions** in Sequence nodes
4. **Common condition patterns**:
   ```json
   {"keyPressed": "KEY_NAME"}
   {"playerDistance": {"lessThan": 5.0}}
   {"health": {"greaterThan": 50}}
   ```
5. **Common action patterns**:
   ```json
   {"moveForward": 5.0}
   {"chase": {"target": "player", "speed": 4.0}}
   {"patrol": {"points": [[0,0,0], [10,0,0]], "speed": 2.0}}
   ```
6. **Always validate JSON syntax** - missing commas and braces are common errors

## Full Condition Types

- `keyPressed`, `keyHeld`, `keyReleased`: `{"KEY_NAME"}`
- `playerDistance`: `{"lessThan"|"greaterThan"|"between": value}`
- `health`: `{"lessThan"|"greaterThan"|"equal": value}`
- `hasTag`: `{"tag_name"}`
- `raycastHit`: `{"direction": [x,y,z], "maxDistance": value}`
- `and`, `or`, `not`: Boolean operators
- `always`, `never`: For testing

## Full Action Types

**Movement**:
- `moveForward`, `moveBackward`, `strafeLeft`, `strafeRight`: `{speed}`
- `translate`: `{"offset": [x,y,z]}`
- `setPosition`: `{"position": [x,y,z]}`
- `rotateY`: `{"degrees_per_sec": value}`
- `lookAt`: `{"target": [x,y,z]}`

**AI**:
- `chase`: `{"target": "player"|entityId, "speed": value}`
- `flee`: `{"target": "player"|entityId, "speed": value}`
- `patrol`: `{"points": [[x,y,z], ...], "speed": value, "loop": bool}`

**Combat**:
- `attack`: `{"target": "player"|entityId, "damage": value}`
- `applyImpulse`: `{"force": [x,y,z]}`

**Effects**:
- `playSound`: `{"path": "sounds/file.ogg"}`
- `spawn`: `{"prefab": "path/to/prefab.json", "position": [x,y,z]}`

**Utility**:
- `wait`: `{duration_seconds}`
- `destroySelf`: true
- `sequence`: `[action1, action2, ...]`
- `idle`: no parameters

## Schema Reference

The complete JSON Schema is available in `schema/aster-behavior-schema.json` for tool use integration.
