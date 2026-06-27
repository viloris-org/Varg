# Playable Prototype

Design a small playable game loop with a clear player verb, goal, obstacle, feedback, tuning hooks, and win or fail state.

Use this skill when the user asks to make a game, prototype, vertical slice, mechanic, challenge, quest, minigame, or interactive scene.

## Prototype Contract

A prototype is not complete until the player can do something, the world responds, and the scene can reach a success or failure state. Keep the first pass small enough to test in 30-90 seconds.

Define these before writing scripts:
- **Verb**: the repeated action the player controls.
- **Goal**: what the player is trying to accomplish.
- **Obstacle**: what creates tension or forces decisions.
- **Feedback**: what confirms action, progress, danger, success, or failure.
- **Tuning**: which values should be exported for iteration.
- **End condition**: win, loss, timeout, completion, or reset-ready state.

## Build Pattern

1. Create or reuse a named Player entity with input logic.
2. Add one readable objective object, zone, pickup, enemy, timer, or hazard.
3. Add one pressure source: distance, cooldown, patrol, timer, health drain, limited score window, or collision flag.
4. Add feedback through movement, object hiding/showing by position, audio tones, logs, particles, or visible state objects.
5. Add exported values for speed, thresholds, target score, timer length, cooldowns, or damage.
6. Validate every changed script and attach it to scene entities before completion.

## Quality Bar

Prefer a complete toy loop over an impressive static layout. A good answer can say what is playable in one sentence, such as: "Move through the arena, collect three cores before the hazard timer expires, and hear a tone when each pickup is scored."

Avoid creating only background props unless the user explicitly asked for a non-playable scene.
