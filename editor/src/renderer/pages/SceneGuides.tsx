import React, { useMemo } from 'react';
import {
  Vec3,
  createViewMatrix, createPerspectiveMatrix,
  createOrthographicMatrix,
  projectToScreen, cameraBasisVectors,
} from './gizmoMath';

// ─── Types ──────────────────────────────────────────────────────────────────

export interface GuideEntity {
  id: string;
  position: Vec3;
  rotation: Vec3; // euler or quaternion as [x,y,z] or [x,y,z,w]
  componentType: 'Camera' | 'Light';
  // Camera-specific
  fov?: number;
  // Light-specific
  lightKind?: 'directional' | 'point' | 'spot';
  lightColor?: Vec3;
}

interface CameraState {
  yaw: number;
  pitch: number;
  distance: number;
  targetX: number;
  targetY: number;
  targetZ: number;
}

interface SceneGuidesProps {
  cameraState: CameraState;
  guides: GuideEntity[];
  selectedId: string | null;
  viewportWidth: number;
  viewportHeight: number;
  viewMode: '2d' | '3d';
  onSelect: (id: string) => void;
}

// ─── Constants ──────────────────────────────────────────────────────────────

const ICON_SIZE = 18;
const CAMERA_COLOR = '#8888FF';
const LIGHT_COLOR = '#FFDD44';

// ─── Component ──────────────────────────────────────────────────────────────

