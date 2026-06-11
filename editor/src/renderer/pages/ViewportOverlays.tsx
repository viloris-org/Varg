import React, { useCallback } from 'react';

// ─── Viewport Grid Overlay ─────────────────────────────────────────────────
// A simple reference grid that renders as a CSS pattern on the viewport.
// Toggle with the `showGrid` prop.

interface ViewportGridProps {
  show?: boolean;
}

export function ViewportGrid({ show = true }: ViewportGridProps) {
  if (!show) return null;
  return <div className="viewport-grid" />;
}

// ─── Orientation Gizmo ─────────────────────────────────────────────────────
// Shows the current XYZ axes orientation in the top-right corner.
// Click on an axis label to snap the view to that axis.

interface OrientationGizmoProps {
  onSnapToAxis?: (axis: 'front' | 'back' | 'left' | 'right' | 'top' | 'bottom') => void;
}

export function OrientationGizmo({ onSnapToAxis }: OrientationGizmoProps) {
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

  return (
    <div className="orientation-gizmo">
      <svg viewBox="-1.2 -1.2 2.4 2.4" width="80" height="80">
        {/* X axis: Red */}
        <line x1="0" y1="0" x2="1" y2="0" stroke="#FF4444" strokeWidth="0.08" />
        <polygon points="1,0 0.85,-0.08 0.85,0.08" fill="#FF4444" cursor="pointer"
          onClick={() => handleClick('x')} />
        <text x="0.9" y="-0.12" fill="#FF4444" fontSize="0.15" fontWeight="bold" textAnchor="middle"
          cursor="pointer" onClick={() => handleClick('x')}>X</text>

        {/* Y axis: Green */}
        <line x1="0" y1="0" x2="0" y2="1" stroke="#44CC44" strokeWidth="0.08" />
        <polygon points="0,1 -0.08,0.85 0.08,0.85" fill="#44CC44" cursor="pointer"
          onClick={() => handleClick('y')} />
        <text x="-0.12" y="0.9" fill="#44CC44" fontSize="0.15" fontWeight="bold" textAnchor="middle"
          cursor="pointer" onClick={() => handleClick('y')}>Y</text>

        {/* Z axis: Blue */}
        <line x1="0" y1="0" x2="-0.7" y2="0.7" stroke="#4488FF" strokeWidth="0.08" />
        <polygon points="-0.7,0.7 -0.6,0.58 -0.55,0.68" fill="#4488FF" cursor="pointer"
          onClick={() => handleClick('z')} />
        <text x="-0.78" y="0.82" fill="#4488FF" fontSize="0.15" fontWeight="bold" textAnchor="middle"
          cursor="pointer" onClick={() => handleClick('z')}>Z</text>

        {/* Center sphere */}
        <circle cx="0" cy="0" r="0.15" fill="#555" stroke="#888" strokeWidth="0.02" />
      </svg>
    </div>
  );
}
