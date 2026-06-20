import React, { useCallback, useMemo } from 'react';
import {
  orientationGizmoAxisClass,
  orientationGizmoClass,
  viewportGridClass,
} from '../uiClasses';
import { cameraBasisVectors, vec3Dot, type Vec3 } from './gizmoMath';

// ─── Viewport Grid Overlay ─────────────────────────────────────────────────
// A simple reference grid that renders as a CSS pattern on the viewport.
// Toggle with the `showGrid` prop.

interface ViewportGridProps {
  show?: boolean;
}

export function ViewportGrid({ show = true }: ViewportGridProps) {
  if (!show) return null;
  return <div className={viewportGridClass} />;
}

// ─── Orientation Gizmo ─────────────────────────────────────────────────────
// Shows the current XYZ axes orientation in the top-right corner.
// Click on an axis label to snap the view to that axis.

interface OrientationGizmoProps {
  camera?: {
    yaw: number;
    pitch: number;
  };
  onSnapToAxis?: (axis: 'front' | 'back' | 'left' | 'right' | 'top' | 'bottom') => void;
}

type AxisName = 'x' | 'y' | 'z' | '-x' | '-y' | '-z';

interface GizmoAxis {
  name: AxisName;
  label: string;
  color: string;
  end: { x: number; y: number };
  depth: number;
}

const AXIS_DEFS: Array<{ name: AxisName; label: string; color: string; dir: Vec3 }> = [
  { name: 'x', label: 'X', color: '#FF4D4D', dir: [1, 0, 0] },
  { name: '-x', label: '-X', color: '#B91C1C', dir: [-1, 0, 0] },
  { name: 'y', label: 'Y', color: '#4ADE5C', dir: [0, 1, 0] },
  { name: '-y', label: '-Y', color: '#15803D', dir: [0, -1, 0] },
  { name: 'z', label: 'Z', color: '#4D8DFF', dir: [0, 0, 1] },
  { name: '-z', label: '-Z', color: '#1D4ED8', dir: [0, 0, -1] },
];

export function OrientationGizmo({ camera = { yaw: -0.5, pitch: 0.3 }, onSnapToAxis }: OrientationGizmoProps) {
  const handleClick = useCallback((axis: string) => {
    if (!onSnapToAxis) return;
    // Map arrow clicks to view presets
    // +X = right, -X = left, +Y = top, -Y = bottom, +Z = back, -Z = front
    // For an identity camera looking along -Z:
    //   "right" → +X axis, "left" → -X axis
    //   "top" → +Y axis, "bottom" → -Y axis
    //   "front" → -Z axis (toward camera), "back" → +Z axis (away from camera)
    switch (axis) {
      case 'x': onSnapToAxis('right'); break;
      case '-x': onSnapToAxis('left'); break;
      case 'y': onSnapToAxis('top'); break;
      case '-y': onSnapToAxis('bottom'); break;
      case 'z': onSnapToAxis('back'); break;
      case '-z': onSnapToAxis('front'); break;
    }
  }, [onSnapToAxis]);

  const axes = useMemo<GizmoAxis[]>(() => {
    const basis = cameraBasisVectors(camera.yaw, camera.pitch);
    return AXIS_DEFS.map((axis) => ({
      name: axis.name,
      label: axis.label,
      color: axis.color,
      end: {
        x: vec3Dot(axis.dir, basis.right),
        y: -vec3Dot(axis.dir, basis.up),
      },
      depth: vec3Dot(axis.dir, basis.forward),
    })).sort((a, b) => a.depth - b.depth);
  }, [camera.pitch, camera.yaw]);

  return (
    <div className={orientationGizmoClass}>
      <svg viewBox="-1.2 -1.2 2.4 2.4" width="80" height="80">
        {axes.map((axis) => {
          const x = axis.end.x * 0.82;
          const y = axis.end.y * 0.82;
          const isFacing = axis.depth < 0;
          return (
            <g
              key={axis.name}
              className={orientationGizmoAxisClass}
              opacity={isFacing ? 1 : 0.56}
              onClick={() => handleClick(axis.name)}
            >
              <line x1="0" y1="0" x2={x} y2={y} stroke={axis.color} strokeWidth={isFacing ? 0.08 : 0.055} />
              <circle cx={x} cy={y} r={isFacing ? 0.18 : 0.14} fill={axis.color} />
              <text
                x={x}
                y={y + 0.045}
                fill="white"
                fontSize={axis.label.length > 1 ? 0.13 : 0.16}
                fontWeight="bold"
                textAnchor="middle"
                dominantBaseline="middle"
              >
                {axis.label}
              </text>
            </g>
          );
        })}
        <circle cx="0" cy="0" r="0.15" fill="#555" stroke="#888" strokeWidth="0.02" />
      </svg>
    </div>
  );
}
