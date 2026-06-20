import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  fetchSceneGuides,
  openGameView,
  openNativeSceneView,
  rpc,
  viewportReadback,
} from '../api';
import { useTranslation } from '../i18n';
import AiPanel, { type AiWorkspaceState } from './AiPanel';
import { CloseProjectDialog } from './Dialogs';
import { ViewportGrid, OrientationGizmo } from './ViewportOverlays';
import { type GuideEntity } from './SceneGuides';
import {
  type Vec3,
  createViewMatrix,
  createPerspectiveMatrix,
  createOrthographicMatrix,
  projectToScreen,
} from './gizmoMath';
import {
  IconAlertCircle,
  IconCheck,
  IconCode,
  IconCopy,
  IconFile,
  IconLoader,
  IconPackage,
  IconPlay,
  IconPlus,
  IconProjects,
  IconSparkles,
  IconTrash,
  IconX,
} from '../icons';
import type { QuestEditorArtifact } from '../App';

// ─── Types ──────────────────────────────────────────────────────────────────

interface ShellState {
  has_project: boolean;
  project_name?: string;
  scene_dirty: boolean;
  can_undo: boolean;
  can_redo: boolean;
  scene_version?: number;
}

interface SceneObject {
  id: string;
  name: string;
  tag: string;
  position: [number, number, number];
  parent_id?: string | null;
}

interface EntityDetails {
  id: string;
  name: string;
  tag: string;
  transform: {
    position: [number, number, number];
    rotation: [number, number, number, number];
    scale: [number, number, number];
  };
  components: Array<{
    type: string;
    data: Record<string, unknown>;
  }>;
}

interface EditorConsoleEntry {
  timestamp: number;
  level: string;
  subsystem: string;
  file?: string | null;
  line?: number | null;
  message: string;
}

interface ProjectAssetMeta {
  guid: string;
  source_path: string;
  kind: string;
  importer: string;
}

interface AssetReferenceRow {
  kind: string;
  label: string;
  detail: string;
}

interface Props {
  onCloseProject: () => void;
  onOpenSettings?: () => void;
  onOpenQuest?: () => void;
  onPromoteToQuest?: (prompt: string, context: string) => Promise<void>;
  questArtifact?: QuestEditorArtifact | null;
  onDismissQuestArtifact?: () => void;
}

interface ArtifactSelection {
  kind: 'model' | 'code' | 'document';
  label: string;
  context: string;
  x: number;
  y: number;
}

type WorkspaceView = 'prd' | 'tasks' | 'game' | 'assets' | 'scripts' | 'build' | 'diagnostics';
type ProjectAssetCreateKind = 'script' | 'material' | 'prefab' | 'scene';
type BuildTarget = 'windows-x64' | 'linux-x64' | 'macos-universal' | 'android-arm64' | 'ios-universal' | 'embedded-linux';
type BuildFormat = 'folder' | 'exe' | 'msi' | 'nsis' | 'appimage' | 'deb' | 'rpm' | 'dmg' | 'apk' | 'aab' | 'ipa' | 'ipk';

interface BuildTargetOption {
  id: BuildTarget;
  label: string;
  formats: BuildFormat[];
  status: 'ready' | 'planned' | 'blocked';
  note: string;
}

interface BuildPreset {
  id: string;
  label: string;
  target: BuildTarget;
  format: BuildFormat;
  channel: 'debug' | 'release';
}

interface BuildPackageResult {
  project: string;
  target: string;
  format: string;
  channel: string;
  path: string;
  binary: string;
  launcher: string;
}

const BUILD_TARGETS: BuildTargetOption[] = [
  {
    id: 'linux-x64',
    label: 'Linux x64',
    formats: ['folder', 'appimage', 'deb', 'rpm'],
    status: 'planned',
    note: 'Requires game export packaging through xtask; editor bundle packaging already exists through Tauri.',
  },
  {
    id: 'windows-x64',
    label: 'Windows x64',
    formats: ['folder', 'exe', 'msi', 'nsis'],
    status: 'planned',
    note: 'Needs Windows runner or cross-build support before installers can be produced from this host.',
  },
  {
    id: 'macos-universal',
    label: 'macOS Universal',
    formats: ['folder', 'dmg'],
    status: 'planned',
    note: 'Requires macOS signing/notarization flow for distributable builds.',
  },
  {
    id: 'android-arm64',
    label: 'Android ARM64',
    formats: ['apk', 'aab'],
    status: 'blocked',
    note: 'Needs Android runtime adaptation, SDK/NDK detection, signing, and asset packaging.',
  },
  {
    id: 'ios-universal',
    label: 'iOS Universal',
    formats: ['ipa'],
    status: 'blocked',
    note: 'Requires Apple toolchain, provisioning, signing, and mobile runtime support.',
  },
  {
    id: 'embedded-linux',
    label: 'Embedded Linux',
    formats: ['ipk', 'folder'],
    status: 'blocked',
    note: 'Requires target device profile, architecture, libc, install paths, and control metadata.',
  },
];

const BUILD_PRESETS: BuildPreset[] = [
  { id: 'local-folder', label: 'Local Folder', target: 'linux-x64', format: 'folder', channel: 'debug' },
  { id: 'linux-appimage', label: 'Linux AppImage', target: 'linux-x64', format: 'appimage', channel: 'release' },
  { id: 'windows-installer', label: 'Windows Installer', target: 'windows-x64', format: 'nsis', channel: 'release' },
  { id: 'android-apk', label: 'Android APK', target: 'android-arm64', format: 'apk', channel: 'release' },
];

interface QuestArtifactContext {
  surface: WorkspaceView;
  title: string;
  description: string;
  focusPath?: string;
}

function formatInspectorValue(value: unknown): string {
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  if (Array.isArray(value)) return value.map(item => formatInspectorValue(item)).join(', ');
  if (value === null || value === undefined) return '';
  return JSON.stringify(value, null, 2);
}

function parseInspectorValue(raw: string, current: unknown): unknown {
  if (typeof current === 'number') {
    const next = Number(raw);
    return Number.isFinite(next) ? next : current;
  }
  if (typeof current === 'boolean') {
    return raw === 'true';
  }
  if (Array.isArray(current)) {
    const parts = raw.split(',').map(part => part.trim());
    if (current.every(item => typeof item === 'number')) {
      const parsed = parts.map(Number);
      return parsed.length === current.length && parsed.every(Number.isFinite) ? parsed : current;
    }
    return parts;
  }
  if (current && typeof current === 'object') {
    try {
      return JSON.parse(raw);
    } catch {
      return current;
    }
  }
  return raw;
}

function ComponentFieldEditor({ fieldName, value, onCommit }: {
  fieldName: string;
  value: unknown;
  onCommit: (fieldName: string, value: unknown) => Promise<void>;
}) {
  const [draft, setDraft] = useState(() => formatInspectorValue(value));

  useEffect(() => {
    setDraft(formatInspectorValue(value));
  }, [value]);

  const commit = useCallback(async () => {
    const next = parseInspectorValue(draft, value);
    if (JSON.stringify(next) === JSON.stringify(value)) {
      setDraft(formatInspectorValue(value));
      return;
    }
    await onCommit(fieldName, next);
  }, [draft, fieldName, onCommit, value]);

  if (typeof value === 'boolean') {
    return (
      <label className="inspector-field inspector-field-row">
        <span>{fieldName}</span>
        <input
          type="checkbox"
          checked={value}
          onChange={event => onCommit(fieldName, event.currentTarget.checked)}
        />
      </label>
    );
  }

  const isObject = Boolean(value && typeof value === 'object' && !Array.isArray(value));

  return (
    <label className="inspector-field">
      <span>{fieldName}</span>
      {isObject ? (
        <textarea
          className="inspector-field-input inspector-field-json"
          value={draft}
          rows={4}
          onChange={event => setDraft(event.currentTarget.value)}
          onBlur={commit}
          onKeyDown={event => {
            if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') event.currentTarget.blur();
            if (event.key === 'Escape') {
              setDraft(formatInspectorValue(value));
              event.currentTarget.blur();
            }
          }}
        />
      ) : (
        <input
          className="inspector-field-input"
          value={draft}
          inputMode={typeof value === 'number' || (Array.isArray(value) && value.every(item => typeof item === 'number')) ? 'decimal' : undefined}
          onChange={event => setDraft(event.currentTarget.value)}
          onBlur={commit}
          onKeyDown={event => {
            if (event.key === 'Enter') event.currentTarget.blur();
            if (event.key === 'Escape') {
              setDraft(formatInspectorValue(value));
              event.currentTarget.blur();
            }
          }}
        />
      )}
    </label>
  );
}

function isScriptPath(path?: string): boolean {
  return Boolean(path && /\.(rhai|js|ts|tsx|lua|rs)$/i.test(path));
}

function questArtifactContext(artifact: QuestEditorArtifact): QuestArtifactContext {
  const path = artifact.path;
  if (artifact.kind === 'spec' || artifact.kind === 'intent') {
    return {
      surface: 'prd',
      title: artifact.kind === 'spec' ? 'Quest specification' : 'Quest intent',
      description: 'Opened as a planning document for review or manual refinement.',
      focusPath: path,
    };
  }
  if (artifact.kind === 'changed_file' && isScriptPath(path)) {
    return {
      surface: 'scripts',
      title: 'Quest script change',
      description: 'Opened in the script inspector for code review and local correction.',
      focusPath: path,
    };
  }
  if (artifact.kind === 'changed_file' && /\.(scene|scn|prefab|level|ron|json)$/i.test(path ?? '')) {
    return {
      surface: 'game',
      title: 'Quest scene or asset change',
      description: 'Opened in the game inspection surface for hierarchy, viewport, and inspector review.',
      focusPath: path,
    };
  }
  if (artifact.kind === 'validation') {
    return {
      surface: 'tasks',
      title: 'Quest validation evidence',
      description: 'Review the validator result and use Editor diagnostics or AI follow-up for local investigation.',
      focusPath: path,
    };
  }
  if (artifact.kind === 'review_finding') {
    return {
      surface: 'tasks',
      title: 'Quest review finding',
      description: 'Review the unresolved issue before deciding whether to fix locally, continue the Quest, or revise.',
      focusPath: path,
    };
  }
  if (artifact.kind === 'checkpoint') {
    return {
      surface: 'tasks',
      title: 'Quest checkpoint',
      description: 'Inspect the recoverability checkpoint and workspace evidence before resuming or applying work.',
      focusPath: path,
    };
  }
  if (artifact.kind === 'trace') {
    return {
      surface: 'tasks',
      title: 'Quest timeline trace',
      description: 'Inspect execution history, decisions, validations, and review events in task context.',
      focusPath: path,
    };
  }
  return {
    surface: 'tasks',
    title: 'Quest artifact',
    description: 'Opened in the task surface for inspection and AI-assisted follow-up.',
    focusPath: path,
  };
}

