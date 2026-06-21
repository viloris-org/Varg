# Aster Engine Context

Aster is a game engine whose runtime and editor share scene extraction, rendering, and platform-independent engine policies.

## Language

**Render World**:
The immutable, per-frame rendering input extracted from the active scene. One Render World contains zero or one camera and many render objects, sprites, lights, and particles.
_Avoid_: Render queue, scene snapshot

**Frame Pipeline**:
The compiled sequence of rendering passes, resource accesses, scaling stages, and presentation work used to produce one frame from a Render World.
_Avoid_: Render loop, hard-coded pass chain

**Visibility Set**:
The subset of a Render World selected for a particular view after frustum culling and level-of-detail selection.
_Avoid_: Visible list, culled scene

**Render Scaling**:
The policy and frame data that separate internal rendering resolution from output and UI composition resolution.
_Avoid_: Resolution hack, resize path

## Example dialogue

> Developer: Does the Frame Pipeline consume the entire Render World?
>
> Graphics programmer: It first derives a Visibility Set for the active camera, then executes shadow, forward, scaling, post-processing, and UI passes according to the compiled Frame Pipeline.