export default function SceneGuides({
  cameraState,
  guides,
  selectedId,
  viewportWidth,
  viewportHeight,
  viewMode,
  onSelect,
}: SceneGuidesProps) {
  const { yaw, pitch, distance, targetX, targetY, targetZ } = cameraState;
  const viewMatrix = useMemo(() =>
    createViewMatrix(
      viewMode === '2d' ? 0 : yaw,
      viewMode === '2d' ? 0 : pitch,
      distance, targetX, targetY, targetZ,
    ),
    [yaw, pitch, distance, targetX, targetY, targetZ, viewMode]
  );
  const fovRadians = 60 * Math.PI / 180;

  const vpW = Math.max(viewportWidth, 1);
  const vpH = Math.max(viewportHeight, 1);
  const aspect = vpW / vpH;
  const projMatrix = useMemo(() =>
    viewMode === '2d'
      ? createOrthographicMatrix(distance * 2, aspect, 0.01, 1000)
      : createPerspectiveMatrix(fovRadians, aspect, 0.01, 1000),
    [vpW, vpH, viewMode, distance]
  );
  const fovRadians = 60 * Math.PI / 180;

  const vpW = Math.max(viewportWidth, 1);
  const vpH = Math.max(viewportHeight, 1);
  const projMatrix = useMemo(() =>
    createPerspectiveMatrix(fovRadians, vpW / vpH, 0.01, 1000),
    [vpW, vpH]
  );

  const projected = useMemo(() => {
    return guides.map(guide => {
      const screen = projectToScreen(guide.position, viewMatrix, projMatrix, vpW, vpH);
      if (!screen) return null;
      return { ...guide, screen };
    }).filter(Boolean) as (GuideEntity & { screen: { x: number; y: number; depth: number } })[];
  }, [guides, viewMatrix, projMatrix]);

  if (projected.length === 0) return null;

  return (
    <svg
      className="scene-guides"
      style={{
        position: 'absolute',
        top: 0,
        left: 0,
        width: '100%',
        height: '100%',
        pointerEvents: 'none',
        overflow: 'visible',
      }}
      viewBox={`0 0 ${vpW} ${vpH}`}
      preserveAspectRatio="none"
    >
      {projected.map(guide => {
        const { x, y, depth } = guide.screen;
        const isSelected = guide.id === selectedId;
        const alpha = Math.max(0.3, Math.min(1, 1 - (depth + 1) * 0.3)); // fade with distance
        const size = ICON_SIZE * (1 + depth * 0.2); // larger when closer

        if (guide.componentType === 'Camera') {
          return (
            <g
              key={guide.id}
              transform={`translate(${x}, ${y})`}
              opacity={alpha}
              style={{ pointerEvents: 'auto', cursor: 'pointer' }}
              onClick={(e) => { e.stopPropagation(); onSelect(guide.id); }}
            >
              {/* Camera body */}
              <rect
                x={-size / 2}
                y={-size / 3}
                width={size}
                height={size * 0.65}
                rx={3}
                fill={isSelected ? '#fff' : CAMERA_COLOR}
                stroke={isSelected ? CAMERA_COLOR : 'none'}
                strokeWidth={1.5}
              />
              {/* Lens */}
              <circle
                cx={size / 2}
                cy={0}
                r={size * 0.25}
                fill={isSelected ? CAMERA_COLOR : '#fff'}
              />
              {/* FOV frustum lines */}
              {guide.fov && (
                <>
                  <line x1={size / 2} y1={0}
                    x2={size / 2 + size * 1.5} y2={-size * 0.8}
                    stroke={CAMERA_COLOR} strokeWidth={1} opacity={0.5} />
                  <line x1={size / 2} y1={0}
                    x2={size / 2 + size * 1.5} y2={size * 0.8}
                    stroke={CAMERA_COLOR} strokeWidth={1} opacity={0.5} />
                  <line x1={size / 2 + size * 1.2} y1={-size * 0.6}
                    x2={size / 2 + size * 1.5} y2={size * 0.6}
                    stroke={CAMERA_COLOR} strokeWidth={1} opacity={0.3} />
                  <line x1={size / 2 + size * 1.2} y1={-size * 0.6}
                    x2={size / 2 + size * 1.2} y2={size * 0.6}
                    stroke={CAMERA_COLOR} strokeWidth={0.5} opacity={0.15} />
                </>
              )}
            </g>
          );
        }

        if (guide.componentType === 'Light') {
          const color = guide.lightColor
            ? `rgb(${Math.round(guide.lightColor[0] * 255)}, ${Math.round(guide.lightColor[1] * 255)}, ${Math.round(guide.lightColor[2] * 255)})`
            : LIGHT_COLOR;

          if (guide.lightKind === 'directional') {
            return (
              <g
                key={guide.id}
                transform={`translate(${x}, ${y})`}
                opacity={alpha}
                style={{ pointerEvents: 'auto', cursor: 'pointer' }}
                onClick={(e) => { e.stopPropagation(); onSelect(guide.id); }}
              >
                {/* Sun-like circle with rays */}
                <circle cx={0} cy={0} r={size * 0.4} fill={color} stroke={isSelected ? '#fff' : color} strokeWidth={1.5} />
                {/* Rays */}
                {[0, 45, 90, 135, 180, 225, 270, 315].map(angle => {
                  const rad = angle * Math.PI / 180;
                  return (
                    <line key={angle}
                      x1={Math.cos(rad) * size * 0.5}
                      y1={Math.sin(rad) * size * 0.5}
                      x2={Math.cos(rad) * size * 0.7}
                      y2={Math.sin(rad) * size * 0.7}
                      stroke={color} strokeWidth={1.5} />
                  );
                })}
              </g>
            );
          }

          if (guide.lightKind === 'spot') {
            return (
              <g
                key={guide.id}
                transform={`translate(${x}, ${y})`}
                opacity={alpha}
                style={{ pointerEvents: 'auto', cursor: 'pointer' }}
                onClick={(e) => { e.stopPropagation(); onSelect(guide.id); }}
              >
                {/* Spot light cone */}
                <path
                  d={`M ${-size * 0.4},${-size * 0.3} L ${size * 0.4},${-size * 0.5} L ${size * 0.5},0 L ${size * 0.4},${size * 0.5} L ${-size * 0.4},${size * 0.3} Z`}
                  fill={color}
                  stroke={isSelected ? '#fff' : color}
                  strokeWidth={1.5}
                  opacity={0.7}
                />
                <circle cx={-size * 0.2} cy={0} r={size * 0.25} fill="#fff" opacity={0.8} />
              </g>
            );
          }

          // Default: point light
          return (
            <g
              key={guide.id}
              transform={`translate(${x}, ${y})`}
              opacity={alpha}
              style={{ pointerEvents: 'auto', cursor: 'pointer' }}
              onClick={(e) => { e.stopPropagation(); onSelect(guide.id); }}
            >
              {/* Glowing circle */}
              <circle cx={0} cy={0} r={size * 0.5} fill={color} opacity={0.3} />
              <circle cx={0} cy={0} r={size * 0.3} fill={color} stroke={isSelected ? '#fff' : color} strokeWidth={1.5} />
              <circle cx={0} cy={0} r={size * 0.1} fill="#fff" />
            </g>
          );
        }

        return null;
      })}
    </svg>
  );
}
