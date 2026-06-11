import React, { useCallback, useEffect, useRef, useState } from 'react';
import {
  Vec3, vec3, vec3Add, vec3Sub, vec3Scale, vec3Normalize, vec3Cross, vec3Length, vec3Dot,
  Mat4,
  createViewMatrix, createPerspectiveMatrix,
  createOrthographicMatrix,
  projectToScreen, rayFromScreen,
  closestPointOnRayToAxis, screenDeltaToWorldDelta,
  gizmoWorldScale, cameraBasisVectors,
} from './gizmoMath';

// ─── Types ──────────────────────────────────────────────────────────────────

export type TransformTool = 'view' | 'move' | 'rotate' | 'scale';
export type TransformSpace = 'global' | 'local';

interface CameraState {
  yaw: number;
  pitch: number;
  distance: number;
  targetX: number;
  targetY: number;
  targetZ: number;
}

interface TransformGizmoProps {
  cameraState: CameraState;
  selectedPosition: Vec3 | null;
  activeTool: TransformTool;
  space: TransformSpace;
  moveSnap: number;
  angleSnap: number;
  snapEnabled: boolean;
  viewMode: '2d' | '3d';
  onTransformDelta: (tool: TransformTool, delta: { position?: Vec3; rotation?: Vec3; scale?: Vec3 }) => void;
  onTransformEnd: () => void;
}

// ─── Axis Colors ───────────────────────────────────────────────────────────

const AXIS_X = '#FF4444';
const AXIS_Y = '#44CC44';
const AXIS_Z = '#4488FF';
const AXIS_CENTER = '#AAAAAA';
const AXIS_HIGHLIGHT = '#FFFFFF';

const AXES: { dir: Vec3; color: string; colorHex: string; name: string }[] = [
  { dir: [1, 0, 0], color: AXIS_X, colorHex: '#FF4444', name: 'X' },
  { dir: [0, 1, 0], color: AXIS_Y, colorHex: '#44CC44', name: 'Y' },
  { dir: [0, 0, 1], color: AXIS_Z, colorHex: '#4488FF', name: 'Z' },
];

// ─── Handle Types for Hit Testing ─────────────────────────────────────────

interface GizmoHandle {
  axis: 'x' | 'y' | 'z' | 'center' | 'xy' | 'xz' | 'yz' | 'free';
  tool: TransformTool;
  screenX?: number;
  screenY?: number;
  hitRadius?: number;
}

// ─── Component ──────────────────────────────────────────────────────────────