function WorkspaceInspector({ object, onFocus, onPositionChange }: {
  object: SceneObject;
  onFocus: () => void;
  onPositionChange: (position: [number, number, number]) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [position, setPosition] = useState<[string, string, string]>(() => (
    object.position.map(value => value.toFixed(2)) as [string, string, string]
  ));

  useEffect(() => {
    setPosition(object.position.map(value => value.toFixed(2)) as [string, string, string]);
  }, [object.id, object.position]);

  const commitPosition = useCallback(async () => {
    const next = position.map(Number) as [number, number, number];
    if (next.some(value => !Number.isFinite(value))) {
      setPosition(object.position.map(value => value.toFixed(2)) as [string, string, string]);
      return;
    }
    await onPositionChange(next);
  }, [object.position, onPositionChange, position]);

  return (
    <div className="workspace-selection-card">
      <div className="workspace-selection-title">
        <div>
          <strong>{object.name}</strong>
          <span>{object.tag || t('entity_untagged')}</span>
        </div>
        <span className="workspace-live-badge">{t('badge_live')}</span>
      </div>
      <label className="workspace-property-label">{t('prop_position')}</label>
      <div className="workspace-position workspace-position-editable">
        {position.map((value, index) => (
          <label key={index}>
            <span>{['X', 'Y', 'Z'][index]}</span>
            <input
              value={value}
              inputMode="decimal"
              aria-label={`${['X', 'Y', 'Z'][index]} position`}
              onChange={event => setPosition(current => {
                const next = [...current] as [string, string, string];
                next[index] = event.target.value;
                return next;
              })}
              onBlur={commitPosition}
              onKeyDown={event => {
                if (event.key === 'Enter') event.currentTarget.blur();
                if (event.key === 'Escape') {
                  setPosition(object.position.map(item => item.toFixed(2)) as [string, string, string]);
                  event.currentTarget.blur();
                }
              }}
            />
          </label>
        ))}
      </div>
      <button onClick={onFocus}>{t('editor_focus_viewport')}</button>
    </div>
  );
}

// ─── Resize Handle Hook ─────────────────────────────────────────────────────

function useDragHandle(
  axis: 'horizontal',
  onDelta: (delta: number) => void,
) {
  const dragging = useRef(false);
  const startPos = useRef(0);

  const onMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      dragging.current = true;
      startPos.current = e.clientX;

      const onMouseMove = (ev: MouseEvent) => {
        if (!dragging.current) return;
        const current = ev.clientX;
        onDelta(current - startPos.current);
        startPos.current = current;
      };

      const onMouseUp = () => {
        dragging.current = false;
        window.removeEventListener('mousemove', onMouseMove);
        window.removeEventListener('mouseup', onMouseUp);
      };

      window.addEventListener('mousemove', onMouseMove);
      window.addEventListener('mouseup', onMouseUp);
    },
    [onDelta],
  );

  return onMouseDown;
}

// ─── Viewport ────────────────────────────────────────────────────────────────

function ViewportCanvas({ sceneVersion = 0, cameraRef, onCameraChange, viewMode, playMode, editorCamera }: {
  sceneVersion?: number;
  cameraRef?: React.MutableRefObject<{
    yaw: number; pitch: number; distance: number;
    targetX: number; targetY: number; targetZ: number;
  }>;
  onCameraChange?: () => void;
  viewMode: '2d' | '3d';
  playMode?: boolean;
  editorCamera?: boolean;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const contextRef = useRef<CanvasRenderingContext2D | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const sizeRef = useRef({ width: 640, height: 480 });
  const isActiveRef = useRef(true);
  const versionRef = useRef(sceneVersion);
  const lastRenderedVersionRef = useRef<number | null>(null);
  const fastPreviewUntilRef = useRef(0);
  const internalCameraRef = useRef({
    yaw: -0.5, pitch: 0.3, distance: 6,
    targetX: 0, targetY: 1, targetZ: 0,
  });
  const camRef = cameraRef ?? internalCameraRef;
  const dragging = useRef<'orbit' | 'pan' | null>(null);
  const dragStart = useRef({
    x: 0, y: 0, yaw: 0, pitch: 0, targetX: 0, targetY: 0, targetZ: 0,
  });

  versionRef.current = sceneVersion;

  // Poll for frames via binary IPC
  useEffect(() => {
    isActiveRef.current = true;
    lastRenderedVersionRef.current = null;
    const poll = async () => {
      if (!isActiveRef.current) return;
      const { width, height } = sizeRef.current;
      const cam = camRef.current;
      try {
        const buffer = await viewportReadback({
          width, height,
          lastVersion: lastRenderedVersionRef.current ?? undefined,
          yaw: cam.yaw, pitch: cam.pitch, distance: cam.distance,
          targetX: cam.targetX, targetY: cam.targetY, targetZ: cam.targetZ,
          viewMode,
          playMode,
          editorCamera,
        });
        if (!isActiveRef.current || !canvasRef.current) return;
        const uint8 = new Uint8Array(buffer);
        const header = new Uint32Array(uint8.buffer, uint8.byteOffset, 2);
        const w = header[0];
        const h = header[1];
        if (w > 0 && h > 0) {
          lastRenderedVersionRef.current = versionRef.current;
          const canvas = canvasRef.current;
          if (canvas.width !== w || canvas.height !== h) {
            canvas.width = w;
            canvas.height = h;
            contextRef.current = null;
          }
          const ctx = contextRef.current ?? canvas.getContext('2d');
          contextRef.current = ctx;
          if (ctx) {
            const pixelOffset = uint8.byteOffset + 8;
            const pixelBytes = w * h * 4;
            const imageData = new ImageData(
              new Uint8ClampedArray(uint8.buffer, pixelOffset, pixelBytes),
              w, h,
            );
            ctx.putImageData(imageData, 0, 0);
          }
        }
      } catch (e) {
        console.error('[viewport] readback error:', e);
      }
      // GPU readback is synchronous on the backend and copies the full RGBA
      // frame through IPC. Refresh quickly only while the camera is actively
      // moving; keep idle scene previews on a low-cost dirty/version poll.
      const previewIsActive = performance.now() < fastPreviewUntilRef.current;
      window.setTimeout(poll, playMode || previewIsActive ? 16 : 100);
    };
    poll();
    return () => { isActiveRef.current = false; };
  }, [viewMode, playMode, editorCamera]);

  // Resize observer
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const initRect = container.getBoundingClientRect();
    sizeRef.current = {
      width: Math.round(initRect.width) || 640,
      height: Math.round(initRect.height) || 480,
    };
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width, height } = entry.contentRect;
        if (width > 0 && height > 0) {
          sizeRef.current = { width: Math.round(width), height: Math.round(height) };
          lastRenderedVersionRef.current = null;
          const canvas = canvasRef.current;
          if (canvas) {
            canvas.width = Math.round(width);
            canvas.height = Math.round(height);
            contextRef.current = null;
          }
        }
      }
    });
    observer.observe(container);
    return () => observer.disconnect();
  }, []);

  // Mouse handlers for orbit/pan
  const onMouseDown = useCallback((e: React.MouseEvent) => {
    if (e.button === 2) {
      dragging.current = viewMode === '2d' ? 'pan' : 'orbit';
      dragStart.current = { x: e.clientX, y: e.clientY, ...camRef.current };
      e.preventDefault();
    } else if (e.button === 1) {
      dragging.current = 'pan';
      dragStart.current = { x: e.clientX, y: e.clientY, ...camRef.current };
      e.preventDefault();
    }
  }, [viewMode]);

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (!dragging.current) return;
      const dpiScale = window.devicePixelRatio || 1;
      const dx = (e.clientX - dragStart.current.x) / dpiScale;
      const dy = (e.clientY - dragStart.current.y) / dpiScale;
      if (dragging.current === 'orbit') {
        camRef.current.yaw = dragStart.current.yaw - dx * 0.005;
        camRef.current.pitch = Math.max(-1.5, Math.min(1.5, dragStart.current.pitch + dy * 0.005));
      } else if (dragging.current === 'pan') {
        const d = camRef.current.distance * 0.002;
        const yaw = camRef.current.yaw;
        camRef.current.targetX = dragStart.current.targetX + (-dx * Math.cos(yaw) - dy * Math.sin(yaw) * 0.5) * d;
        camRef.current.targetY = dragStart.current.targetY + dy * d * 0.5;
        camRef.current.targetZ = dragStart.current.targetZ + (dx * Math.sin(yaw) - dy * Math.cos(yaw) * 0.5) * d;
      }
      lastRenderedVersionRef.current = null;
      fastPreviewUntilRef.current = performance.now() + 160;
      onCameraChange?.();
    };
    const handleMouseUp = () => { dragging.current = null; };
    const handleWheel = (e: WheelEvent) => {
      if (containerRef.current && containerRef.current.contains(e.target as Node)) {
        camRef.current.distance = Math.max(0.5, Math.min(100, camRef.current.distance + e.deltaY * 0.01));
        lastRenderedVersionRef.current = null;
        fastPreviewUntilRef.current = performance.now() + 160;
        onCameraChange?.();
        e.preventDefault();
      }
    };
    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
    window.addEventListener('wheel', handleWheel, { passive: false });
    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);
      window.removeEventListener('wheel', handleWheel);
    };
  }, []);

  return (
    <div
      ref={containerRef}
      className="viewport-container"
      onMouseDown={onMouseDown}
      onContextMenu={(e) => e.preventDefault()}
    >
      <canvas ref={canvasRef} className="viewport-canvas" />
      <ViewportGrid show={true} />
      {viewMode === '3d' && <OrientationGizmo camera={camRef.current} onSnapToAxis={(axis) => {
        switch (axis) {
          case 'top':    camRef.current.pitch = 1.5;  camRef.current.yaw = 0;     break;
          case 'bottom': camRef.current.pitch = -1.5; camRef.current.yaw = 0;     break;
          case 'left':   camRef.current.pitch = 0;    camRef.current.yaw = 1.5;   break;
          case 'right':  camRef.current.pitch = 0;    camRef.current.yaw = -1.5;  break;
          case 'front':  camRef.current.pitch = 0;    camRef.current.yaw = 0;     break;
          case 'back':   camRef.current.pitch = 0;    camRef.current.yaw = 3.14;  break;
        }
        lastRenderedVersionRef.current = null;
        fastPreviewUntilRef.current = performance.now() + 160;
        onCameraChange?.();
      }} />}
    </div>
  );
}
// ─── Click-to-Pick Utility ──────────────────────────────────────────────────

