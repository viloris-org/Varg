# Game Feel And Juice

Improve game feel with immediate action feedback, readable state changes, audio cues, motion variation, cooldown clarity, and small reward moments.

Use this skill when the user asks to polish, juice, improve feel, make gameplay satisfying, add feedback, or make a prototype feel more like a game.

## Feedback Layers

Add at least two feedback layers for important actions:
- **Motion**: quick translate, bob, pulse, recoil, lift, hide, reveal, or state-based repositioning.
- **Audio**: short `Audio.playTone` cue for collect, hit, fail, win, low health, cooldown ready, or interaction.
- **State**: score, health, timer, opened, collected, alerted, dead, won, or failed variable.
- **Readability**: object names, spacing, color/material if available, light, camera, and clear entity roles.
- **Rhythm**: cooldowns, timers, patrol turns, pulse intervals, or escalating pace.

## Current-API Friendly Techniques

With the MVP scripting subset, prefer:
- `Audio.playTone` for one-shot confirmation.
- `Audio.startLoop` / `Audio.stopLoop` for ambient or pressure states.
- Position changes to reveal, hide, pulse, or mark completion.
- Timers and cooldown variables to prevent noisy repeated feedback.
- Logs only for debugging or temporary HUD-like status when no UI exists.

## Polish Pass Checklist

For each key action, ask:
- Does the player know the input worked?
- Does success feel different from failure?
- Is danger communicated before punishment?
- Is progress visible or audible?
- Are repeated actions paced by cooldowns or rhythm?
- Are tuning values exported for iteration?

Polish should support the mechanic. Do not add unrelated sounds or motion that makes the goal harder to read.