export default function TransformGizmo({
  cameraState,
  selectedPosition,
  activeTool,
  space,
  moveSnap,
  angleSnap,
  snapEnabled,
  viewMode,
  onTransformDelta,
  onTransformEnd,
}: TransformGizmoProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const sizeRef = useRef({ width: 640, height: 480 });
  const [, forceRender] = useState(0);
  const dragRef = useRef<{
    active: boolean;
    handle: GizmoHandle | null;
    initialPos: Vec3 | null;
    initialMouse: { x: number; y: number } | null;
  }>({ active: false, handle: null, initialPos: null, initialMouse: null });

  const handlesRef = useRef<GizmoHandle[]>([]);

  // Observe parent size
  useEffect(() => {
    const container = containerRef.current?.parentElement;
    if (!container) return;
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const w = Math.round(entry.contentRect.width);
        const h = Math.round(entry.contentRect.height);
        if (w > 0 && h > 0) {
          sizeRef.current = { width: w, height: h };
          forceRender(v => v + 1);
        }
      }
    });
    observer.observe(container);
    // Init size
    const rect = container.getBoundingClientRect();
    sizeRef.current = { width: Math.round(rect.width), height: Math.round(rect.height) };
    return () => observer.disconnect();
  }, []);

  if (activeTool === 'view' || !selectedPosition) {
    return null;
  }

  const viewportWidth = sizeRef.current.width;
  const viewportHeight = sizeRef.current.height;

  // ── Build view/projection matrices ──

  const { yaw, pitch, distance, targetX, targetY, targetZ } = cameraState;
  const viewMatrix = createViewMatrix(
    viewMode === '2d' ? 0 : yaw,
    viewMode === '2d' ? 0 : pitch,
    distance,
    targetX,
    targetY,
    targetZ,
  );
  const fovRadians = 60 * Math.PI / 180;
  const aspect = viewportWidth / (viewportHeight || 1);
  const projMatrix = viewMode === '2d'
    ? createOrthographicMatrix(distance * 2, aspect, 0.01, 1000)
    : createPerspectiveMatrix(fovRadians, aspect, 0.01, 1000);

  const worldScale = gizmoWorldScale(distance, 100);
  const { right: camRight, up: camUp } = cameraBasisVectors(yaw, pitch);

  // ── Draw function ──

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas || !selectedPosition) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const w = viewportWidth;
    const h = viewportHeight;
    canvas.width = w;
    canvas.height = h;

    ctx.clearRect(0, 0, w, h);

    const pos = selectedPosition;
    const handles: GizmoHandle[] = [];

    // Project center
    const center = projectToScreen(pos, viewMatrix, projMatrix, w, h);
    if (!center) return;

    const handleRadius = Math.max(12, Math.min(24, 600 / distance));

    if (activeTool === 'move') {
      // Draw move arrows
      for (const axis of AXES) {
        // TODO: When selectedRotation prop is available, compute local-space axis from entity rotation
        const dir = axis.dir;
        const end = vec3Add(pos, vec3Scale(dir, worldScale));
        const startProj = projectToScreen(pos, viewMatrix, projMatrix, w, h);
        const endProj = projectToScreen(end, viewMatrix, projMatrix, w, h);
        if (!startProj || !endProj) continue;

        // Check if pointing toward or away from camera
        const towardCamera = endProj.depth < startProj.depth;

        ctx.strokeStyle = towardCamera ? axis.color + '88' : axis.color;
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(startProj.x, startProj.y);
        ctx.lineTo(endProj.x, endProj.y);
        ctx.stroke();

        // Arrow head (cone tip)
        const tipX = endProj.x;
        const tipY = endProj.y;
        const tipSize = 6;
        ctx.fillStyle = axis.color;
        ctx.beginPath();
        if (towardCamera) {
          ctx.arc(tipX, tipY, handleRadius, 0, Math.PI * 2);
          ctx.fill();
        } else {
          const arrowDir = vec3Normalize([startProj.x - endProj.x, startProj.y - endProj.y, 0]);
          const perpX = -arrowDir[1];
          const perpY = arrowDir[0];
          ctx.moveTo(tipX, tipY);
          ctx.lineTo(tipX + arrowDir[0] * tipSize * 2 + perpX * tipSize, tipY + arrowDir[1] * tipSize * 2 + perpY * tipSize);
          ctx.lineTo(tipX + arrowDir[0] * tipSize * 2 - perpX * tipSize, tipY + arrowDir[1] * tipSize * 2 - perpY * tipSize);
          ctx.closePath();
          ctx.fill();
        }

        // Register handle
        handles.push({
          axis: axis.name.toLowerCase() as 'x' | 'y' | 'z',
          tool: 'move',
          screenX: tipX,
          screenY: tipY,
          hitRadius: handleRadius + 4,
        });
      }

      // Center handle (free move in view plane)
      ctx.fillStyle = AXIS_CENTER;
      ctx.beginPath();
      ctx.arc(center.x, center.y, 8, 0, Math.PI * 2);
      ctx.fill();
      ctx.strokeStyle = '#888';
      ctx.lineWidth = 1;
      ctx.stroke();

      handles.push({
        axis: 'center',
        tool: 'move',
        screenX: center.x,
        screenY: center.y,
        hitRadius: 10,
      });

    } else if (activeTool === 'rotate') {
      // Draw rotation rings (as ellipses projected from circles in view space)
      const ringRadius = worldScale * 0.7;
      const ringSegments = 64;

      for (const axis of AXES) {
        // TODO: When selectedRotation prop is available, compute local-space axis from entity rotation
        const axisDir = axis.dir;

        // The ring is a circle in the plane perpendicular to the axis
        // For view-space rendering, we approximate by drawing the projected ellipse
        ctx.strokeStyle = axis.color;
        ctx.lineWidth = 2.5;
        ctx.beginPath();

        let firstPoint = true;
        let avgX = 0, avgY = 0;
        let pointCount = 0;

        for (let i = 0; i <= ringSegments; i++) {
          const angle = (i / ringSegments) * Math.PI * 2;
          // Compute a point on the ring in world space
          // Build a basis for the plane perpendicular to the axis
          let perpA: Vec3, perpB: Vec3;
          if (Math.abs(axisDir[1]) < 0.99) {
            perpA = vec3Normalize(vec3Cross(axisDir, [0, 1, 0]));
          } else {
            perpA = vec3Normalize(vec3Cross(axisDir, [1, 0, 0]));
          }
          perpB = vec3Cross(axisDir, perpA);

          const point: Vec3 = vec3Add(
            vec3Add(pos, vec3Scale(perpA, Math.cos(angle) * ringRadius)),
            vec3Scale(perpB, Math.sin(angle) * ringRadius),
          );

          const proj = projectToScreen(point, viewMatrix, projMatrix, w, h);
          if (proj) {
            if (firstPoint) { ctx.moveTo(proj.x, proj.y); firstPoint = false; }
            else { ctx.lineTo(proj.x, proj.y); }
            avgX += proj.x;
            avgY += proj.y;
            pointCount++;
          }
        }
        ctx.closePath();
        ctx.stroke();

        // Register handle at the center of the visible ring
        if (pointCount > 0) {
          handles.push({
            axis: axis.name.toLowerCase() as 'x' | 'y' | 'z',
            tool: 'rotate',
            screenX: avgX / pointCount,
            screenY: avgY / pointCount,
            hitRadius: handleRadius,
          });
        }
      }

      // Outer free-rotate ring
      ctx.strokeStyle = AXIS_CENTER;
      ctx.lineWidth = 1.5;
      ctx.setLineDash([4, 4]);
      ctx.beginPath();
      ctx.arc(center.x, center.y, ringRadius * 1.3 * (viewportHeight / 500) * distance, 0, Math.PI * 2);
      ctx.stroke();
      ctx.setLineDash([]);

      handles.push({
        axis: 'free',
        tool: 'rotate',
        screenX: center.x,
        screenY: center.y,
        hitRadius: ringRadius * 1.3 * (viewportHeight / 500) * distance + 8,
      });

    } else if (activeTool === 'scale') {
      // Draw scale handles (cubes at axis endpoints)
      for (const axis of AXES) {
        // TODO: When selectedRotation prop is available, compute local-space axis from entity rotation
        const dir = axis.dir;
        const end = vec3Add(pos, vec3Scale(dir, worldScale));
        const endProj = projectToScreen(end, viewMatrix, projMatrix, w, h);
        if (!endProj) continue;

        // Line to handle
        ctx.strokeStyle = axis.color + '88';
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        ctx.moveTo(center.x, center.y);
        ctx.lineTo(endProj.x, endProj.y);
        ctx.stroke();

        // Cube handle
        const cubeHalf = 6;
        ctx.fillStyle = axis.color;
        ctx.fillRect(endProj.x - cubeHalf, endProj.y - cubeHalf, cubeHalf * 2, cubeHalf * 2);
        ctx.strokeStyle = '#fff';
        ctx.lineWidth = 1;
        ctx.strokeRect(endProj.x - cubeHalf, endProj.y - cubeHalf, cubeHalf * 2, cubeHalf * 2);

        handles.push({
          axis: axis.name.toLowerCase() as 'x' | 'y' | 'z',
          tool: 'scale',
          screenX: endProj.x,
          screenY: endProj.y,
          hitRadius: cubeHalf + 4,
        });
      }

      // Center uniform scale
      const cubeHalf = 7;
      ctx.fillStyle = AXIS_CENTER;
      ctx.fillRect(center.x - cubeHalf, center.y - cubeHalf, cubeHalf * 2, cubeHalf * 2);
      ctx.strokeStyle = '#888';
      ctx.lineWidth = 1;
      ctx.strokeRect(center.x - cubeHalf, center.y - cubeHalf, cubeHalf * 2, cubeHalf * 2);

      handles.push({
        axis: 'center',
        tool: 'scale',
        screenX: center.x,
        screenY: center.y,
        hitRadius: cubeHalf + 4,
      });
    }

    handlesRef.current = handles;
  }, [selectedPosition, activeTool, space, viewMatrix, projMatrix, viewportWidth, viewportHeight, worldScale, distance, camRight, camUp]);

  useEffect(() => {
    draw();
  }, [draw]);

  // ── Hit testing ──

  const hitTest = useCallback((clientX: number, clientY: number): GizmoHandle | null => {
    const canvas = canvasRef.current;
    if (!canvas) return null;
    const rect = canvas.getBoundingClientRect();
    const x = clientX - rect.left;
    const y = clientY - rect.top;

    // Check handles from front to back (by depth)
    for (const handle of handlesRef.current) {
      if (handle.screenX === undefined || handle.screenY === undefined) continue;
      const dx = x - handle.screenX;
      const dy = y - handle.screenY;
      const dist = Math.sqrt(dx * dx + dy * dy);
      if (dist < (handle.hitRadius ?? 16)) {
        return handle;
      }
    }
    return null;
  }, []);

  // ── Pointer event handlers ──

  const onPointerDown = useCallback((e: React.PointerEvent) => {
    if (!selectedPosition) return;

    const handle = hitTest(e.clientX, e.clientY);
    if (!handle) return;

    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    dragRef.current = {
      active: true,
      handle,
      initialPos: selectedPosition ? [...selectedPosition] as Vec3 : null,
      initialMouse: { x: e.clientX, y: e.clientY },
    };
    e.preventDefault();
    e.stopPropagation();
  }, [activeTool, selectedPosition, hitTest]);

  const onPointerMove = useCallback((e: React.PointerEvent) => {
    const drag = dragRef.current;
    if (!drag.active || !drag.handle || !drag.initialPos || !drag.initialMouse) return;

    const dx = e.clientX - drag.initialMouse.x;
    const dy = e.clientY - drag.initialMouse.y;

    if (drag.handle.tool === 'move') {
      if (drag.handle.axis === 'center') {
        // Free move in view plane
        const delta = screenDeltaToWorldDelta(dx, dy, camRight, camUp, distance);
        onTransformDelta('move', { position: delta });
      } else {
        // Axis-constrained move
        const axisIdx = drag.handle.axis === 'x' ? 0 : drag.handle.axis === 'y' ? 1 : 2;
        // TODO: When selectedRotation prop is available, compute local-space axis from entity rotation
        const axisDir = AXES[axisIdx].dir;
        const ray = rayFromScreen(
          e.clientX - (canvasRef.current?.getBoundingClientRect().left ?? 0),
          e.clientY - (canvasRef.current?.getBoundingClientRect().top ?? 0),
          viewMatrix, projMatrix, viewportWidth, viewportHeight,
        );
        const t = closestPointOnRayToAxis(ray.origin, ray.direction, drag.initialPos, axisDir);
        const worldPt = vec3Add(ray.origin, vec3Scale(ray.direction, t));
        const delta = vec3Sub(worldPt, drag.initialPos);
        const projectedDelta = vec3Scale(axisDir, vec3Dot(delta, axisDir));

        let finalDelta = projectedDelta;
        if (snapEnabled && moveSnap > 0) {
          for (let i = 0; i < 3; i++) {
            finalDelta[i] = Math.round(finalDelta[i] / moveSnap) * moveSnap;
          }
        }
        onTransformDelta('move', { position: finalDelta });
      }
    } else if (drag.handle.tool === 'rotate') {
      if (drag.handle.axis === 'free') {
        // Free rotate around view direction
        const angle = Math.atan2(dx, -dy) * 0.01;
        const forward = vec3Normalize([-camRight[1], camUp[1], -camRight[0]]);
        onTransformDelta('rotate', { rotation: vec3Scale(forward, angle) });
      } else {
        // Axis-constrained rotate
        const axisIdx = drag.handle.axis === 'x' ? 0 : drag.handle.axis === 'y' ? 1 : 2;
        const rotation: Vec3 = [0, 0, 0];
        // Screen-space arc drag -> angle around axis
        const angle = Math.atan2(dx, -dy) * 0.015;
        rotation[axisIdx] = angle;

        if (snapEnabled && angleSnap > 0) {
          rotation[axisIdx] = Math.round(rotation[axisIdx] / (angleSnap * Math.PI / 180)) * (angleSnap * Math.PI / 180);
        }
        onTransformDelta('rotate', { rotation });
      }
    } else if (drag.handle.tool === 'scale') {
      const axisIdx = drag.handle.axis === 'x' ? 0 : drag.handle.axis === 'y' ? 1 : 2;
      if (drag.handle.axis === 'center') {
        // Uniform scale
        const factor = 1 + (dx - dy) * 0.005;
        onTransformDelta('scale', { scale: [factor, factor, factor] });
      } else {
        // Axis scale
        const factor = 1 + (dx - dy) * 0.005;
        const scale: Vec3 = [1, 1, 1];
        scale[axisIdx] = factor;
        if (snapEnabled && moveSnap > 0) {
          scale[axisIdx] = Math.round(scale[axisIdx] / moveSnap) * moveSnap;
        }
        onTransformDelta('scale', { scale });
      }
    }

    // Update initial for next delta
    dragRef.current.initialMouse = { x: e.clientX, y: e.clientY };
  }, [space, snapEnabled, moveSnap, angleSnap, camRight, camUp, distance, viewMatrix, projMatrix, viewportWidth, viewportHeight, onTransformDelta]);

  const onPointerUp = useCallback((e: React.PointerEvent) => {
    if (dragRef.current.active) {
      dragRef.current.active = false;
      dragRef.current.handle = null;
      onTransformEnd();
    }
  }, [onTransformEnd]);

  // Hover effect for cursor
  const onPointerMoveHover = useCallback((e: React.PointerEvent) => {
    if (dragRef.current.active) return;
    const handle = hitTest(e.clientX, e.clientY);
    const canvas = canvasRef.current;
    if (canvas) {
      canvas.style.cursor = handle ? 'grab' : 'default';
    }
  }, [hitTest]);

  const onPointerDownHover = useCallback((e: React.PointerEvent) => {
    const handle = hitTest(e.clientX, e.clientY);
    const canvas = canvasRef.current;
    if (canvas && handle) {
      canvas.style.cursor = 'grabbing';
    }
  }, [hitTest]);

  return (
    <div
      ref={containerRef}
      className={`transform-gizmo ${dragRef.current.active ? 'active' : ''}`}
      style={{
        position: 'absolute',
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        pointerEvents: 'auto',
        overflow: 'hidden',
      }}
    >
      <canvas
        ref={canvasRef}
        style={{
          position: 'absolute',
          top: 0,
          left: 0,
          width: viewportWidth,
          height: viewportHeight,
        }}
        onPointerDown={onPointerDown}
        onPointerMove={(e) => {
          onPointerMoveHover(e);
          if (dragRef.current.active) onPointerMove(e);
        }}
        onPointerUp={onPointerUp}
        onPointerDownCapture={onPointerDownHover}
      />
    </div>
  );
}