const PICK_RADIUS_PX = 30;
const VIEWPORT_FOV_DEG = 60;

/**
 * Given a click position in the viewport, find the closest scene object.
 * Returns the entity ID or null if nothing is close enough.
 */
function pickEntityAtScreen(
  clickX: number,
  clickY: number,
  vpWidth: number,
  vpHeight: number,
  sceneTree: SceneObject[],
  camera: { yaw: number; pitch: number; distance: number; targetX: number; targetY: number; targetZ: number },
  viewMode: '2d' | '3d',
): string | null {
  if (sceneTree.length === 0 || vpWidth <= 0 || vpHeight <= 0) return null;

  const viewMatrix = createViewMatrix(
    viewMode === '2d' ? 0 : camera.yaw,
    viewMode === '2d' ? 0 : camera.pitch,
    camera.distance,
    camera.targetX, camera.targetY, camera.targetZ,
  );
  const fovRad = VIEWPORT_FOV_DEG * Math.PI / 180;
  const projMatrix = viewMode === '2d'
    ? createOrthographicMatrix(camera.distance * 2, vpWidth / vpHeight, 0.01, 1000)
    : createPerspectiveMatrix(fovRad, vpWidth / vpHeight, 0.1, 1000);

  let bestId: string | null = null;
  let bestDist = PICK_RADIUS_PX;
  let bestDepth = Infinity;

  for (const obj of sceneTree) {
    const screen = projectToScreen(obj.position, viewMatrix, projMatrix, vpWidth, vpHeight);
    if (!screen) continue;

    const dx = screen.x - clickX;
    const dy = screen.y - clickY;
    const dist = Math.sqrt(dx * dx + dy * dy);

    if (dist < bestDist || (dist === bestDist && screen.depth < bestDepth)) {
      bestDist = dist;
      bestDepth = screen.depth;
      bestId = obj.id;
    }
  }

  return bestId;
}

// ─── Selection Overlay ──────────────────────────────────────────────────────

function SelectionOverlay({ sceneTree, selectedId, camera, width, height, viewMode }: {
  sceneTree: SceneObject[];
  selectedId: string | null;
  camera: { yaw: number; pitch: number; distance: number; targetX: number; targetY: number; targetZ: number };
  width: number;
  height: number;
  viewMode: '2d' | '3d';
}) {
  const selected = selectedId ? sceneTree.find(o => o.id === selectedId) : null;

  const screenPos = useMemo(() => {
    if (!selected) return null;
    const viewMatrix = createViewMatrix(
      viewMode === '2d' ? 0 : camera.yaw,
      viewMode === '2d' ? 0 : camera.pitch,
      camera.distance,
      camera.targetX, camera.targetY, camera.targetZ,
    );
    const fovRad = VIEWPORT_FOV_DEG * Math.PI / 180;
    const aspect = width / Math.max(height, 1);
    const projMatrix = viewMode === '2d'
      ? createOrthographicMatrix(camera.distance * 2, aspect, 0.01, 1000)
      : createPerspectiveMatrix(fovRad, aspect, 0.01, 1000);
    return projectToScreen(selected.position, viewMatrix, projMatrix, width, height);
  }, [selected, camera.yaw, camera.pitch, camera.distance, camera.targetX, camera.targetY, camera.targetZ, width, height, viewMode]);

  if (!selected || !screenPos) return null;

  return (
    <svg
      className="selection-overlay"
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      style={{ position: 'absolute', top: 0, left: 0, width: '100%', height: '100%', pointerEvents: 'none', zIndex: 20 }}
    >
      {/* Selection ring */}
      <circle
        cx={screenPos.x}
        cy={screenPos.y}
        r={18}
        fill="none"
        stroke="var(--accent, #60A5FA)"
        strokeWidth={2}
        strokeDasharray="4 3"
        opacity={0.9}
      />
      {/* Entity name label */}
      <rect
        x={screenPos.x + 22}
        y={screenPos.y - 10}
        width={Math.max(60, selected.name.length * 7 + 16)}
        height={20}
        rx={4}
        fill="rgba(37, 99, 235, 0.85)"
      />
      <text
        x={screenPos.x + 30}
        y={screenPos.y + 4}
        fill="white"
        fontSize={11}
        fontFamily="var(--font-sans)"
        fontWeight={500}
      >
        {selected.name}
      </text>
    </svg>
  );
}

// ─── Editor Page (AI-First Layout) ──────────────────────────────────────────

