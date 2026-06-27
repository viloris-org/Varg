# Level Layout

Design compact levels that teach the goal, guide the camera, stage obstacles, and make progress readable through object placement and simple state.

Use this skill for levels, rooms, arenas, platforming routes, exploration spaces, puzzles, quests, onboarding, or scene composition that should be playable.

## Layout Principles

Build the level around a route and a decision:
- **Start**: where the player begins and what they can immediately understand.
- **Landmark**: a visible goal, exit, pickup cluster, enemy, locked door, or hazard.
- **Path**: the primary route from start to goal.
- **Choice**: risk/reward branch, timing window, enemy bypass, optional pickup, or shortcut.
- **Gate**: score threshold, key, interaction, timer, position threshold, or defeat condition.
- **Return signal**: feedback that tells the player a gate opened, a danger ended, or the objective changed.

## Scene Authoring Pattern

1. Place Player at a clear start.
2. Place the objective in camera-readable space.
3. Add blockers, hazards, pickups, or enemies in small groups.
4. Use names that reveal purpose: `Exit Gate`, `Risk Pickup`, `Patrol Hazard`, `Safe Lane`.
5. Add camera and light early so the viewport communicates the intended play path.
6. Attach scripts to route-critical objects, not just the player.

## Pacing

Use three beats for a first playable level:
- Learn: safe space to try the verb.
- Test: one obstacle or timing challenge.
- Reward: score, pickup, open gate, tone, or visible state change.

Do not fill the level with many static props before the route and objective work.
