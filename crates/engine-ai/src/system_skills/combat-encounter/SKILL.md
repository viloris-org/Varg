# Combat Encounter

Design readable combat encounters with player intent, enemy pressure, damage rules, cooldowns, arena layout, feedback, and victory or defeat.

Use this skill for enemies, bosses, weapons, projectiles, hazards, arenas, survival waves, or any request involving fighting.

## Combat Checks

Before authoring scripts, decide:
- **Player attack verb**: fire, melee, dodge, bait, parry, kite, or position.
- **Enemy behavior**: patrol, chase, guard, pulse attack, timed hazard, proximity threat, or stationary turret.
- **Damage rule**: what causes damage and how often it can happen.
- **Avoidance rule**: how a skilled player reduces or avoids damage.
- **Readable tell**: motion, timer, sound, position, or named object that communicates danger.
- **Outcome**: enemy defeated, survival timer completed, player health reaches zero, or arena state changes.

## MVP Combat With Current APIs

Use supported Varg APIs honestly. If true spawning or physics impulses are unavailable, build combat from pre-placed actors, position thresholds, timers, input checks, cooldowns, and state flags.

Useful script responsibilities:
- Player movement and attack input.
- Enemy patrol or timed attack.
- Health and invulnerability cooldown.
- Projectile or hazard lifetime.
- Score, defeat, or survival timer.
- Audio feedback for hit, miss, damage, low health, or win.

## Encounter Layout

Make the arena readable from the camera:
- Place Player, enemies, objective, and hazards with distinct names and spacing.
- Add light and camera if the scene lacks them.
- Keep the first encounter small: one enemy type, one hazard type, one objective.
- Put tuning values on scripts with `@export` for speed, damage, range, cooldown, score target, and timer length.

## Completion Standard

The encounter should have at least one player decision, one enemy or hazard behavior, one feedback signal, and one end condition. Do not finish with only a weapon script or only an enemy model.