export default function EditorPage({
  onCloseProject,
  onOpenSettings,
  onOpenQuest,
  onPromoteToQuest,
  questArtifact,
  onDismissQuestArtifact,
}: Props) {
  const { t } = useTranslation();

  // State
  const [shellState, setShellState] = useState<ShellState | null>(null);
  const [sceneTree, setSceneTree] = useState<SceneObject[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [sceneVersion, setSceneVersion] = useState(0);
  const [showCloseDialog, setShowCloseDialog] = useState(false);
  const [aiPanelWidth, setAiPanelWidth] = useState(() => {
    const saved = Number(window.localStorage.getItem('aster.aiPanelWidth'));
    return Number.isFinite(saved) && saved >= 320 && saved <= 560 ? saved : 380;
  });
  const [workspaceView, setWorkspaceView] = useState<WorkspaceView>('game');
  const [aiWorkspace, setAiWorkspace] = useState<AiWorkspaceState | null>(null);
  const [scripts, setScripts] = useState<string[]>([]);
  const [assets, setAssets] = useState<ProjectAssetMeta[]>([]);
  const [assetsBusy, setAssetsBusy] = useState(false);
  const [selectedScript, setSelectedScript] = useState<string | null>(null);
  const [openedQuestArtifact, setOpenedQuestArtifact] = useState<QuestArtifactContext | null>(null);
  const [selectedEntityDetails, setSelectedEntityDetails] = useState<EntityDetails | null>(null);
  const [selectedEntityNameDraft, setSelectedEntityNameDraft] = useState('');
  const [addComponentType, setAddComponentType] = useState('Camera');
  const [scriptContent, setScriptContent] = useState('');
  const [scriptSavedContent, setScriptSavedContent] = useState('');
  const [scriptSaving, setScriptSaving] = useState(false);
  const [scriptLineRange, setScriptLineRange] = useState<[number, number] | null>(null);
  const [consoleEntries, setConsoleEntries] = useState<EditorConsoleEntry[]>([]);
  const [consoleBusy, setConsoleBusy] = useState(false);
  const [buildTarget, setBuildTarget] = useState<BuildTarget>('linux-x64');
  const [buildFormat, setBuildFormat] = useState<BuildFormat>('folder');
  const [buildChannel, setBuildChannel] = useState<'debug' | 'release'>('debug');
  const [buildOptimizeAssets, setBuildOptimizeAssets] = useState(true);
  const [buildIncludeDebugSymbols, setBuildIncludeDebugSymbols] = useState(false);
  const [buildBusy, setBuildBusy] = useState(false);
  const [buildMessage, setBuildMessage] = useState<string | null>(null);
  const [artifactSelection, setArtifactSelection] = useState<ArtifactSelection | null>(null);
  const [artifactQuestionOpen, setArtifactQuestionOpen] = useState(false);
  const [artifactQuestion, setArtifactQuestion] = useState('');
  const [contextualRequest, setContextualRequest] = useState<{ id: number; prompt: string } | null>(null);
  const [promotingQuest, setPromotingQuest] = useState(false);
  const [promoteError, setPromoteError] = useState<string | null>(null);
  const [guides, setGuides] = useState<GuideEntity[]>([]);
  const [viewMode, setViewMode] = useState<'2d' | '3d'>('3d');
  const [viewportSize, setViewportSize] = useState({ width: 640, height: 480 });
  const [, setCameraRevision] = useState(0);
  const cameraRevisionFrameRef = useRef<number | null>(null);
  const prevSceneVersionRef = useRef(0);
  const cameraRef = useRef({
    yaw: -0.5, pitch: 0.3, distance: 6,
    targetX: 0, targetY: 1, targetZ: 0,
  });
  const validParentOptions = useMemo(() => {
    if (!selectedId) return sceneTree;
    const descendants = new Set<string>();
    let changed = true;
    while (changed) {
      changed = false;
      for (const object of sceneTree) {
        if (
          object.parent_id
          && (object.parent_id === selectedId || descendants.has(object.parent_id))
          && !descendants.has(object.id)
        ) {
          descendants.add(object.id);
          changed = true;
        }
      }
    }
    return sceneTree.filter(object => object.id !== selectedId && !descendants.has(object.id));
  }, [sceneTree, selectedId]);

  // Gizmo state
  const [activeTool] = useState<'view' | 'move' | 'rotate' | 'scale'>('move');
  const [transformSpace] = useState<'global' | 'local'>('global');
  const [selectedPosition, setSelectedPosition] = useState<Vec3 | null>(null);
  const viewportMainRef = useRef<HTMLElement>(null);

  useEffect(() => {
    const viewport = viewportMainRef.current;
    if (!viewport) return;

    const updateSize = () => {
      const rect = viewport.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) {
        const width = Math.round(rect.width);
        const height = Math.round(rect.height);
        setViewportSize(current => {
          if (current.width === width && current.height === height) return current;
          return { width, height };
        });
      }
    };

    updateSize();
    const observer = new ResizeObserver(updateSize);
    observer.observe(viewport);
    return () => observer.disconnect();
  }, []);

  const handleCameraChange = useCallback(() => {
    if (cameraRevisionFrameRef.current !== null) return;
    cameraRevisionFrameRef.current = window.requestAnimationFrame(() => {
      cameraRevisionFrameRef.current = null;
      setCameraRevision(revision => revision + 1);
    });
  }, []);

  useEffect(() => () => {
    if (cameraRevisionFrameRef.current !== null) {
      window.cancelAnimationFrame(cameraRevisionFrameRef.current);
    }
  }, []);

  useEffect(() => {
    window.localStorage.setItem('aster.aiPanelWidth', String(aiPanelWidth));
  }, [aiPanelWidth]);

  // Periodic state poll
  useEffect(() => {
    const poll = async () => {
      try {
        const state = await rpc<ShellState>('shell/get_state');
        setShellState(state);
        const newVer = state.scene_version ?? 0;
        if (newVer !== prevSceneVersionRef.current) {
          prevSceneVersionRef.current = newVer;
          setSceneVersion(newVer);
          const { objects } = await rpc<{ objects: SceneObject[] }>('shell/get_scene_tree');
          setSceneTree(objects);
        }
      } catch { /* not ready */ }
    };
    poll();
    const interval = setInterval(poll, 2000);
    return () => clearInterval(interval);
  }, []);

  const refreshProjectAssets = useCallback(async () => {
    setAssetsBusy(true);
    try {
      const result = await rpc<{ entries: Array<{ path: string; kind: string }>; assets: ProjectAssetMeta[] }>('project/list_assets');
      setAssets(result.assets);
      const paths = result.entries
        .filter(entry => /script/i.test(entry.kind) || /\.(rhai|js|ts|lua)$/i.test(entry.path))
        .map(entry => entry.path);
      setScripts(paths);
      setSelectedScript(current => current && paths.includes(current) ? current : paths[0] ?? null);
    } catch {
      setAssets([]);
      setScripts([]);
    } finally {
      setAssetsBusy(false);
    }
  }, []);

  useEffect(() => {
    if (!shellState?.has_project) return;
    refreshProjectAssets();
  }, [refreshProjectAssets, sceneVersion, shellState?.has_project]);

  useEffect(() => {
    if (!selectedScript) {
      setScriptContent('');
      return;
    }
    rpc<{ content: string }>('project/read_file', { path: selectedScript })
      .then(result => {
        setScriptContent(result.content);
        setScriptSavedContent(result.content);
      })
      .catch(() => {
        const fallback = '// Unable to load this script.';
        setScriptContent(fallback);
        setScriptSavedContent(fallback);
      });
  }, [selectedScript]);

  useEffect(() => {
    setScriptLineRange(null);
    setArtifactSelection(null);
    setArtifactQuestionOpen(false);
  }, [selectedScript, workspaceView]);

  useEffect(() => {
    if (!questArtifact) {
      setOpenedQuestArtifact(null);
      return;
    }
    const context = questArtifactContext(questArtifact);
    setOpenedQuestArtifact(context);
    setWorkspaceView(context.surface);
    if (context.surface === 'scripts' && context.focusPath) {
      setSelectedScript(context.focusPath);
    }
  }, [questArtifact]);

  // Track selected position
  useEffect(() => {
    if (selectedId) {
      const obj = sceneTree.find(o => o.id === selectedId);
      setSelectedPosition(obj ? ([...obj.position] as Vec3) : null);
    } else {
      setSelectedPosition(null);
    }
  }, [selectedId, sceneTree]);

  useEffect(() => {
    if (!selectedId) {
      setSelectedEntityDetails(null);
      setSelectedEntityNameDraft('');
      return;
    }
    rpc<EntityDetails>('shell/get_entity', { id: selectedId })
      .then(entity => {
        setSelectedEntityDetails(entity);
        setSelectedEntityNameDraft(entity.name);
      })
      .catch(() => {
        setSelectedEntityDetails(null);
        setSelectedEntityNameDraft('');
      });
  }, [selectedId, sceneVersion]);

  // Fetch scene guides
  useEffect(() => {
    if (!shellState?.has_project) return;
    fetchSceneGuides()
      .then(res => setGuides(res.guides ?? []))
      .catch(() => setGuides([]));
  }, [sceneVersion, shellState?.has_project]);

  // Scene tree refresh — returns the new scene objects list
  const refreshSceneTree = useCallback(async (): Promise<SceneObject[]> => {
    try {
      const state = await rpc<ShellState>('shell/get_state');
      setShellState(state);
      const newVer = state.scene_version ?? 0;
      prevSceneVersionRef.current = newVer;
      setSceneVersion(newVer);
      const { objects } = await rpc<{ objects: SceneObject[] }>('shell/get_scene_tree');
      setSceneTree(objects);
      return objects;
    } catch { /* ignore */ }
    return [];
  }, []);

  const refreshConsoleEntries = useCallback(async () => {
    setConsoleBusy(true);
    try {
      const result = await rpc<{ entries: EditorConsoleEntry[] }>('console/get_entries');
      setConsoleEntries(result.entries);
    } finally {
      setConsoleBusy(false);
    }
  }, []);

  const clearConsoleEntries = useCallback(async () => {
    setConsoleBusy(true);
    try {
      await rpc('console/clear');
      setConsoleEntries([]);
    } finally {
      setConsoleBusy(false);
    }
  }, []);

  const selectedBuildTarget = useMemo(
    () => BUILD_TARGETS.find(target => target.id === buildTarget) ?? BUILD_TARGETS[0],
    [buildTarget],
  );

  useEffect(() => {
    if (!selectedBuildTarget.formats.includes(buildFormat)) {
      setBuildFormat(selectedBuildTarget.formats[0]);
    }
  }, [buildFormat, selectedBuildTarget]);

  const applyBuildPreset = useCallback((preset: BuildPreset) => {
    setBuildTarget(preset.target);
    setBuildFormat(preset.format);
    setBuildChannel(preset.channel);
  }, []);

  const requestBuildPackage = useCallback(async () => {
    setBuildBusy(true);
    setBuildMessage(null);
    try {
      const result = await rpc<BuildPackageResult>('project/package', {
        target: buildTarget,
        format: buildFormat,
        channel: buildChannel,
        optimize_assets: buildOptimizeAssets,
        include_debug_symbols: buildIncludeDebugSymbols,
      });
      const message = [
        `Packaged ${result.project}.`,
        `Target: ${result.target}`,
        `Format: ${result.format}`,
        `Channel: ${result.channel}`,
        `Output: ${result.path}`,
        `Binary: ${result.binary}`,
        `Launcher: ${result.launcher}`,
      ].join('\n');
      setBuildMessage(message);
      await refreshConsoleEntries().catch(() => {});
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setBuildMessage(`Package failed.\n${message}`);
      await rpc('console/push_entry', {
        level: 'error',
        subsystem: 'build',
        message: `Package failed for ${selectedBuildTarget.label}/${buildFormat}: ${message}`,
      }).catch(() => {});
      await refreshConsoleEntries().catch(() => {});
    } finally {
      setBuildBusy(false);
    }
  }, [
    buildChannel,
    buildFormat,
    buildIncludeDebugSymbols,
    buildOptimizeAssets,
    buildTarget,
    refreshConsoleEntries,
    selectedBuildTarget,
  ]);

  const reloadAsset = useCallback(async (path: string) => {
    setAssetsBusy(true);
    try {
      await rpc('project/reimport_asset', { path });
      await refreshProjectAssets();
      await refreshConsoleEntries().catch(() => {});
    } finally {
      setAssetsBusy(false);
    }
  }, [refreshConsoleEntries, refreshProjectAssets]);

  const revealAssetReferences = useCallback(async (asset: ProjectAssetMeta, event: React.MouseEvent) => {
    setAssetsBusy(true);
    try {
      const result = await rpc<{ references: AssetReferenceRow[] }>('project/list_asset_references', {
        path: asset.source_path,
      });
      const references = result.references.length > 0
        ? result.references.map(ref => `${ref.kind}: ${ref.label} - ${ref.detail}`).join('\n')
        : 'No references found.';
      setArtifactSelection({
        kind: 'document',
        label: `${asset.source_path} references`,
        context: `Asset: ${asset.source_path}\nKind: ${asset.kind}\nGUID: ${asset.guid}\n\n${references}`,
        x: event.clientX,
        y: event.clientY,
      });
      setArtifactQuestionOpen(false);
    } finally {
      setAssetsBusy(false);
    }
  }, []);

  const createProjectAsset = useCallback(async (kind: ProjectAssetCreateKind) => {
    const defaultName = `new_${kind}`;
    const rawName = window.prompt(`Create ${kind}`, defaultName);
    const name = rawName?.trim();
    if (!name) return;

    setAssetsBusy(true);
    try {
      const method = kind === 'script' ? 'project/create_script' : `project/create_${kind}`;
      const params = kind === 'script' ? { name, backend: 'rhai' } : { name };
      const result = await rpc<{ path: string }>(method, params);
      await refreshProjectAssets();
      await refreshConsoleEntries().catch(() => {});
      if (kind === 'script') {
        setSelectedScript(result.path);
        setWorkspaceView('scripts');
      }
    } finally {
      setAssetsBusy(false);
    }
  }, [refreshConsoleEntries, refreshProjectAssets]);

  useEffect(() => {
    if (!shellState?.has_project) return;
    refreshConsoleEntries().catch(() => {});
  }, [refreshConsoleEntries, sceneVersion, shellState?.has_project]);

  // Focus camera on a given position with smooth lerp
  const focusOnPosition = useCallback((target: [number, number, number]) => {
    const cam = cameraRef.current;
    const startX = cam.targetX;
    const startY = cam.targetY;
    const startZ = cam.targetZ;
    const [endX, endY, endZ] = target;
    let t = 0;
    const animate = () => {
      t += 0.08;
      if (t >= 1) {
        cam.targetX = endX;
        cam.targetY = endY;
        cam.targetZ = endZ;
        handleCameraChange();
        return;
      }
      const ease = 1 - Math.pow(1 - t, 3); // ease-out cubic
      cam.targetX = startX + (endX - startX) * ease;
      cam.targetY = startY + (endY - startY) * ease;
      cam.targetZ = startZ + (endZ - startZ) * ease;
      handleCameraChange();
      requestAnimationFrame(animate);
    };
    requestAnimationFrame(animate);
  }, [handleCameraChange]);

  // Handle scene changes from AI panel — detect new entities and focus
  const sceneTreeRef = useRef(sceneTree);
  sceneTreeRef.current = sceneTree;

  const handleAiSceneChanged = useCallback(async () => {
    const prevIds = new Set(sceneTreeRef.current.map(o => o.id));
    const newObjects = await refreshSceneTree();
    // Find newly created objects
    const created = newObjects.filter(o => !prevIds.has(o.id));
    if (created.length > 0) {
      // Focus on the first new object and select it
      const first = created[0];
      focusOnPosition(first.position);
      setSelectedId(first.id);
      rpc('shell/select_entity', { id: first.id });
    }
  }, [refreshSceneTree, focusOnPosition]);

  const selectSceneObject = useCallback((id: string | null) => {
    setSelectedId(id);
    rpc('shell/select_entity', id ? { id } : {}).catch(() => {});
    const object = id ? sceneTree.find(item => item.id === id) : null;
    if (object) focusOnPosition(object.position);
  }, [focusOnPosition, sceneTree]);

  const createSceneObject = useCallback(async (name = 'New Object') => {
    const created = await rpc<SceneObject>('shell/create_object', {
      name,
      parent_id: selectedId ?? undefined,
    });
    await refreshSceneTree();
    selectSceneObject(created.id);
  }, [refreshSceneTree, selectSceneObject, selectedId]);

  const renameSelectedObject = useCallback(async () => {
    if (!selectedId || !selectedEntityNameDraft.trim()) return;
    await rpc('shell/rename_object', { id: selectedId, name: selectedEntityNameDraft.trim() });
    await refreshSceneTree();
  }, [refreshSceneTree, selectedEntityNameDraft, selectedId]);

  const duplicateSelectedObject = useCallback(async () => {
    if (!selectedId) return;
    const duplicated = await rpc<SceneObject>('shell/duplicate_object', { id: selectedId });
    await refreshSceneTree();
    selectSceneObject(duplicated.id);
  }, [refreshSceneTree, selectSceneObject, selectedId]);

  const deleteSelectedObject = useCallback(async () => {
    if (!selectedId) return;
    await rpc('shell/delete_object', { id: selectedId });
    setSelectedId(null);
    setSelectedEntityDetails(null);
    await refreshSceneTree();
  }, [refreshSceneTree, selectedId]);

  const reparentSelectedObject = useCallback(async (parentId: string) => {
    if (!selectedId) return;
    await rpc('shell/reparent_object', {
      id: selectedId,
      parent_id: parentId || undefined,
    });
    await refreshSceneTree();
  }, [refreshSceneTree, selectedId]);

  const updateSelectedTransform = useCallback(async (
    field: 'position' | 'rotation' | 'scale',
    index: number,
    rawValue: string,
  ) => {
    if (!selectedId || !selectedEntityDetails) return;
    const value = Number(rawValue);
    if (!Number.isFinite(value)) return;
    const next = [...selectedEntityDetails.transform[field]];
    next[index] = value;
    await rpc('shell/update_transform', { id: selectedId, [field]: next });
    await refreshSceneTree();
  }, [refreshSceneTree, selectedEntityDetails, selectedId]);

  const addSelectedComponent = useCallback(async () => {
    if (!selectedId) return;
    await rpc('shell/add_component', { id: selectedId, component_type: addComponentType });
    await refreshSceneTree();
  }, [addComponentType, refreshSceneTree, selectedId]);

  const removeSelectedComponent = useCallback(async (componentType: string) => {
    if (!selectedId) return;
    await rpc('shell/remove_component', { id: selectedId, component_type: componentType });
    await refreshSceneTree();
  }, [refreshSceneTree, selectedId]);

  const updateSelectedComponentField = useCallback(async (
    componentType: string,
    fieldName: string,
    value: unknown,
  ) => {
    if (!selectedId) return;
    await rpc('shell/update_component', {
      id: selectedId,
      component_type: componentType,
      data: { [fieldName]: value },
    });
    await refreshSceneTree();
  }, [refreshSceneTree, selectedId]);

  // Quick actions from AI panel
  const handleQuickAction = useCallback(async (action: string) => {
    switch (action) {
      case 'save':
        await rpc('shell/save_scene').catch(() => {});
        await refreshSceneTree();
        break;
      case 'undo':
        await rpc('shell/undo').catch(() => {});
        await refreshSceneTree();
        break;
      case 'play':
        openGameView();
        break;
    }
  }, [refreshSceneTree]);

  // Close project handler
  const handleClose = useCallback(() => {
    if (shellState?.scene_dirty) {
      setShowCloseDialog(true);
    } else {
      onCloseProject();
    }
  }, [onCloseProject, shellState?.scene_dirty]);

  // Keyboard shortcuts (minimal set)
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLElement) {
        const tag = e.target.tagName;
        if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
        if (e.target.isContentEditable) return;
      }
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key === 's') {
        e.preventDefault();
        rpc('shell/save_scene').then(() => refreshSceneTree());
      }
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key === 'z') {
        e.preventDefault();
        rpc('shell/undo').then(() => refreshSceneTree());
      }
      if ((e.ctrlKey || e.metaKey) && e.key === 'y') {
        e.preventDefault();
        rpc('shell/redo').then(() => refreshSceneTree());
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [refreshSceneTree]);

  // Resize handle for AI panel
  const handleResizeDown = useDragHandle('horizontal', (delta) => {
    setAiPanelWidth((w) => Math.max(320, Math.min(w - delta, 560)));
  });

  // Viewport click-to-pick
  const handleViewportClick = useCallback((e: React.MouseEvent) => {
    // Only left-click, not on buttons or gizmo elements
    if (e.button !== 0) return;
    const target = e.target as HTMLElement;
    if (target.closest('.transform-gizmo') || target.closest('.scene-guides') || target.closest('.orientation-gizmo')) return;

    const container = e.currentTarget as HTMLElement;
    const rect = container.getBoundingClientRect();
    const clickX = e.clientX - rect.left;
    const clickY = e.clientY - rect.top;
    const vpWidth = rect.width;
    const vpHeight = rect.height;

    const hitId = pickEntityAtScreen(
      clickX, clickY, vpWidth, vpHeight,
      sceneTree, cameraRef.current,
      viewMode,
    );

    if (hitId) {
      const object = sceneTree.find(item => item.id === hitId);
      selectSceneObject(hitId);
      if (object) {
        setArtifactSelection({
          kind: 'model',
          label: object.name,
          context: `Scene model: ${object.name}\nEntity ID: ${object.id}\nTag: ${object.tag || 'Untagged'}\nPosition: ${object.position.join(', ')}`,
          x: e.clientX,
          y: e.clientY,
        });
        setArtifactQuestionOpen(false);
      }
    } else {
      selectSceneObject(null);
      setArtifactSelection(null);
    }
  }, [sceneTree, selectSceneObject, viewMode]);

  const selectDocumentText = useCallback((event: React.MouseEvent<HTMLElement>) => {
    const selection = window.getSelection();
    const text = selection?.toString().trim();
    if (!text || text.length < 2) return;
    setArtifactSelection({
      kind: 'document',
      label: text.length > 48 ? `${text.slice(0, 48)}…` : text,
      context: `Spec excerpt:\n${text}`,
      x: event.clientX,
      y: event.clientY,
    });
    setArtifactQuestionOpen(false);
  }, []);

  const selectScriptLine = useCallback((line: number, extend: boolean, event: React.MouseEvent) => {
    const nextRange: [number, number] = extend && scriptLineRange
      ? [Math.min(scriptLineRange[0], line), Math.max(scriptLineRange[1], line)]
      : [line, line];
    setScriptLineRange(nextRange);
    const lines = scriptContent.split('\n').slice(nextRange[0] - 1, nextRange[1]);
    setArtifactSelection({
      kind: 'code',
      label: `${selectedScript || 'script'}:${nextRange[0]}${nextRange[1] === nextRange[0] ? '' : `-${nextRange[1]}`}`,
      context: `Script: ${selectedScript}\nLines ${nextRange[0]}-${nextRange[1]}:\n${lines.join('\n')}`,
      x: event.clientX,
      y: event.clientY,
    });
    setArtifactQuestionOpen(false);
  }, [scriptContent, scriptLineRange, selectedScript]);

  const submitArtifactQuestion = useCallback(() => {
    if (!artifactSelection || !artifactQuestion.trim()) return;
    setContextualRequest({
      id: Date.now(),
      prompt: `${artifactQuestion.trim()}\n\n[Selected context]\n${artifactSelection.context}`,
    });
    setArtifactQuestion('');
    setArtifactQuestionOpen(false);
  }, [artifactQuestion, artifactSelection]);

  const saveSelectedScript = useCallback(async () => {
    if (!selectedScript) return;
    setScriptSaving(true);
    try {
      await rpc('project/write_file', { path: selectedScript, content: scriptContent });
      setScriptSavedContent(scriptContent);
      await refreshConsoleEntries().catch(() => {});
    } finally {
      setScriptSaving(false);
    }
  }, [refreshConsoleEntries, scriptContent, selectedScript]);

  const createPresetObject = useCallback(async (
    name: string,
    componentType?: string,
  ) => {
    const created = await rpc<SceneObject>('shell/create_object', {
      name,
      parent_id: selectedId ?? undefined,
    });
    if (componentType) {
      await rpc('shell/add_component', { id: created.id, component_type: componentType });
    }
    await refreshSceneTree();
    selectSceneObject(created.id);
  }, [refreshSceneTree, selectSceneObject, selectedId]);

  const createBehaviorObject = useCallback(async () => {
    const name = `behavior_${Date.now()}`;
    const result = await rpc<{ path: string }>('project/create_script', {
      name,
      backend: 'rhai',
    });
    const created = await rpc<SceneObject>('shell/create_object', {
      name: 'Behavior Object',
      parent_id: selectedId ?? undefined,
    });
    await rpc('shell/add_component', { id: created.id, component_type: 'Script' });
    await rpc('shell/update_component', {
      id: created.id,
      component_type: 'Script',
      data: { script: result.path },
    });
    await refreshProjectAssets();
    await refreshSceneTree();
    selectSceneObject(created.id);
    setSelectedScript(result.path);
    setWorkspaceView('scripts');
  }, [refreshProjectAssets, refreshSceneTree, selectSceneObject, selectedId]);

  const promoteCurrentContext = useCallback(async () => {
    if (!onPromoteToQuest || !shellState) return;
    const currentSelectedObject = selectedId
      ? sceneTree.find(object => object.id === selectedId) ?? null
      : null;
    const contextLines = [
      `Project: ${shellState.project_name}`,
      `Active editor view: ${workspaceView}`,
      `Scene objects: ${sceneTree.length}`,
      currentSelectedObject ? `Selected entity: ${currentSelectedObject.name} (${currentSelectedObject.id}) at ${currentSelectedObject.position.join(', ')}` : null,
      selectedScript ? `Selected script: ${selectedScript}` : null,
      scriptLineRange ? `Selected script lines: ${scriptLineRange[0]}-${scriptLineRange[1]}` : null,
      artifactSelection ? `Selected artifact context:\n${artifactSelection.context}` : null,
      questArtifact ? `Opened Quest artifact: ${questArtifact.kind} ${questArtifact.label}` : null,
      openedQuestArtifact ? `Quest artifact inspection: ${openedQuestArtifact.title}\n${openedQuestArtifact.description}` : null,
    ].filter(Boolean).join('\n');
    const prompt = currentSelectedObject
      ? `Continue this Editor work as a Quest for ${currentSelectedObject.name}`
      : selectedScript
        ? `Continue this Editor script work as a Quest`
        : `Continue this Editor work as a Quest`;
    setPromotingQuest(true);
    setPromoteError(null);
    try {
      await onPromoteToQuest(prompt, contextLines);
    } catch (reason) {
      setPromoteError(String(reason));
    } finally {
      setPromotingQuest(false);
    }
  }, [
    artifactSelection,
    onPromoteToQuest,
    openedQuestArtifact,
    questArtifact,
    sceneTree,
    scriptLineRange,
    selectedId,
    selectedScript,
    shellState,
    workspaceView,
  ]);

  // Derive selected entity name
  const selectedEntityName = selectedId
    ? sceneTree.find(o => o.id === selectedId)?.name ?? null
    : null;
  const selectedObject = selectedId
    ? sceneTree.find(o => o.id === selectedId) ?? null
    : null;
  const scriptDirty = scriptContent !== scriptSavedContent;
  const hasSpecArtifact = Boolean(openedQuestArtifact?.surface === 'prd');
  const hasTaskArtifact = Boolean(openedQuestArtifact?.surface === 'tasks' || aiWorkspace?.plan);
  const hasDiagnostics = consoleEntries.length > 0 || Boolean(promoteError);
  const visibleWorkspaceTabs = ([
    ...(hasSpecArtifact ? [['prd', 'Spec', <IconFile key="prd" />] as const] : []),
    ...(hasTaskArtifact ? [['tasks', 'AI work', <IconCheck key="tasks" />] as const] : []),
    ['game', t('tab_game_view'), <IconPlay key="game" />] as const,
    ['assets', 'Assets', <IconFile key="assets" />] as const,
    ['scripts', t('tab_scripts'), <IconCode key="scripts" />] as const,
    ['build', 'Build', <IconPackage key="build" />] as const,
    ...(hasDiagnostics ? [['diagnostics', 'Diagnostics', <IconAlertCircle key="diagnostics" />] as const] : []),
  ]);

  useEffect(() => {
    const visible = new Set<WorkspaceView>(visibleWorkspaceTabs.map(([view]) => view));
    if (!visible.has(workspaceView)) {
      setWorkspaceView('game');
    }
  }, [visibleWorkspaceTabs, workspaceView]);

  // ── Render ──

  if (!shellState) {
    return <div className="loading">{t('loading_editor')}</div>;
  }

  return (
    <div className="editor editor-ai-first">
      {/* Minimal toolbar */}
      <div className="editor-toolbar editor-toolbar-minimal">
        <div className="toolbar-project">
          <span className="toolbar-project-kicker">{t('editor_workspace_kicker')}</span>
          <span className="toolbar-project-name">{shellState.project_name || t('editor_untitled')}</span>
        </div>
        <span className="ai-only-badge">AI-native</span>
        <button
          className="tool-btn quest-mode-btn"
          onClick={promoteCurrentContext}
          disabled={!onPromoteToQuest || promotingQuest}
          title="Promote current Editor context to Quest"
        >
          {promotingQuest ? <IconLoader className="spin-icon" /> : <IconSparkles />} <span>Promote</span>
        </button>
        <button className="tool-btn quest-mode-btn" onClick={onOpenQuest} title="Open Quest Mode"><IconProjects /> <span>Quests</span></button>
        <button className="tool-btn" onClick={() => setWorkspaceView('build')} title="Build and package"><IconPackage /></button>
        <button className="tool-btn play-btn" onClick={() => setWorkspaceView('game')} title={t('editor_open_game_view')}><IconPlay /></button>
        <button className="tool-btn" onClick={handleClose} title={t('editor_close')}><IconX /></button>
      </div>

      {/* Main body: viewport + AI panel */}
      <div className="editor-body editor-body-ai">
        <main className="ai-product-workspace" ref={viewportMainRef}>
          {questArtifact && (
            <div className="quest-artifact-banner">
              <IconProjects />
              <div>
                <span>Opened from Quest</span>
                <strong>{questArtifact.questTitle}</strong>
                <small>{questArtifact.kind.replaceAll('_', ' ')} · {questArtifact.label}</small>
              </div>
              <button onClick={onOpenQuest}><IconProjects /> Return</button>
              <button onClick={onDismissQuestArtifact} title="Dismiss"><IconX /></button>
            </div>
          )}
          {promoteError && (
            <div className="quest-artifact-banner promote-error-banner">
              <IconAlertCircle />
              <div>
                <span>Promote to Quest failed</span>
                <small>{promoteError}</small>
              </div>
              <button onClick={() => setPromoteError(null)}><IconX /></button>
            </div>
          )}
          <nav className="product-tabs" role="tablist" aria-label="Editor surfaces">
            {visibleWorkspaceTabs.map(([view, label, icon]) => (
              <button key={view} className={workspaceView === view ? 'active' : ''} onClick={() => setWorkspaceView(view)} role="tab" aria-selected={workspaceView === view}>
                {icon}<span>{label}</span>
                {view === 'tasks' && aiWorkspace?.plan && <b>{aiWorkspace.plan.operations.length}</b>}
                {view === 'assets' && assets.length > 0 && <b>{assets.length}</b>}
                {view === 'scripts' && scripts.length > 0 && <b>{scripts.length}</b>}
                {view === 'diagnostics' && consoleEntries.length > 0 && <b>{consoleEntries.length}</b>}
              </button>
            ))}
          </nav>

          <section className={`product-view product-view-${workspaceView}`}>
            {workspaceView === 'prd' && openedQuestArtifact && <article className="prd-document" onMouseUp={selectDocumentText}>
              <header><span>{t('prd_header')}</span><strong>{shellState.project_name || t('prd_untitled_game')}</strong><p>{t('prd_brief_desc')}</p></header>
              <section><h2>{t('prd_vision')}</h2><p>{t('prd_vision_text').replace('{scene_count}', String(sceneTree.length)).replace('{script_count}', String(scripts.length))}</p></section>
              <section><h2>{t('prd_current_scope')}</h2><div className="prd-grid"><div><span>{t('prd_scope_player_exp')}</span><strong>{t('prd_scope_playable')}</strong></div><div><span>{t('prd_scope_world')}</span><strong>{sceneTree.length} {t('prd_scope_authored')}</strong></div><div><span>{t('prd_scope_automation')}</span><strong>{t('prd_scope_review')}</strong></div><div><span>{t('prd_scope_delivery')}</span><strong>{t('prd_scope_verification')}</strong></div></div></section>
              <section><h2>{t('prd_acceptance')}</h2><ul><li>{t('prd_criteria_1')}</li><li>{t('prd_criteria_2')}</li><li>{t('prd_criteria_3')}</li><li>{t('prd_criteria_4')}</li></ul></section>
            </article>}

            {workspaceView === 'tasks' && <div className="task-board">
              <header><div><span>AI artifact</span><h1>{aiWorkspace?.plan ? 'Changes need review' : openedQuestArtifact ? openedQuestArtifact.title : 'AI work'}</h1></div><small>{shellState.project_name} · {sceneTree.length} {t('label_objects')}</small></header>
              {openedQuestArtifact && (
                <section className="quest-artifact-context-card">
                  <div>
                    <span>{questArtifact?.kind.replaceAll('_', ' ')}</span>
                    <strong>{openedQuestArtifact.title}</strong>
                    <p>{openedQuestArtifact.description}</p>
                    {openedQuestArtifact.focusPath && <small>{openedQuestArtifact.focusPath}</small>}
                  </div>
                  <div>
                    {openedQuestArtifact.surface !== 'tasks' && (
                      <button onClick={() => setWorkspaceView(openedQuestArtifact.surface)}>
                        <IconFile /> Open surface
                      </button>
                    )}
                    <button
                      onClick={() => setContextualRequest({
                        id: Date.now(),
                        prompt: `Inspect this Quest artifact and suggest the next local editor check.\n\n${openedQuestArtifact.title}\n${openedQuestArtifact.description}\n${openedQuestArtifact.focusPath ?? ''}`,
                      })}
                    >
                      <IconSparkles /> Ask AI
                    </button>
                    <button onClick={onOpenQuest}><IconProjects /> Return</button>
                  </div>
                </section>
              )}
              <section className="task-operations"><div className="task-section-title"><span>{t('task_proposed_ops')}</span></div>
                {!aiWorkspace?.plan ? <div className="product-empty"><IconProjects /><strong>{t('task_no_plan')}</strong><span>{t('task_no_plan_desc')}</span></div> : aiWorkspace.plan.operations.map(operation => <div key={operation.index}><span className={operation.permission_kind}>{operation.permission_kind.toUpperCase()}</span><p>{operation.preview}</p><small>{operation.permission_kind === 'read' ? t('op_auto_allowed') : aiWorkspace.approved.has(operation.index) ? t('op_allowed') : aiWorkspace.denied.has(operation.index) ? t('op_denied_once') : t('op_awaiting')}</small></div>)}
              </section>
              {aiWorkspace?.plan && <footer><button className="btn btn-ghost" onClick={aiWorkspace.discardProposal}>{t('btn_discard')}</button><button className="btn btn-primary" disabled={aiWorkspace.approved.size === 0} onClick={aiWorkspace.applyApproved}>{t('btn_continue_allowed').replace('{count}', String(aiWorkspace.approved.size))}</button></footer>}
            </div>}

            {workspaceView === 'game' && <div className="game-editor-surface">
              <aside className="game-hierarchy-panel">
                <header>
                  <div><span>Hierarchy</span><strong>{sceneTree.length} objects</strong></div>
                  <button onClick={() => createSceneObject()} title="Create object"><IconPlus /></button>
                </header>
                <div className="game-hierarchy-list">
                  {sceneTree.length === 0 ? <p>No scene objects.</p> : sceneTree.map(object => (
                    <button
                      key={object.id}
                      className={selectedId === object.id ? 'selected' : ''}
                      onClick={() => selectSceneObject(object.id)}
                      style={{ paddingLeft: object.parent_id ? 22 : 10 }}
                    >
                      <span>{object.parent_id ? '↳' : '●'}</span>
                      <strong>{object.name}</strong>
                      <small>{object.tag || 'Untagged'}</small>
                    </button>
                  ))}
                </div>
              </aside>

              <section className="game-main-panel">
                <div className="game-preview-bar">
                  <div><span className="live-dot" />Scene/Game View</div>
                  <div className="viewport-mode-switch">
                    <button className={viewMode === '2d' ? 'active' : ''} onClick={() => setViewMode('2d')}>2D</button>
                    <button className={viewMode === '3d' ? 'active' : ''} onClick={() => setViewMode('3d')}>3D</button>
                    <button onClick={() => rpc('shell/undo').then(() => refreshSceneTree())} disabled={!shellState.can_undo}>Undo</button>
                    <button onClick={() => rpc('shell/redo').then(() => refreshSceneTree())} disabled={!shellState.can_redo}>Redo</button>
                    <button onClick={() => openNativeSceneView({
                      yaw: cameraRef.current.yaw,
                      pitch: cameraRef.current.pitch,
                      distance: cameraRef.current.distance,
                      targetX: cameraRef.current.targetX,
                      targetY: cameraRef.current.targetY,
                      targetZ: cameraRef.current.targetZ,
                    })}>Native View</button>
                    <button onClick={openGameView}><IconPlay /> Run</button>
                  </div>
                </div>
                <div className="game-create-presets" aria-label="Create scene preset">
                  <button onClick={() => createSceneObject()}><IconPlus /> Empty</button>
                  <button onClick={() => createPresetObject('Camera', 'Camera')}><IconPlus /> Camera</button>
                  <button onClick={() => createPresetObject('Light', 'Light')}><IconPlus /> Light</button>
                  <button onClick={() => createPresetObject('Mesh Object', 'MeshRenderer')}><IconPlus /> Mesh</button>
                  <button onClick={() => createPresetObject('Audio Source', 'AudioSource')}><IconPlus /> Audio</button>
                  <button onClick={() => createPresetObject('Rigid Body', 'Rigidbody')}><IconPlus /> Rigidbody</button>
                  <button onClick={() => createPresetObject('Collider', 'Collider')}><IconPlus /> Collider</button>
                  <button onClick={createBehaviorObject}><IconCode /> Behavior</button>
                </div>
                <div className="game-preview-canvas" onClick={handleViewportClick}>
                  <ViewportCanvas sceneVersion={sceneVersion} cameraRef={cameraRef} onCameraChange={handleCameraChange} viewMode={viewMode} editorCamera />
                  <SelectionOverlay sceneTree={sceneTree} selectedId={selectedId} camera={cameraRef.current} width={viewportSize.width} height={viewportSize.height} viewMode={viewMode} />
                </div>
              </section>

              <aside className="game-inspector-panel">
                <header><span>Inspector</span>{selectedEntityDetails && <strong>{selectedEntityDetails.name}</strong>}</header>
                {!selectedEntityDetails ? (
                  <div className="inspector-empty">Select an object in the viewport or hierarchy.</div>
                ) : (
                  <div className="inspector">
                    <section className="inspector-section">
                      <div className="inspector-section-title">Object</div>
                      <label className="inspector-field">
                        <span>Name</span>
                        <input
                          className="inspector-text-input"
                          value={selectedEntityNameDraft}
                          onChange={event => setSelectedEntityNameDraft(event.target.value)}
                          onBlur={renameSelectedObject}
                          onKeyDown={event => {
                            if (event.key === 'Enter') event.currentTarget.blur();
                            if (event.key === 'Escape') setSelectedEntityNameDraft(selectedEntityDetails.name);
                          }}
                        />
                      </label>
                      <label className="inspector-field">
                        <span>Parent</span>
                        <select
                          className="inspector-text-input"
                          value={selectedObject?.parent_id ?? ''}
                          onChange={event => reparentSelectedObject(event.currentTarget.value)}
                        >
                          <option value="">Scene root</option>
                          {validParentOptions.map(object => (
                            <option key={object.id} value={object.id}>{object.name}</option>
                          ))}
                        </select>
                      </label>
                      <div className="inspector-action-row">
                        <button onClick={duplicateSelectedObject}><IconCopy /> Duplicate</button>
                        <button onClick={deleteSelectedObject}><IconTrash /> Delete</button>
                      </div>
                    </section>
                    {(['position', 'rotation', 'scale'] as const).map(field => (
                      <section className="inspector-section" key={field}>
                        <div className="inspector-section-title">{field}</div>
                        <div className="inspector-vec3">
                          {selectedEntityDetails.transform[field].map((value, index) => (
                            <label className="inspector-vec3-input-wrap" key={`${field}-${index}`}>
                              <span className="inspector-vec3-label">{['X', 'Y', 'Z', 'W'][index]}</span>
                              <input
                                defaultValue={value.toFixed(2)}
                                inputMode="decimal"
                                onBlur={event => updateSelectedTransform(field, index, event.currentTarget.value)}
                                onKeyDown={event => {
                                  if (event.key === 'Enter') event.currentTarget.blur();
                                  if (event.key === 'Escape') event.currentTarget.blur();
                                }}
                              />
                            </label>
                          ))}
                        </div>
                      </section>
                    ))}
                    <section className="inspector-section">
                      <div className="inspector-section-title">Components</div>
                      {selectedEntityDetails.components.map(component => (
                        <div className="inspector-component" key={component.type}>
                          <div className="inspector-component-header">
                            <span className="inspector-component-type">{component.type}</span>
                            <button className="inspector-remove-btn" onClick={() => removeSelectedComponent(component.type)} title="Remove component">×</button>
                          </div>
                          <div className="inspector-component-fields">
                            {Object.entries(component.data ?? {}).length === 0 ? (
                              <div className="inspector-field-empty">No editable fields</div>
                            ) : Object.entries(component.data ?? {}).map(([fieldName, value]) => (
                              <ComponentFieldEditor
                                key={`${component.type}-${fieldName}`}
                                fieldName={fieldName}
                                value={value}
                                onCommit={(name, nextValue) => updateSelectedComponentField(component.type, name, nextValue)}
                              />
                            ))}
                          </div>
                        </div>
                      ))}
                      <div className="inspector-add-row">
                        <select value={addComponentType} onChange={event => setAddComponentType(event.target.value)}>
                          {['Camera', 'Light', 'MeshRenderer', 'Rigidbody', 'Collider', 'AudioSource', 'AudioListener', 'Script'].map(type => (
                            <option key={type} value={type}>{type}</option>
                          ))}
                        </select>
                        <button onClick={addSelectedComponent}><IconPlus /> Add</button>
                      </div>
                    </section>
                  </div>
                )}
              </aside>
            </div>}

            {workspaceView === 'assets' && <div className="assets-surface">
              <header>
                <div>
                  <span>Project/Assets</span>
                  <strong>{assets.length} assets</strong>
                </div>
                <div className="assets-toolbar">
                  <button onClick={() => createProjectAsset('script')} disabled={assetsBusy}>Script</button>
                  <button onClick={() => createProjectAsset('material')} disabled={assetsBusy}>Material</button>
                  <button onClick={() => createProjectAsset('prefab')} disabled={assetsBusy}>Prefab</button>
                  <button onClick={() => createProjectAsset('scene')} disabled={assetsBusy}>Scene</button>
                  <button onClick={refreshProjectAssets} disabled={assetsBusy}>
                    {assetsBusy ? 'Refreshing' : 'Refresh'}
                  </button>
                </div>
              </header>
              <div className="assets-list">
                {assets.length === 0 ? (
                  <div className="assets-empty">No imported assets found.</div>
                ) : assets.map(asset => {
                  const canOpenScript = isScriptPath(asset.source_path);
                  return (
                    <article className="asset-row" key={asset.guid || asset.source_path}>
                      <IconFile />
                      <div>
                        <strong>{asset.source_path.split('/').pop() || asset.source_path}</strong>
                        <span>{asset.source_path}</span>
                      </div>
                      <small>{asset.kind}</small>
                      <small>{asset.importer || 'default'}</small>
                      <div className="asset-row-actions">
                        {canOpenScript ? (
                          <button onClick={() => {
                            setSelectedScript(asset.source_path);
                            setWorkspaceView('scripts');
                          }}>Open</button>
                        ) : (
                          <button onClick={event => {
                            setArtifactSelection({
                              kind: 'document',
                              label: asset.source_path,
                              context: `Asset: ${asset.source_path}\nKind: ${asset.kind}\nImporter: ${asset.importer || 'default'}\nGUID: ${asset.guid}`,
                              x: event.clientX,
                              y: event.clientY,
                            });
                            setArtifactQuestionOpen(false);
                          }}>Inspect</button>
                        )}
                        <button onClick={event => revealAssetReferences(asset, event)} disabled={assetsBusy}>Refs</button>
                        <button onClick={() => reloadAsset(asset.source_path)} disabled={assetsBusy}>Reload</button>
                      </div>
                    </article>
                  );
                })}
              </div>
            </div>}

            {workspaceView === 'scripts' && <div className="script-preview">
              <aside>
                <header>{t('scripts_header')} <span>{scripts.length}</span></header>
                {scripts.length === 0 ? <p>{t('scripts_empty')}</p> : scripts.map(path => (
                  <button key={path} className={selectedScript === path ? 'active' : ''} onClick={() => setSelectedScript(path)}>
                    <IconCode /><span>{path.split('/').pop()}</span><small>{path}</small>
                  </button>
                ))}
              </aside>
              <article>
                <header>
                  <span>{selectedScript || t('scripts_select')}{scriptDirty ? ' *' : ''}</span>
                  <div>
                    <b>{t('scripts_line_select_hint')}</b>
                    <button onClick={saveSelectedScript} disabled={!selectedScript || !scriptDirty || scriptSaving}>
                      {scriptSaving ? 'Saving' : 'Save'}
                    </button>
                  </div>
                </header>
                <div className="script-editor-pane">
                  <textarea
                    value={scriptContent}
                    spellCheck={false}
                    disabled={!selectedScript}
                    onChange={event => setScriptContent(event.currentTarget.value)}
                    onSelect={event => {
                      const target = event.currentTarget;
                      const before = target.value.slice(0, target.selectionStart);
                      const selectedText = target.value.slice(target.selectionStart, target.selectionEnd);
                      if (!selectedText.trim()) return;
                      const startLine = before.split('\n').length;
                      const endLine = startLine + selectedText.split('\n').length - 1;
                      setScriptLineRange([startLine, endLine]);
                    }}
                    onKeyDown={event => {
                      if ((event.ctrlKey || event.metaKey) && event.key === 's') {
                        event.preventDefault();
                        saveSelectedScript();
                      }
                    }}
                  />
                  <pre className="selectable-code script-line-gutter"><code>{(scriptContent || '// Aster-generated scripts will appear here.').split('\n').map((line, index) => { const lineNumber = index + 1; const selected = scriptLineRange && lineNumber >= scriptLineRange[0] && lineNumber <= scriptLineRange[1]; return <button key={lineNumber} className={selected ? 'selected' : ''} onClick={event => selectScriptLine(lineNumber, event.shiftKey, event)}><span>{lineNumber}</span><i>{line || ' '}</i></button>; })}</code></pre>
                </div>
              </article>
            </div>}

            {workspaceView === 'build' && <div className="build-surface">
              <header>
                <div>
                  <span>Build & Package</span>
                  <strong>{shellState.project_name || 'Untitled project'}</strong>
                </div>
                <button onClick={requestBuildPackage} disabled={buildBusy}>
                  {buildBusy ? <IconLoader className="spin-icon" /> : <IconPackage />}
                  {buildBusy ? 'Preparing' : 'Package'}
                </button>
              </header>

              <div className="build-layout">
                <section className="build-main">
                  <div className="build-presets" aria-label="Build presets">
                    {BUILD_PRESETS.map(preset => (
                      <button
                        key={preset.id}
                        className={buildTarget === preset.target && buildFormat === preset.format && buildChannel === preset.channel ? 'active' : ''}
                        onClick={() => applyBuildPreset(preset)}
                      >
                        <IconPackage />
                        <span>{preset.label}</span>
                        <small>{preset.channel} · {preset.format}</small>
                      </button>
                    ))}
                  </div>

                  <div className="build-section">
                    <div className="build-section-title">
                      <span>Target</span>
                      <strong className={`build-status build-status-${selectedBuildTarget.status}`}>{selectedBuildTarget.status}</strong>
                    </div>
                    <div className="build-target-grid">
                      {BUILD_TARGETS.map(target => (
                        <button
                          key={target.id}
                          className={buildTarget === target.id ? 'selected' : ''}
                          onClick={() => {
                            setBuildTarget(target.id);
                            setBuildFormat(target.formats[0]);
                          }}
                        >
                          <span>{target.label}</span>
                          <small>{target.formats.join(', ')}</small>
                          <b className={`build-status build-status-${target.status}`}>{target.status}</b>
                        </button>
                      ))}
                    </div>
                  </div>

                  <div className="build-section">
                    <div className="build-section-title">
                      <span>Package</span>
                      <strong>{selectedBuildTarget.label}</strong>
                    </div>
                    <div className="build-form-grid">
                      <label>
                        <span>Format</span>
                        <select value={buildFormat} onChange={event => setBuildFormat(event.currentTarget.value as BuildFormat)}>
                          {selectedBuildTarget.formats.map(format => (
                            <option key={format} value={format}>{format}</option>
                          ))}
                        </select>
                      </label>
                      <label>
                        <span>Channel</span>
                        <select value={buildChannel} onChange={event => setBuildChannel(event.currentTarget.value as 'debug' | 'release')}>
                          <option value="release">release</option>
                          <option value="debug">debug</option>
                        </select>
                      </label>
                      <label className="build-checkbox">
                        <input
                          type="checkbox"
                          checked={buildOptimizeAssets}
                          onChange={event => setBuildOptimizeAssets(event.currentTarget.checked)}
                        />
                        <span>Optimize assets</span>
                      </label>
                      <label className="build-checkbox">
                        <input
                          type="checkbox"
                          checked={buildIncludeDebugSymbols}
                          onChange={event => setBuildIncludeDebugSymbols(event.currentTarget.checked)}
                        />
                        <span>Debug symbols</span>
                      </label>
                    </div>
                  </div>

                  <div className="build-output">
                    <div>
                      <span>Output</span>
                      <strong>exports/{shellState.project_name || 'project'}/{buildTarget}/{buildChannel}</strong>
                    </div>
                    <p>{selectedBuildTarget.note}</p>
                    {buildMessage && <pre>{buildMessage}</pre>}
                  </div>
                </section>

                <aside className="build-sidebar">
                  <section>
                    <span>Pipeline</span>
                    <ol>
                      <li className="complete"><IconCheck /> Validate project</li>
                      <li className="active"><IconPackage /> Export runtime</li>
                      <li><IconPackage /> Bundle assets</li>
                      <li><IconPackage /> Create installer</li>
                      <li><IconCheck /> Sign & verify</li>
                    </ol>
                  </section>
                  <section>
                    <span>Current request</span>
                    <dl>
                      <div><dt>Project</dt><dd>{shellState.project_name || 'project'}</dd></div>
                      <div><dt>Target</dt><dd>{selectedBuildTarget.label}</dd></div>
                      <div><dt>Format</dt><dd>{buildFormat}</dd></div>
                      <div><dt>Assets</dt><dd>{buildOptimizeAssets ? 'optimized' : 'raw'}</dd></div>
                      <div><dt>Symbols</dt><dd>{buildIncludeDebugSymbols ? 'included' : 'stripped'}</dd></div>
                    </dl>
                  </section>
                </aside>
              </div>
            </div>}

            {workspaceView === 'diagnostics' && <div className="diagnostics-surface">
              <header>
                <div>
                  <span>Console</span>
                  <strong>{consoleEntries.length} diagnostics</strong>
                </div>
                <div>
                  <button onClick={() => refreshConsoleEntries()} disabled={consoleBusy}>{consoleBusy ? 'Refreshing' : 'Refresh'}</button>
                  <button onClick={clearConsoleEntries} disabled={consoleBusy || consoleEntries.length === 0}>Clear</button>
                </div>
              </header>
              <div className="diagnostics-list">
                {consoleEntries.length === 0 ? (
                  <div className="diagnostics-empty">No diagnostics or tool output yet.</div>
                ) : consoleEntries.map((entry, index) => (
                  <article className={`diagnostics-entry level-${entry.level}`} key={`${entry.timestamp}-${index}`}>
                    <div>
                      <span>{entry.level}</span>
                      <strong>{entry.subsystem || 'editor'}</strong>
                    </div>
                    <p>{entry.message}</p>
                    {(entry.file || entry.line) && (
                      <small>{entry.file ?? 'source'}{entry.line ? `:${entry.line}` : ''}</small>
                    )}
                  </article>
                ))}
              </div>
            </div>}
          </section>

          {artifactSelection && <div className={`artifact-ask-popover ${artifactQuestionOpen ? 'expanded' : ''}`} style={{ left: artifactSelection.x, top: artifactSelection.y }}>
            {!artifactQuestionOpen ? <button onClick={() => setArtifactQuestionOpen(true)}><IconSparkles /> {t('artifact_ask_about').replace('{kind}', artifactSelection.kind)}</button> : <div><header><span>{artifactSelection.label}</span><button onClick={() => setArtifactSelection(null)}><IconX /></button></header><div><input autoFocus value={artifactQuestion} onChange={event => setArtifactQuestion(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') submitArtifactQuestion(); if (event.key === 'Escape') setArtifactQuestionOpen(false); }} placeholder={t('artifact_ask_placeholder')} /><button onClick={submitArtifactQuestion} disabled={!artifactQuestion.trim()}>{t('btn_ask')}</button></div></div>}
          </div>}
        </main>

        {/* Resize handle */}
        <div
          className="resize-handle resize-handle-right"
          onMouseDown={handleResizeDown}
          role="separator"
          aria-label="Resize AI workspace"
          aria-orientation="vertical"
          aria-valuemin={360}
          aria-valuemax={720}
          aria-valuenow={aiPanelWidth}
          tabIndex={0}
          onKeyDown={event => {
            if (event.key === 'ArrowLeft') setAiPanelWidth(width => Math.min(720, width + 16));
            if (event.key === 'ArrowRight') setAiPanelWidth(width => Math.max(360, width - 16));
          }}
        />

        {/* AI Panel (right side) */}
        <aside className="ai-panel-container" style={{ width: aiPanelWidth }}>
          <AiPanel
            projectName={shellState.project_name}
            selectedEntity={selectedId}
            selectedEntityName={selectedEntityName}
            sceneObjectCount={sceneTree.length}
            sceneObjects={sceneTree}
            onQuickAction={handleQuickAction}
            onSceneChanged={handleAiSceneChanged}
            onFocusPosition={focusOnPosition}
            chatOnly
            onWorkspaceStateChange={setAiWorkspace}
            contextualRequest={contextualRequest}
            onContextualRequestConsumed={id => setContextualRequest(current => current?.id === id ? null : current)}
            onOpenSettings={onOpenSettings}
          />
        </aside>
      </div>

      {/* Status Bar */}
      <footer className="editor-statusbar">
        <div className="status-group">
          <span className="status-item">{shellState.project_name || t('status_no_project')}</span>
          <span className="status-divider" />
          <span className="status-item">{sceneTree.length} {t('label_objects')}</span>
          {selectedEntityName && <><span className="status-divider" /><span className="status-item status-selection">{t('status_selected')} {selectedEntityName}</span></>}
        </div>
        <div className="status-group">
          {shellState.scene_dirty ? (
            <span className="status-item status-dirty"><span className="status-dot" />{t('status_unsaved')}</span>
          ) : (
            <span className="status-item status-saved">{t('status_saved')}</span>
          )}
          <span className="status-divider" />
          <span className="status-item" style={{ color: 'var(--accent)' }}>v0.1.0</span>
        </div>
      </footer>

      {/* Close Project Dialog */}
      {showCloseDialog && shellState && (
        <CloseProjectDialog
          projectName={shellState.project_name || 'project'}
          onSave={async () => {
            setShowCloseDialog(false);
            await rpc('shell/save_scene').catch(() => {});
            onCloseProject();
          }}
          onDiscard={() => { setShowCloseDialog(false); onCloseProject(); }}
          onCancel={() => setShowCloseDialog(false)}
        />
      )}
    </div>
  );
}
