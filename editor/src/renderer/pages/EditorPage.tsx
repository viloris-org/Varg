import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  fetchSceneGuides,
  openGameView,
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
  IconCheck,
  IconCode,
  IconFile,
  IconPlay,
  IconProjects,
  IconSparkles,
  IconX,
} from '../icons';

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

interface Props {
  onCloseProject: () => void;
  onOpenSettings?: () => void;
}

interface ArtifactSelection {
  kind: 'model' | 'code' | 'document';
  label: string;
  context: string;
  x: number;
  y: number;
}

function WorkspaceInspector({ object, onFocus, onPositionChange }: {
  object: SceneObject;
  onFocus: () => void;
  onPositionChange: (position: [number, number, number]) => Promise<void>;
}) {
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
          <span>{object.tag || 'Untagged entity'}</span>
        </div>
        <span className="workspace-live-badge">Live</span>
      </div>
      <label className="workspace-property-label">Position</label>
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
      <button onClick={onFocus}>Focus in viewport</button>
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

function ViewportCanvas({ sceneVersion = 0, cameraRef, onCameraChange, viewMode }: {
  sceneVersion?: number;
  cameraRef?: React.MutableRefObject<{
    yaw: number; pitch: number; distance: number;
    targetX: number; targetY: number; targetZ: number;
  }>;
  onCameraChange?: () => void;
  viewMode: '2d' | '3d';
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const sizeRef = useRef({ width: 640, height: 480 });
  const isActiveRef = useRef(true);
  const versionRef = useRef(sceneVersion);
  const lastRenderedVersionRef = useRef<number | null>(null);
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
          }
          const ctx = canvas.getContext('2d');
          if (ctx) {
            const imageData = new ImageData(
              new Uint8ClampedArray(uint8.buffer, uint8.byteOffset + 8, w * h * 4),
              w, h,
            );
            ctx.putImageData(imageData, 0, 0);
          }
        }
      } catch (e) {
        console.error('[viewport] readback error:', e);
      }
      setTimeout(poll, 100);
    };
    poll();
    return () => { isActiveRef.current = false; };
  }, [viewMode]);

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
          if (canvas) { canvas.width = Math.round(width); canvas.height = Math.round(height); }
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
      onCameraChange?.();
    };
    const handleMouseUp = () => { dragging.current = null; };
    const handleWheel = (e: WheelEvent) => {
      if (containerRef.current && containerRef.current.contains(e.target as Node)) {
        camRef.current.distance = Math.max(0.5, Math.min(100, camRef.current.distance + e.deltaY * 0.01));
        lastRenderedVersionRef.current = null;
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
      {viewMode === '3d' && <OrientationGizmo onSnapToAxis={(axis) => {
        switch (axis) {
          case 'top':    camRef.current.pitch = 1.5;  camRef.current.yaw = 0;     break;
          case 'bottom': camRef.current.pitch = -1.5; camRef.current.yaw = 0;     break;
          case 'left':   camRef.current.pitch = 0;    camRef.current.yaw = 1.5;   break;
          case 'right':  camRef.current.pitch = 0;    camRef.current.yaw = -1.5;  break;
          case 'front':  camRef.current.pitch = 0;    camRef.current.yaw = 0;     break;
          case 'back':   camRef.current.pitch = 0;    camRef.current.yaw = 3.14;  break;
        }
        lastRenderedVersionRef.current = null;
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

export default function EditorPage({ onCloseProject, onOpenSettings }: Props) {
  const { t } = useTranslation();

  // State
  const [shellState, setShellState] = useState<ShellState | null>(null);
  const [sceneTree, setSceneTree] = useState<SceneObject[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [sceneVersion, setSceneVersion] = useState(0);
  const [showCloseDialog, setShowCloseDialog] = useState(false);
  const [aiPanelWidth, setAiPanelWidth] = useState(() => {
    const saved = Number(window.localStorage.getItem('aster.aiPanelWidth'));
    return Number.isFinite(saved) && saved >= 360 && saved <= 720 ? saved : 440;
  });
  const [workspaceView, setWorkspaceView] = useState<'prd' | 'tasks' | 'game' | 'scripts'>('prd');
  const [aiWorkspace, setAiWorkspace] = useState<AiWorkspaceState | null>(null);
  const [scripts, setScripts] = useState<string[]>([]);
  const [selectedScript, setSelectedScript] = useState<string | null>(null);
  const [scriptContent, setScriptContent] = useState('');
  const [scriptLineRange, setScriptLineRange] = useState<[number, number] | null>(null);
  const [artifactSelection, setArtifactSelection] = useState<ArtifactSelection | null>(null);
  const [artifactQuestionOpen, setArtifactQuestionOpen] = useState(false);
  const [artifactQuestion, setArtifactQuestion] = useState('');
  const [contextualRequest, setContextualRequest] = useState<{ id: number; prompt: string } | null>(null);
  const [guides, setGuides] = useState<GuideEntity[]>([]);
  const [viewMode, setViewMode] = useState<'2d' | '3d'>('3d');
  const [viewportSize, setViewportSize] = useState({ width: 640, height: 480 });
  const [, setCameraRevision] = useState(0);
  const prevSceneVersionRef = useRef(0);
  const cameraRef = useRef({
    yaw: -0.5, pitch: 0.3, distance: 6,
    targetX: 0, targetY: 1, targetZ: 0,
  });

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
        setViewportSize({
          width: Math.round(rect.width),
          height: Math.round(rect.height),
        });
      }
    };

    updateSize();
    const observer = new ResizeObserver(updateSize);
    observer.observe(viewport);
    return () => observer.disconnect();
  }, []);

  const handleCameraChange = useCallback(() => {
    setCameraRevision(revision => revision + 1);
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

  useEffect(() => {
    if (!shellState?.has_project) return;
    rpc<{ entries: Array<{ path: string; kind: string }> }>('project/list_assets')
      .then(result => {
        const paths = result.entries
          .filter(entry => /script/i.test(entry.kind) || /\.(rhai|js|ts|lua)$/i.test(entry.path))
          .map(entry => entry.path);
        setScripts(paths);
        setSelectedScript(current => current && paths.includes(current) ? current : paths[0] ?? null);
      })
      .catch(() => setScripts([]));
  }, [sceneVersion, shellState?.has_project]);

  useEffect(() => {
    if (!selectedScript) {
      setScriptContent('');
      return;
    }
    rpc<{ content: string }>('project/read_file', { path: selectedScript })
      .then(result => setScriptContent(result.content))
      .catch(() => setScriptContent('// Unable to load this script.'));
  }, [selectedScript]);

  useEffect(() => {
    setScriptLineRange(null);
    setArtifactSelection(null);
    setArtifactQuestionOpen(false);
  }, [selectedScript, workspaceView]);

  // Track selected position
  useEffect(() => {
    if (selectedId) {
      const obj = sceneTree.find(o => o.id === selectedId);
      setSelectedPosition(obj ? ([...obj.position] as Vec3) : null);
    } else {
      setSelectedPosition(null);
    }
  }, [selectedId, sceneTree]);

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
    setAiPanelWidth((w) => Math.max(360, Math.min(w - delta, 720)));
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
      setSelectedId(hitId);
      rpc('shell/select_entity', { id: hitId });
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
      setSelectedId(null);
      rpc('shell/select_entity', {});
      setArtifactSelection(null);
    }
  }, [sceneTree, viewMode]);

  const selectDocumentText = useCallback((event: React.MouseEvent<HTMLElement>) => {
    const selection = window.getSelection();
    const text = selection?.toString().trim();
    if (!text || text.length < 2) return;
    setArtifactSelection({
      kind: 'document',
      label: text.length > 48 ? `${text.slice(0, 48)}…` : text,
      context: `PRD excerpt:\n${text}`,
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

  // Derive selected entity name
  const selectedEntityName = selectedId
    ? sceneTree.find(o => o.id === selectedId)?.name ?? null
    : null;
  const selectedObject = selectedId
    ? sceneTree.find(o => o.id === selectedId) ?? null
    : null;
  const taskSteps = ['Describe outcome', 'Inspect project', 'Review plan', 'Apply changes', 'Verify result'];
  const taskStepIndex = aiWorkspace?.status === 'thinking' ? 1
    : aiWorkspace?.status === 'ready' ? 2
      : aiWorkspace?.status === 'executing' ? 3
        : aiWorkspace?.status === 'complete' ? 4 : 0;

  // ── Render ──

  if (!shellState) {
    return <div className="loading">{t('loading_editor')}</div>;
  }

  return (
    <div className="editor editor-ai-first">
      {/* Minimal toolbar */}
      <div className="editor-toolbar editor-toolbar-minimal">
        <div className="toolbar-project">
          <span className="toolbar-project-kicker">Aster workspace</span>
          <span className="toolbar-project-name">{shellState.project_name || 'Untitled'}</span>
        </div>
        <span className="ai-only-badge">AI only</span>
        <button className="tool-btn play-btn" onClick={() => setWorkspaceView('game')} title="Open Game View"><IconPlay /></button>
        <button className="tool-btn" onClick={handleClose} title="Close"><IconX /></button>
      </div>

      {/* Main body: viewport + AI panel */}
      <div className="editor-body editor-body-ai">
        <main className="ai-product-workspace" ref={viewportMainRef}>
          <nav className="product-tabs" role="tablist" aria-label="Project outputs">
            {([
              ['prd', 'PRD', <IconFile key="prd" />],
              ['tasks', 'Tasks', <IconCheck key="tasks" />],
              ['game', 'Game View', <IconPlay key="game" />],
              ['scripts', 'Scripts', <IconCode key="scripts" />],
            ] as const).map(([view, label, icon]) => (
              <button key={view} className={workspaceView === view ? 'active' : ''} onClick={() => setWorkspaceView(view)} role="tab" aria-selected={workspaceView === view}>
                {icon}<span>{label}</span>
                {view === 'tasks' && aiWorkspace?.plan && <b>{aiWorkspace.plan.operations.length}</b>}
                {view === 'scripts' && scripts.length > 0 && <b>{scripts.length}</b>}
              </button>
            ))}
          </nav>

          <section className={`product-view product-view-${workspaceView}`}>
            {workspaceView === 'prd' && <article className="prd-document" onMouseUp={selectDocumentText}>
              <header><span>Product requirements</span><strong>{shellState.project_name || 'Untitled game'}</strong><p>Living brief maintained by Aster from the current project and conversation.</p></header>
              <section><h2>Vision</h2><p>Build a coherent, playable game experience through outcome-driven AI iteration. The project currently contains {sceneTree.length} scene objects and {scripts.length} scripts.</p></section>
              <section><h2>Current scope</h2><div className="prd-grid"><div><span>Player experience</span><strong>Playable core loop</strong></div><div><span>World</span><strong>{sceneTree.length} authored objects</strong></div><div><span>Automation</span><strong>Review before apply</strong></div><div><span>Delivery</span><strong>Game View verification</strong></div></div></section>
              <section><h2>Acceptance criteria</h2><ul><li>The game launches into a playable state.</li><li>AI-generated changes remain reviewable before write operations.</li><li>Every task ends with a Game View verification pass.</li><li>Scripts remain inspectable without exposing manual scene editing.</li></ul></section>
            </article>}

            {workspaceView === 'tasks' && <div className="task-board">
              <header><div><span>Execution plan</span><h1>{aiWorkspace?.status === 'idle' || !aiWorkspace ? 'Waiting for an outcome' : aiWorkspace.status}</h1></div><small>{shellState.project_name} · {sceneTree.length} objects</small></header>
              <ol className="task-progress">{taskSteps.map((step, index) => <li key={step} className={index < taskStepIndex ? 'complete' : index === taskStepIndex ? 'active' : ''}><span>{index < taskStepIndex ? <IconCheck /> : index + 1}</span><div><strong>{step}</strong><small>{index === taskStepIndex ? 'Current step' : index < taskStepIndex ? 'Complete' : 'Pending'}</small></div></li>)}</ol>
              <section className="task-operations"><div className="task-section-title"><span>Proposed operations</span></div>
                {!aiWorkspace?.plan ? <div className="product-empty"><IconProjects /><strong>No active plan</strong><span>Describe a result in chat. Aster's plan will appear here.</span></div> : aiWorkspace.plan.operations.map(operation => <div key={operation.index}><span className={operation.permission_kind}>{operation.permission_kind.toUpperCase()}</span><p>{operation.preview}</p><small>{operation.permission_kind === 'read' ? 'Auto allowed' : aiWorkspace.approved.has(operation.index) ? 'Allowed' : aiWorkspace.denied.has(operation.index) ? 'Denied once' : 'Awaiting permission in chat'}</small></div>)}
              </section>
              {aiWorkspace?.plan && <footer><button className="btn btn-ghost" onClick={aiWorkspace.discardProposal}>Discard</button><button className="btn btn-primary" disabled={aiWorkspace.approved.size === 0} onClick={aiWorkspace.applyApproved}>Continue with allowed ({aiWorkspace.approved.size})</button></footer>}
            </div>}

            {workspaceView === 'game' && <div className="game-preview"><div className="game-preview-bar"><div><span className="live-dot" />Live Game View</div><div className="viewport-mode-switch"><button className={viewMode === '2d' ? 'active' : ''} onClick={() => setViewMode('2d')}>2D</button><button className={viewMode === '3d' ? 'active' : ''} onClick={() => setViewMode('3d')}>3D</button><button onClick={openGameView}><IconPlay /> Run window</button></div></div><div className="game-preview-canvas" onClick={handleViewportClick}><ViewportCanvas sceneVersion={sceneVersion} cameraRef={cameraRef} onCameraChange={handleCameraChange} viewMode={viewMode} /></div></div>}

            {workspaceView === 'scripts' && <div className="script-preview"><aside><header>Project scripts <span>{scripts.length}</span></header>{scripts.length === 0 ? <p>No scripts found.</p> : scripts.map(path => <button key={path} className={selectedScript === path ? 'active' : ''} onClick={() => setSelectedScript(path)}><IconCode /><span>{path.split('/').pop()}</span><small>{path}</small></button>)}</aside><article><header><span>{selectedScript || 'Select a script'}</span><b>CLICK · SHIFT+CLICK TO SELECT LINES</b></header><pre className="selectable-code"><code>{(scriptContent || '// Aster-generated scripts will appear here.').split('\n').map((line, index) => { const lineNumber = index + 1; const selected = scriptLineRange && lineNumber >= scriptLineRange[0] && lineNumber <= scriptLineRange[1]; return <button key={lineNumber} className={selected ? 'selected' : ''} onClick={event => selectScriptLine(lineNumber, event.shiftKey, event)}><span>{lineNumber}</span><i>{line || ' '}</i></button>; })}</code></pre></article></div>}
          </section>

          {artifactSelection && <div className={`artifact-ask-popover ${artifactQuestionOpen ? 'expanded' : ''}`} style={{ left: artifactSelection.x, top: artifactSelection.y }}>
            {!artifactQuestionOpen ? <button onClick={() => setArtifactQuestionOpen(true)}><IconSparkles /> Ask Aster about {artifactSelection.kind}</button> : <div><header><span>{artifactSelection.label}</span><button onClick={() => setArtifactSelection(null)}><IconX /></button></header><div><input autoFocus value={artifactQuestion} onChange={event => setArtifactQuestion(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') submitArtifactQuestion(); if (event.key === 'Escape') setArtifactQuestionOpen(false); }} placeholder="Ask about this selection…" /><button onClick={submitArtifactQuestion} disabled={!artifactQuestion.trim()}>Ask</button></div></div>}
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
          <span className="status-item">{shellState.project_name || 'No project'}</span>
          <span className="status-divider" />
          <span className="status-item">{sceneTree.length} objects</span>
          {selectedEntityName && <><span className="status-divider" /><span className="status-item status-selection">Selected: {selectedEntityName}</span></>}
        </div>
        <div className="status-group">
          {shellState.scene_dirty ? (
            <span className="status-item status-dirty"><span className="status-dot" />Unsaved changes</span>
          ) : (
            <span className="status-item status-saved">Saved</span>
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
