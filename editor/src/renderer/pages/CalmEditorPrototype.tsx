import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  VargLogo,
  IconAlertCircle,
  IconAlertTriangle,
  IconBot,
  IconCheck,
  IconChevronDown,
  IconChevronRight,
  IconCode,
  IconEdit,
  IconFile,
  IconFolder,
  IconModel,
  IconMove,
  IconPackage,
  IconPlay,
  IconPlus,
  IconRedo,
  IconRotate,
  IconSave,
  IconScale,
  IconSearch,
  IconSend,
  IconSettings,
  IconSparkles,
  IconStop,
  IconTrash,
  IconUndo,
  IconView,
} from '../icons';
import {
  closeNativeSceneView,
  openWaylandEmbeddedCompositorSceneView,
  openNoCpuReadbackSceneView,
  openGameView,
  rpc,
  syncEditorCompositorViewport,
  syncWaylandEmbeddedCompositorViewport,
  syncNoCpuReadbackSceneView,
  viewportPresentationCapabilities,
  viewportPresentationStatus,
  type ViewportPresentationAdapter,
  type ViewportPresentationMode,
  type ViewportPresentationStatus,
  viewportReadback,
} from '../api';
import type { QuestEditorArtifact } from '../App';
import { OrientationGizmo, ViewportGrid } from './ViewportOverlays';
import {
  createOrthographicMatrix,
  createPerspectiveMatrix,
  createViewMatrix,
  projectToScreen,
} from './gizmoMath';
import AiPanel from './AiPanel';

interface CalmEditorPrototypeProps {
  onCloseProject: () => void;
  onOpenSettings?: () => void;
  onOpenQuest?: (questId?: string | null) => void;
  questArtifact?: QuestEditorArtifact | null;
  onDismissQuestArtifact?: () => void;
}

class AiPanelBoundary extends React.Component<
  { children: React.ReactNode },
  { error: string | null }
> {
  state = { error: null };

  static getDerivedStateFromError(error: unknown) {
    return { error: error instanceof Error ? error.message : String(error) };
  }

  render() {
    if (this.state.error) {
      return (
        <div className="m-3 rounded-[var(--radius-md)] border border-[rgba(248,113,113,0.28)] bg-[var(--danger-dim)] p-3 text-[12px] leading-5 text-[var(--danger)]">
          <div className="mb-1 font-semibold">AI panel failed to load</div>
          <div className="break-words font-mono text-[10px]">{this.state.error}</div>
        </div>
      );
    }
    return this.props.children;
  }
}

type NavSection = 'scene' | 'assets' | 'scripts' | 'build' | 'ai';
type TransformTool = 'select' | 'move' | 'rotate' | 'scale';
type BottomTab = 'console' | 'problems' | 'tasks' | 'assets';
type InspectorSection = 'transform' | 'script' | 'input' | 'physics' | 'render' | 'tags';
type ViewMode = '2d' | '3d';
type BuildTarget = 'windows-x64' | 'linux-x64' | 'macos-universal' | 'android-arm64' | 'ios-universal' | 'embedded-linux';
type BuildFormat = 'folder' | 'exe' | 'msi' | 'nsis' | 'appimage' | 'deb' | 'rpm' | 'dmg' | 'apk' | 'aab' | 'ipa' | 'ipk';

interface SceneEntity {
  id: string;
  name: string;
  type: string;
  group: string;
  visible: boolean;
  locked: boolean;
  position: [number, number, number];
  rotation: [number, number, number];
  scale: [number, number, number];
  parentId?: string | null;
  components?: Array<{ type: string; data: Record<string, unknown> }>;
}

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
  components: Array<{ type: string; data: Record<string, unknown> }>;
}

interface ProjectAssetMeta {
  guid: string;
  source_path: string;
  kind: string;
  importer: string;
}

interface EditorConsoleEntry {
  timestamp: number;
  level: string;
  subsystem: string;
  file?: string | null;
  line?: number | null;
  message: string;
}

function isWaylandEmbeddedCompositorPresentation(mode: ViewportPresentationMode | null) {
  return mode === 'wayland-embedded-compositor';
}

function isNoCpuReadbackAdapter(adapter?: ViewportPresentationAdapter) {
  return Boolean(adapter?.available && !adapter.cpu_readback && adapter.gpu_native_surface);
}

interface BuildTargetOption {
  id: BuildTarget;
  label: string;
  formats: BuildFormat[];
  status: 'ready' | 'planned' | 'blocked';
  note: string;
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

interface TextAssetDiagnostic {
  severity: string;
  message: string;
  line?: number;
  column?: number;
}

const mockSceneEntities: SceneEntity[] = [
  {
    id: 'world',
    name: 'World',
    type: 'SceneRoot',
    group: 'Core',
    visible: true,
    locked: true,
    position: [0, 0, 0],
    rotation: [0, 0, 0],
    scale: [1, 1, 1],
  },
  {
    id: 'lighting',
    name: 'Lighting',
    type: 'Environment',
    group: 'Core',
    visible: true,
    locked: false,
    position: [0, 4, 0],
    rotation: [45, 20, 0],
    scale: [1, 1, 1],
  },
  {
    id: 'player',
    name: 'PlayerController',
    type: 'Entity',
    group: 'Gameplay',
    visible: true,
    locked: false,
    position: [1.25, 0, -2.5],
    rotation: [0, 28, 0],
    scale: [1, 1, 1],
  },
  {
    id: 'camera',
    name: 'CameraRig',
    type: 'Camera',
    group: 'Gameplay',
    visible: true,
    locked: false,
    position: [-2, 2.2, 4.8],
    rotation: [-18, -24, 0],
    scale: [1, 1, 1],
  },
  {
    id: 'terrain',
    name: 'Terrain',
    type: 'Mesh',
    group: 'Environment',
    visible: true,
    locked: false,
    position: [0, -0.05, 0],
    rotation: [0, 0, 0],
    scale: [12, 1, 12],
  },
  {
    id: 'ui-root',
    name: 'UI Root',
    type: 'Canvas',
    group: 'Interface',
    visible: true,
    locked: false,
    position: [0, 0, 0],
    rotation: [0, 0, 0],
    scale: [1, 1, 1],
  },
];

const mockAssets = [
  { name: 'Meadow.scene', kind: 'scene', path: 'scenes/Meadow.scene', updated: '2m ago' },
  { name: 'PlayerController.varg', kind: 'script', path: 'scripts/PlayerController.varg', updated: '8m ago' },
  { name: 'ground_moss.mat', kind: 'material', path: 'materials/ground_moss.mat', updated: '18m ago' },
  { name: 'capsule_player.prefab', kind: 'prefab', path: 'prefabs/capsule_player.prefab', updated: '1h ago' },
  { name: 'ambient_forest.wav', kind: 'audio', path: 'audio/ambient_forest.wav', updated: '1d ago' },
];

const mockProblems = [
  {
    severity: 'warning',
    file: 'scripts/PlayerController.varg',
    line: 42,
    title: 'Input axis "dash" has no binding in the active profile.',
  },
  {
    severity: 'error',
    file: 'materials/ground_moss.mat',
    line: 8,
    title: 'Texture reference missing: textures/moss_detail.ktx2',
  },
];

const tasks = [
  { title: 'Bake navigation mesh', status: 'ready' },
  { title: 'Package Linux debug build', status: 'queued' },
  { title: 'Run scene validation', status: 'complete' },
];

const mockConsoleRows = [
  { level: 'info', source: 'runtime', message: 'Scene Meadow loaded in 84ms.' },
  { level: 'warn', source: 'assets', message: 'Material ground_moss uses fallback texture.' },
  { level: 'info', source: 'ecs', message: '42 entities, 118 components active.' },
];

const buildTargets: BuildTargetOption[] = [
  {
    id: 'linux-x64',
    label: 'Linux x64',
    formats: ['folder', 'appimage', 'deb', 'rpm'],
    status: 'planned',
    note: 'Local desktop export path; installer formats depend on host tooling.',
  },
  {
    id: 'windows-x64',
    label: 'Windows x64',
    formats: ['folder', 'exe', 'msi', 'nsis'],
    status: 'planned',
    note: 'Requires Windows runner or cross-build support for installers.',
  },
  {
    id: 'macos-universal',
    label: 'macOS Universal',
    formats: ['folder', 'dmg'],
    status: 'planned',
    note: 'Requires macOS signing/notarization for distributable builds.',
  },
  {
    id: 'android-arm64',
    label: 'Android ARM64',
    formats: ['apk', 'aab'],
    status: 'blocked',
    note: 'Needs Android runtime profile, SDK/NDK detection, and signing.',
  },
  {
    id: 'ios-universal',
    label: 'iOS Universal',
    formats: ['ipa'],
    status: 'blocked',
    note: 'Requires Apple toolchain, provisioning, and mobile runtime support.',
  },
  {
    id: 'embedded-linux',
    label: 'Embedded Linux',
    formats: ['ipk', 'folder'],
    status: 'blocked',
    note: 'Requires target device profile and install metadata.',
  },
];

const componentTypes = ['Camera', 'Light', 'MeshRenderer', 'RigidBody', 'Collider', 'Script', 'AudioSource'];
const pickRadiusPx = 30;
const viewportFovDeg = 60;
const editorViewportTargetFps = 75;
const editorViewportFrameMs = 1000 / editorViewportTargetFps;
const editorViewportMaxPixels = 1920 * 1080;
const interactiveViewportMaxPixels = 1280 * 720;
const playViewportMaxPixels = editorViewportMaxPixels;

function cx(...classes: Array<string | false | null | undefined>) {
  return classes.filter(Boolean).join(' ');
}

function viewportDevicePixelRatio() {
  return Math.min(Math.max(window.devicePixelRatio || 1, 1), 2);
}

function fitViewportReadbackSize(width: number, height: number, maxPixels = editorViewportMaxPixels) {
  const pixelRatio = viewportDevicePixelRatio();
  const roundedWidth = Math.max(1, Math.round(width * pixelRatio));
  const roundedHeight = Math.max(1, Math.round(height * pixelRatio));
  if (!maxPixels || roundedWidth * roundedHeight <= maxPixels) {
    return { width: roundedWidth, height: roundedHeight, scaled: false };
  }
  const scale = Math.sqrt(maxPixels / (roundedWidth * roundedHeight));
  return {
    width: Math.max(1, Math.round(roundedWidth * scale)),
    height: Math.max(1, Math.round(roundedHeight * scale)),
    scaled: true,
  };
}

function iconForNav(section: NavSection) {
  if (section === 'scene') return <IconView />;
  if (section === 'assets') return <IconPackage />;
  if (section === 'scripts') return <IconCode />;
  if (section === 'build') return <IconModel />;
  return <IconBot />;
}

function iconForTool(tool: TransformTool) {
  if (tool === 'move') return <IconMove />;
  if (tool === 'rotate') return <IconRotate />;
  if (tool === 'scale') return <IconScale />;
  return <IconView />;
}

function sceneObjectType(object: SceneObject): string {
  const tag = object.tag?.trim();
  if (!tag) return 'Entity';
  if (/camera/i.test(tag)) return 'Camera';
  if (/light/i.test(tag)) return 'Light';
  if (/mesh|model|terrain/i.test(tag)) return 'Mesh';
  if (/audio/i.test(tag)) return 'Audio';
  return tag;
}

function sceneObjectGroup(object: SceneObject): string {
  const tag = object.tag?.toLowerCase() ?? '';
  if (!object.parent_id) return 'Core';
  if (tag.includes('camera') || tag.includes('player') || tag.includes('script')) return 'Gameplay';
  if (tag.includes('light') || tag.includes('mesh') || tag.includes('terrain')) return 'Environment';
  return 'Scene';
}

function mapSceneObject(object: SceneObject): SceneEntity {
  return {
    id: object.id,
    name: object.name,
    type: sceneObjectType(object),
    group: sceneObjectGroup(object),
    visible: true,
    locked: false,
    position: object.position,
    rotation: [0, 0, 0],
    scale: [1, 1, 1],
    parentId: object.parent_id ?? null,
  };
}

function mapAsset(asset: ProjectAssetMeta) {
  return {
    name: asset.source_path.split('/').pop() || asset.source_path,
    kind: asset.kind,
    path: asset.source_path,
    updated: asset.importer || 'imported',
  };
}

function SectionHeader({
  title,
  open,
  onToggle,
  children,
}: {
  title: string;
  open: boolean;
  onToggle: () => void;
  children?: React.ReactNode;
}) {
  return (
    <div className="flex h-8 w-full items-center justify-between border-t border-[var(--border)] text-[12px] font-semibold text-[var(--text-secondary)]">
      <button
        type="button"
        className="flex h-full min-w-0 flex-1 items-center gap-2 px-3 text-left transition-colors hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
        onClick={onToggle}
      >
        {open ? <IconChevronDown size={13} /> : <IconChevronRight size={13} />}
        <span className="truncate">{title}</span>
      </button>
      {children && <div className="flex shrink-0 items-center pr-2">{children}</div>}
    </div>
  );
}

function NumberField({ value, onChange, label }: { value: number; onChange: (value: number) => void; label?: string }) {
  return (
    <input
      type="number"
      className="h-7 min-w-0 rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] px-2 font-mono text-[12px] text-[var(--text-primary)] outline-none transition-colors focus:border-[var(--brand)]"
      value={Number.isInteger(value) ? value : value.toFixed(2)}
      onChange={(event) => onChange(Number(event.target.value))}
      aria-label={label}
    />
  );
}

function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (checked: boolean) => void; label?: string }) {
  return (
    <button
      type="button"
      className={cx(
        'relative h-5 w-9 rounded-full border transition-colors',
        checked
          ? 'border-[rgba(34,197,94,0.4)] bg-[rgba(34,197,94,0.22)]'
          : 'border-[var(--border)] bg-[var(--bg-base)]',
      )}
      onClick={() => onChange(!checked)}
      aria-pressed={checked}
      aria-label={label}
    >
      <span
        className={cx(
          'absolute top-1/2 size-3.5 -translate-y-1/2 rounded-full transition-[left,background]',
          checked ? 'left-[18px] bg-[var(--brand)]' : 'left-[3px] bg-[var(--text-muted)]',
        )}
      />
    </button>
  );
}

function ViewportCanvas({
  sceneObjects,
  sceneVersion,
  selectedEntityId,
  viewMode,
  playMode,
  cameraRef,
  onCameraChange,
  onResize,
  onSelectEntity,
  inputOnly = false,
}: {
  sceneObjects: SceneObject[];
  sceneVersion: number;
  selectedEntityId: string | null;
  viewMode: ViewMode;
  playMode: boolean;
  cameraRef: React.MutableRefObject<{
    yaw: number;
    pitch: number;
    distance: number;
    targetX: number;
    targetY: number;
    targetZ: number;
  }>;
  onCameraChange: () => void;
  onResize: (size: { width: number; height: number }) => void;
  onSelectEntity: (id: string) => void;
  inputOnly?: boolean;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const contextRef = useRef<CanvasRenderingContext2D | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const sizeRef = useRef({ width: 640, height: 480 });
  const versionRef = useRef(sceneVersion);
  const activeRef = useRef(true);
  const lastRenderedVersionRef = useRef<number | null>(null);
  const fastPreviewUntilRef = useRef(0);
  const dirtyRef = useRef(true);
  const lastDrawWasScaledRef = useRef(false);
  const cameraRevisionFrameRef = useRef<number | null>(null);
  const dragging = useRef<'orbit' | 'pan' | null>(null);
  const dragStart = useRef({
    x: 0,
    y: 0,
    yaw: 0,
    pitch: 0,
    targetX: 0,
    targetY: 0,
    targetZ: 0,
  });

  if (versionRef.current !== sceneVersion) {
    versionRef.current = sceneVersion;
    dirtyRef.current = true;
  }

  const markViewportDirty = useCallback((resetVersion = true) => {
    dirtyRef.current = true;
    if (resetVersion) lastRenderedVersionRef.current = null;
  }, []);

  const scheduleCameraRevision = useCallback(() => {
    if (cameraRevisionFrameRef.current !== null) return;
    cameraRevisionFrameRef.current = window.requestAnimationFrame(() => {
      cameraRevisionFrameRef.current = null;
      onCameraChange();
    });
  }, [onCameraChange]);

  useEffect(() => {
    if (inputOnly) return undefined;
    activeRef.current = true;
    lastRenderedVersionRef.current = null;
    dirtyRef.current = true;

    const poll = async () => {
      if (!activeRef.current) return;
      const fastPreview = performance.now() < fastPreviewUntilRef.current;
      if (!dirtyRef.current && !playMode && !fastPreview) {
        if (lastDrawWasScaledRef.current) {
          markViewportDirty();
        } else {
          window.setTimeout(poll, 120);
          return;
        }
      }
      const { width, height } = fitViewportReadbackSize(
        sizeRef.current.width,
        sizeRef.current.height,
        fastPreview ? interactiveViewportMaxPixels : playMode ? playViewportMaxPixels : editorViewportMaxPixels,
      );
      const camera = cameraRef.current;
      const lastVersion = !playMode && !fastPreview
        ? lastRenderedVersionRef.current ?? undefined
        : undefined;
      dirtyRef.current = false;
      try {
        const buffer = await viewportReadback({
          width,
          height,
          lastVersion,
          yaw: camera.yaw,
          pitch: camera.pitch,
          distance: camera.distance,
          targetX: camera.targetX,
          targetY: camera.targetY,
          targetZ: camera.targetZ,
          viewMode,
          playMode,
          editorCamera: !playMode,
          entityId: selectedEntityId ?? undefined,
        });
        if (!activeRef.current || !canvasRef.current) return;
        const bytes = new Uint8Array(buffer);
        const header = new Uint32Array(bytes.buffer, bytes.byteOffset, 2);
        const widthFromBackend = header[0];
        const heightFromBackend = header[1];
        if (widthFromBackend > 0 && heightFromBackend > 0) {
          lastRenderedVersionRef.current = versionRef.current;
          const expected = fitViewportReadbackSize(
            sizeRef.current.width,
            sizeRef.current.height,
            fastPreview ? interactiveViewportMaxPixels : playMode ? playViewportMaxPixels : editorViewportMaxPixels,
          );
          lastDrawWasScaledRef.current = widthFromBackend !== expected.width || heightFromBackend !== expected.height;
          const canvas = canvasRef.current;
          if (canvas.width !== widthFromBackend || canvas.height !== heightFromBackend) {
            canvas.width = widthFromBackend;
            canvas.height = heightFromBackend;
            contextRef.current = null;
          }
          const context = contextRef.current ?? canvas.getContext('2d');
          contextRef.current = context;
          if (context) {
            const pixelOffset = bytes.byteOffset + 8;
            const pixelBytes = widthFromBackend * heightFromBackend * 4;
            context.putImageData(
              new ImageData(new Uint8ClampedArray(bytes.buffer, pixelOffset, pixelBytes), widthFromBackend, heightFromBackend),
              0,
              0,
            );
          }
        }
      } catch {
        // Browser preview intentionally falls through to the CSS fallback underneath.
      }
      window.setTimeout(poll, playMode || fastPreview ? editorViewportFrameMs : 120);
    };

    poll();
    return () => {
      activeRef.current = false;
      if (cameraRevisionFrameRef.current !== null) {
        window.cancelAnimationFrame(cameraRevisionFrameRef.current);
        cameraRevisionFrameRef.current = null;
      }
    };
  }, [cameraRef, inputOnly, markViewportDirty, playMode, selectedEntityId, viewMode]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const updateSize = (width: number, height: number) => {
      const next = { width: Math.round(width) || 640, height: Math.round(height) || 480 };
      sizeRef.current = next;
      onResize(next);
      markViewportDirty();
      const canvas = canvasRef.current;
      if (canvas) {
        const nextReadback = fitViewportReadbackSize(next.width, next.height);
        canvas.width = nextReadback.width;
        canvas.height = nextReadback.height;
        contextRef.current = null;
      }
    };
    const initial = container.getBoundingClientRect();
    updateSize(initial.width, initial.height);
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        updateSize(entry.contentRect.width, entry.contentRect.height);
      }
    });
    observer.observe(container);
    return () => observer.disconnect();
  }, [onResize]);

  const pickEntity = useCallback((clientX: number, clientY: number) => {
    const container = containerRef.current;
    if (!container) return null;
    const rect = container.getBoundingClientRect();
    const clickX = clientX - rect.left;
    const clickY = clientY - rect.top;
    const { width, height } = sizeRef.current;
    const camera = cameraRef.current;
    const viewMatrix = createViewMatrix(
      viewMode === '2d' ? 0 : camera.yaw,
      viewMode === '2d' ? 0 : camera.pitch,
      camera.distance,
      camera.targetX,
      camera.targetY,
      camera.targetZ,
    );
    const aspect = width / Math.max(height, 1);
    const projectionMatrix = viewMode === '2d'
      ? createOrthographicMatrix(camera.distance * 2, aspect, 0.01, 1000)
      : createPerspectiveMatrix((viewportFovDeg * Math.PI) / 180, aspect, 0.1, 1000);

    let bestId: string | null = null;
    let bestDistance = pickRadiusPx;
    let bestDepth = Infinity;
    for (const object of sceneObjects) {
      const screen = projectToScreen(object.position, viewMatrix, projectionMatrix, width, height);
      if (!screen) continue;
      const dx = screen.x - clickX;
      const dy = screen.y - clickY;
      const distance = Math.sqrt(dx * dx + dy * dy);
      if (distance < bestDistance || (distance === bestDistance && screen.depth < bestDepth)) {
        bestId = object.id;
        bestDistance = distance;
        bestDepth = screen.depth;
      }
    }
    return bestId;
  }, [cameraRef, sceneObjects, viewMode]);

  const selectedScreenPosition = useMemo(() => {
    const selected = selectedEntityId ? sceneObjects.find((object) => object.id === selectedEntityId) : null;
    if (!selected) return null;
    const { width, height } = sizeRef.current;
    const camera = cameraRef.current;
    const viewMatrix = createViewMatrix(
      viewMode === '2d' ? 0 : camera.yaw,
      viewMode === '2d' ? 0 : camera.pitch,
      camera.distance,
      camera.targetX,
      camera.targetY,
      camera.targetZ,
    );
    const projectionMatrix = viewMode === '2d'
      ? createOrthographicMatrix(camera.distance * 2, width / Math.max(height, 1), 0.01, 1000)
      : createPerspectiveMatrix((viewportFovDeg * Math.PI) / 180, width / Math.max(height, 1), 0.1, 1000);
    return projectToScreen(selected.position, viewMatrix, projectionMatrix, width, height);
  }, [
    cameraRef,
    sceneObjects,
    selectedEntityId,
    sceneVersion,
    viewMode,
  ]);

  const handleMouseDown = useCallback((event: React.MouseEvent) => {
    if (event.button === 0) {
      const id = pickEntity(event.clientX, event.clientY);
      if (id) onSelectEntity(id);
      return;
    }
    if (event.button === 2) {
      dragging.current = viewMode === '2d' ? 'pan' : 'orbit';
    } else if (event.button === 1) {
      dragging.current = 'pan';
    } else {
      return;
    }
    dragStart.current = { x: event.clientX, y: event.clientY, ...cameraRef.current };
    event.preventDefault();
  }, [cameraRef, onSelectEntity, pickEntity, viewMode]);

  useEffect(() => {
    const handleMouseMove = (event: MouseEvent) => {
      if (!dragging.current) return;
      const dx = event.clientX - dragStart.current.x;
      const dy = event.clientY - dragStart.current.y;
      const camera = cameraRef.current;
      if (dragging.current === 'orbit') {
        camera.yaw = dragStart.current.yaw - dx * 0.005;
        camera.pitch = Math.max(-1.5, Math.min(1.5, dragStart.current.pitch + dy * 0.005));
      } else {
        const distanceScale = camera.distance * 0.002;
        const yaw = camera.yaw;
        camera.targetX = dragStart.current.targetX + (-dx * Math.cos(yaw) - dy * Math.sin(yaw) * 0.5) * distanceScale;
        camera.targetY = dragStart.current.targetY + dy * distanceScale * 0.5;
        camera.targetZ = dragStart.current.targetZ + (dx * Math.sin(yaw) - dy * Math.cos(yaw) * 0.5) * distanceScale;
      }
      lastRenderedVersionRef.current = null;
      markViewportDirty();
      fastPreviewUntilRef.current = performance.now() + 180;
      scheduleCameraRevision();
    };
    const handleMouseUp = () => {
      dragging.current = null;
    };
    const handleWheel = (event: WheelEvent) => {
      if (!containerRef.current?.contains(event.target as Node)) return;
      cameraRef.current.distance = Math.max(0.5, Math.min(100, cameraRef.current.distance + event.deltaY * 0.01));
      markViewportDirty();
      fastPreviewUntilRef.current = performance.now() + 180;
      scheduleCameraRevision();
      event.preventDefault();
    };
    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
    window.addEventListener('wheel', handleWheel, { passive: false });
    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);
      window.removeEventListener('wheel', handleWheel);
    };
  }, [cameraRef, markViewportDirty, scheduleCameraRevision]);

  return (
    <div
      ref={containerRef}
      className={cx(
        'absolute inset-0 cursor-crosshair overflow-hidden',
        inputOnly
          ? 'z-[2] bg-transparent'
          : 'bg-[radial-gradient(circle_at_50%_18%,rgba(96,165,250,0.10),transparent_32%),linear-gradient(180deg,#101721_0%,#081018_55%,#070A0F_100%)]',
      )}
      onMouseDown={handleMouseDown}
      onContextMenu={(event) => event.preventDefault()}
    >
      {!inputOnly && <ViewportGrid />}
      {!inputOnly && <canvas ref={canvasRef} className="relative z-[1] h-full w-full object-fill" />}
      {selectedScreenPosition && (
        <div
          className="pointer-events-none absolute z-[3] -translate-x-1/2 -translate-y-1/2"
          style={{ left: selectedScreenPosition.x, top: selectedScreenPosition.y }}
        >
          <div className="size-9 rounded-full border-2 border-[var(--brand)] opacity-90 shadow-[0_0_18px_rgba(34,197,94,0.36)]" />
        </div>
      )}
      {viewMode === '3d' && (
        <OrientationGizmo
          camera={cameraRef.current}
          onSnapToAxis={(axis) => {
            const camera = cameraRef.current;
            if (axis === 'top') {
              camera.pitch = 1.5;
              camera.yaw = 0;
            } else if (axis === 'bottom') {
              camera.pitch = -1.5;
              camera.yaw = 0;
            } else if (axis === 'left') {
              camera.pitch = 0;
              camera.yaw = 1.5;
            } else if (axis === 'right') {
              camera.pitch = 0;
              camera.yaw = -1.5;
            } else if (axis === 'front') {
              camera.pitch = 0;
              camera.yaw = 0;
            } else {
              camera.pitch = 0;
              camera.yaw = 3.14;
            }
            markViewportDirty();
            fastPreviewUntilRef.current = performance.now() + 180;
            scheduleCameraRevision();
          }}
        />
      )}
    </div>
  );
}

function FieldRow({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="grid min-h-8 grid-cols-[78px_minmax(0,1fr)] items-center gap-3 px-3 py-1.5 text-[12px]">
      <span className="truncate text-[var(--text-muted)]">{label}</span>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

function isVec3Record(value: unknown): value is { x: number; y: number; z: number } {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const record = value as Record<string, unknown>;
  return typeof record.x === 'number' && typeof record.y === 'number' && typeof record.z === 'number';
}

function isColorField(name: string): boolean {
  return name === 'color' || name.endsWith('_color');
}

function vec3ToHex(value: { x: number; y: number; z: number }): string {
  return `#${[value.x, value.y, value.z]
    .map((channel) => Math.round(Math.max(0, Math.min(1, channel)) * 255).toString(16).padStart(2, '0'))
    .join('')}`;
}

function hexToVec3(hex: string): { x: number; y: number; z: number } | null {
  const match = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!match) return null;
  const value = match[1];
  return {
    x: parseInt(value.slice(0, 2), 16) / 255,
    y: parseInt(value.slice(2, 4), 16) / 255,
    z: parseInt(value.slice(4, 6), 16) / 255,
  };
}

const hierarchyContextMenuClass = {
  root: 'fixed z-[1000] min-w-[156px] rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-surface)] py-1 shadow-[var(--shadow-lg)]',
  item: 'flex min-h-8 w-full cursor-pointer items-center gap-2 border-0 bg-transparent px-3 text-left text-[12px] text-[var(--text-primary)] hover:bg-[var(--bg-hover)]',
  danger: 'text-[var(--danger)] hover:bg-[var(--danger-dim)]',
  separator: 'mx-2 my-1 h-px bg-[var(--border)]',
};

function ComponentField({
  name,
  value,
  onChange,
}: {
  name: string;
  value: unknown;
  onChange: (value: unknown) => void;
}) {
  if (typeof value === 'boolean') {
    return (
      <FieldRow label={name}>
        <Toggle checked={value} onChange={onChange} label={name} />
      </FieldRow>
    );
  }
  if (typeof value === 'number') {
    return (
      <FieldRow label={name}>
        <NumberField value={value} onChange={onChange} label={name} />
      </FieldRow>
    );
  }
  if (typeof value === 'string') {
    return (
      <FieldRow label={name}>
        <input
          className="h-7 w-full rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] px-2 text-[12px] text-[var(--text-primary)] outline-none focus:border-[var(--brand)]"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          aria-label={name}
        />
      </FieldRow>
    );
  }
  if (isVec3Record(value)) {
    if (isColorField(name)) {
      const hex = vec3ToHex(value);
      const commitHex = (rawHex: string) => {
        const next = hexToVec3(rawHex);
        if (next) onChange(next);
      };

      return (
        <FieldRow label={name}>
          <div className="grid gap-1.5">
            <div className="flex items-center gap-2">
              <label className="relative grid size-7 shrink-0 cursor-pointer place-items-center overflow-hidden rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)]" title="Pick color">
                <input
                  type="color"
                  className="absolute inset-0 cursor-pointer opacity-0"
                  value={hex}
                  onChange={(event) => commitHex(event.target.value)}
                  aria-label={`${name} color`}
                />
                <span className="size-full" style={{ backgroundColor: hex }} />
              </label>
              <input
                key={hex}
                className="h-7 min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] px-2 font-mono text-[12px] text-[var(--text-primary)] outline-none focus:border-[var(--brand)]"
                defaultValue={hex}
                spellCheck={false}
                aria-label={`${name} hex`}
                onBlur={(event) => commitHex(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') event.currentTarget.blur();
                  if (event.key === 'Escape') event.currentTarget.blur();
                }}
              />
            </div>
            <div className="grid grid-cols-3 gap-1.5">
              {(['x', 'y', 'z'] as const).map((axis, index) => (
                <label key={axis} className="grid grid-cols-[14px_minmax(0,1fr)] items-center gap-1 rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] px-1.5 py-1">
                  <span className="font-mono text-[10px] text-[var(--text-muted)]">{['R', 'G', 'B'][index]}</span>
                  <input
                    key={`${axis}-${value[axis]}`}
                    className="min-w-0 border-0 bg-transparent p-0 text-right font-mono text-[11px] text-[var(--text-primary)] outline-none"
                    defaultValue={value[axis].toFixed(2)}
                    inputMode="decimal"
                    aria-label={`${name} ${['red', 'green', 'blue'][index]}`}
                    onBlur={(event) => {
                      const next = Number(event.target.value);
                      if (Number.isFinite(next)) onChange({ ...value, [axis]: next });
                    }}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter') event.currentTarget.blur();
                      if (event.key === 'Escape') event.currentTarget.blur();
                    }}
                  />
                </label>
              ))}
            </div>
          </div>
        </FieldRow>
      );
    }

    return (
      <FieldRow label={name}>
        <div className="grid grid-cols-3 gap-1.5">
          {(['x', 'y', 'z'] as const).map((axis) => (
            <label key={axis} className="grid grid-cols-[16px_minmax(0,1fr)] items-center gap-1 rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] px-1.5 py-1">
              <span className="font-mono text-[10px] text-[var(--text-muted)]">{axis.toUpperCase()}</span>
              <input
                key={`${axis}-${value[axis]}`}
                className="min-w-0 border-0 bg-transparent p-0 text-right font-mono text-[11px] text-[var(--text-primary)] outline-none"
                defaultValue={value[axis].toFixed(2)}
                inputMode="decimal"
                aria-label={`${name} ${axis}`}
                onBlur={(event) => {
                  const next = Number(event.target.value);
                  if (Number.isFinite(next)) onChange({ ...value, [axis]: next });
                }}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') event.currentTarget.blur();
                  if (event.key === 'Escape') event.currentTarget.blur();
                }}
              />
            </label>
          ))}
        </div>
      </FieldRow>
    );
  }
  return (
    <FieldRow label={name}>
      <textarea
        className="h-16 w-full resize-none rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] px-2 py-1 font-mono text-[11px] text-[var(--text-secondary)] outline-none focus:border-[var(--brand)]"
        value={JSON.stringify(value ?? null, null, 2)}
        spellCheck={false}
        aria-label={name}
        onChange={(event) => {
          try {
            onChange(JSON.parse(event.target.value));
          } catch {
            onChange(event.target.value);
          }
        }}
      />
    </FieldRow>
  );
}

export default function CalmEditorPrototype({
  onCloseProject,
  onOpenSettings,
  onOpenQuest,
  questArtifact,
  onDismissQuestArtifact,
}: CalmEditorPrototypeProps) {
  const [activeNav, setActiveNav] = useState<NavSection>('scene');
  const [selectedEntityId, setSelectedEntityId] = useState('player');
  const [tool, setTool] = useState<TransformTool>('move');
  const [bottomTab, setBottomTab] = useState<BottomTab>('problems');
  const [commandOpen, setCommandOpen] = useState(false);
  const [isPlaying, setIsPlaying] = useState(false);
  const [backendReady, setBackendReady] = useState(false);
  const [shellState, setShellState] = useState<ShellState | null>(null);
  const [sceneVersion, setSceneVersion] = useState(0);
  const [sceneSearch, setSceneSearch] = useState('');
  const [assetSearch, setAssetSearch] = useState('');
  const [snapEnabled, setSnapEnabled] = useState(true);
  const [showDrawer, setShowDrawer] = useState(true);
  const [aiPrompt, setAiPrompt] = useState('');
  const [contextualRequest, setContextualRequest] = useState<{ id: number; prompt: string } | null>(null);
  const [viewMode, setViewMode] = useState<ViewMode>('3d');
  const [viewportSize, setViewportSize] = useState({ width: 640, height: 480 });
  const [cameraRevision, setCameraRevision] = useState(0);
  const [viewportPresentation, setViewportPresentation] = useState<ViewportPresentationMode | null>(null);
  const [viewportPresentationAdapters, setViewportPresentationAdapters] = useState<ViewportPresentationAdapter[]>([]);
  const [viewportPresentationDiagnostics, setViewportPresentationDiagnostics] = useState<ViewportPresentationStatus | null>(null);
  const [nativeSceneError, setNativeSceneError] = useState<string | null>(null);
  const [selectedScript, setSelectedScript] = useState<string | null>(null);
  const [scriptContent, setScriptContent] = useState('');
  const [scriptSavedContent, setScriptSavedContent] = useState('');
  const [scriptDiagnostics, setScriptDiagnostics] = useState<TextAssetDiagnostic[]>([]);
  const [scriptSaving, setScriptSaving] = useState(false);
  const [buildTarget, setBuildTarget] = useState<BuildTarget>('linux-x64');
  const [buildFormat, setBuildFormat] = useState<BuildFormat>('folder');
  const [buildChannel, setBuildChannel] = useState<'debug' | 'release'>('debug');
  const [buildBusy, setBuildBusy] = useState(false);
  const [buildMessage, setBuildMessage] = useState<string | null>(null);
  const [addComponentType, setAddComponentType] = useState('Camera');
  const [hierarchyContextMenu, setHierarchyContextMenu] = useState<{
    x: number;
    y: number;
    entity: SceneEntity;
  } | null>(null);
  const [openInspector, setOpenInspector] = useState<Record<InspectorSection, boolean>>({
    transform: true,
    script: true,
    input: true,
    physics: false,
    render: false,
    tags: true,
  });
  const [entities, setEntities] = useState(mockSceneEntities);
  const [projectAssets, setProjectAssets] = useState(mockAssets);
  const [consoleEntries, setConsoleEntries] = useState<EditorConsoleEntry[]>([]);
  const [selectedEntityDetails, setSelectedEntityDetails] = useState<EntityDetails | null>(null);
  const sceneVersionRef = useRef(0);
  const viewportFrameRef = useRef<HTMLDivElement>(null);
  const cameraRef = useRef({
    yaw: -0.5,
    pitch: 0.3,
    distance: 6,
    targetX: 0,
    targetY: 1,
    targetZ: 0,
  });

  const selectedEntity = entities.find((entity) => entity.id === selectedEntityId) ?? entities[0];
  const effectivePresentationAdapters = viewportPresentationDiagnostics?.adapters ?? viewportPresentationAdapters;
  const viewportPresentationAdapter = viewportPresentation
    ? effectivePresentationAdapters.find((adapter) => adapter.mode === viewportPresentation)
    : undefined;
  const noCpuReadbackPresentation = isNoCpuReadbackAdapter(viewportPresentationAdapter);
  const inspectorEntity = selectedEntityDetails
    ? {
      ...selectedEntity,
      name: selectedEntityDetails.name,
      type: selectedEntityDetails.tag || selectedEntity.type,
      position: selectedEntityDetails.transform.position,
      rotation: selectedEntityDetails.transform.rotation.slice(0, 3) as [number, number, number],
      scale: selectedEntityDetails.transform.scale,
      components: selectedEntityDetails.components,
    }
    : selectedEntity;
  const filteredEntities = entities.filter((entity) => {
    const query = sceneSearch.trim().toLowerCase();
    if (!query) return true;
    return `${entity.name} ${entity.type} ${entity.group}`.toLowerCase().includes(query);
  });
  const groupedEntities = useMemo(() => {
    return filteredEntities.reduce<Record<string, SceneEntity[]>>((groups, entity) => {
      groups[entity.group] = [...(groups[entity.group] ?? []), entity];
      return groups;
    }, {});
  }, [filteredEntities]);
  const filteredAssets = projectAssets.filter((asset) => {
    const query = assetSearch.trim().toLowerCase();
    if (!query) return true;
    return `${asset.name} ${asset.kind} ${asset.path}`.toLowerCase().includes(query);
  });
  const scriptAssets = useMemo(() => (
    projectAssets
      .filter((asset) => /script|model/i.test(asset.kind) || /\.(varg|vscene|vasset|amdl|js|ts|lua)$/i.test(asset.path))
      .map((asset) => asset.path)
  ), [projectAssets]);
  const selectedBuildTarget = buildTargets.find((target) => target.id === buildTarget) ?? buildTargets[0];
  const problemRows = consoleEntries
    .filter((entry) => /error|warn/i.test(entry.level))
    .map((entry) => ({
      severity: /error/i.test(entry.level) ? 'error' : 'warning',
      file: entry.file ?? entry.subsystem,
      line: entry.line ?? 0,
      title: entry.message,
    }));
  const visibleProblems = problemRows.length > 0 ? problemRows : mockProblems;
  const sceneObjects = useMemo<SceneObject[]>(() => entities.map((entity) => ({
    id: entity.id,
    name: entity.name,
    tag: entity.type,
    position: entity.position,
    parent_id: entity.parentId ?? null,
  })), [entities]);

  const refreshSceneTree = async () => {
    try {
      const [state, tree] = await Promise.all([
        rpc<ShellState>('shell/get_state'),
        rpc<{ objects: SceneObject[] }>('shell/get_scene_tree'),
      ]);
      setBackendReady(true);
      setShellState(state);
      const nextSceneVersion = state.scene_version ?? 0;
      sceneVersionRef.current = nextSceneVersion;
      setSceneVersion(nextSceneVersion);
      const nextEntities = tree.objects.map(mapSceneObject);
      setEntities(nextEntities.length > 0 ? nextEntities : mockSceneEntities);
      setSelectedEntityId((current) => {
        if (nextEntities.some((entity) => entity.id === current)) return current;
        return nextEntities[0]?.id ?? current;
      });
    } catch {
      setBackendReady(false);
    }
  };

  const refreshAssets = async () => {
    try {
      const result = await rpc<{ entries: Array<{ path: string; kind: string }>; assets: ProjectAssetMeta[] }>('project/list_assets');
      setProjectAssets(result.assets.map(mapAsset));
    } catch {
      setProjectAssets(mockAssets);
    }
  };

  const refreshConsole = async () => {
    try {
      const result = await rpc<{ entries: EditorConsoleEntry[] }>('console/get_entries');
      setConsoleEntries(result.entries);
    } catch {
      setConsoleEntries([]);
    }
  };

  const refreshShellState = async () => {
    try {
      const state = await rpc<ShellState>('shell/get_state');
      setBackendReady(true);
      setShellState(state);
      const nextSceneVersion = state.scene_version ?? 0;
      if (nextSceneVersion !== sceneVersionRef.current) {
        await refreshSceneTree();
      }
    } catch {
      setBackendReady(false);
    }
  };

  const handleCameraChange = useCallback(() => {
    window.requestAnimationFrame(() => setCameraRevision((revision) => revision + 1));
  }, []);

  const handleViewportResize = useCallback((size: { width: number; height: number }) => {
    setViewportSize((current) => (
      current.width === size.width && current.height === size.height ? current : size
    ));
  }, []);

  const embeddedSceneViewport = useCallback(() => {
    const rect = viewportFrameRef.current?.getBoundingClientRect();
    if (!rect || rect.width <= 0 || rect.height <= 0) return null;
    return {
      x: Math.round(rect.left),
      y: Math.round(rect.top),
      width: Math.max(1, Math.round(rect.width)),
      height: Math.max(1, Math.round(rect.height)),
    };
  }, []);

  const openNativeSceneViewport = useCallback(async () => {
    if (!viewportPresentation) throw new Error('No native Scene View adapter is available.');
    const viewport = embeddedSceneViewport();
    if (!viewport) throw new Error('Scene View frame is not ready yet.');
    const camera = cameraRef.current;
    const openSceneView = isWaylandEmbeddedCompositorPresentation(viewportPresentation)
      ? openWaylandEmbeddedCompositorSceneView
      : openNoCpuReadbackSceneView;
    await openSceneView({
      viewport,
      yaw: camera.yaw,
      pitch: camera.pitch,
      distance: camera.distance,
      targetX: camera.targetX,
      targetY: camera.targetY,
      targetZ: camera.targetZ,
    });
  }, [embeddedSceneViewport, viewportPresentation]);

  const zeroCopySceneActive = backendReady
    && noCpuReadbackPresentation
    && viewMode === '3d'
    && !isPlaying
    && !nativeSceneError;

  const nativeHostSceneActive = zeroCopySceneActive && noCpuReadbackPresentation;

  const setSelectedTransform = async (axis: 'position' | 'rotation' | 'scale', index: number, value: number) => {
    setEntities((current) =>
      current.map((entity) => {
        if (entity.id !== inspectorEntity.id) return entity;
        const next = [...entity[axis]] as [number, number, number];
        next[index] = value;
        return { ...entity, [axis]: next };
      }),
    );
    if (selectedEntityDetails) {
      setSelectedEntityDetails((current) => {
        if (!current) return current;
        const source = axis === 'rotation'
          ? current.transform.rotation.slice(0, 3)
          : current.transform[axis];
        const next = [...source] as [number, number, number];
        next[index] = value;
        return {
          ...current,
          transform: {
            ...current.transform,
            [axis]: axis === 'rotation'
              ? ([next[0], next[1], next[2], current.transform.rotation[3]] as [number, number, number, number])
              : next,
          },
        };
      });
    }
    if (backendReady) {
      const payloadValue = axis === 'rotation' && selectedEntityDetails
        ? (() => {
          const next = [...selectedEntityDetails.transform.rotation] as [number, number, number, number];
          next[index] = value;
          return next;
        })()
        : (() => {
          const next = [...inspectorEntity[axis]] as [number, number, number];
          next[index] = value;
          return next;
        })();
      await rpc('shell/update_transform', { id: inspectorEntity.id, [axis]: payloadValue }).catch(() => {});
      await refreshSceneTree();
    }
  };

  const toggleInspector = (section: InspectorSection) => {
    setOpenInspector((current) => ({ ...current, [section]: !current[section] }));
  };

  const updateComponentField = async (componentType: string, fieldName: string, value: unknown) => {
    setSelectedEntityDetails((current) => {
      if (!current) return current;
      return {
        ...current,
        components: current.components.map((component) => (
          component.type === componentType
            ? { ...component, data: { ...component.data, [fieldName]: value } }
            : component
        )),
      };
    });
    if (backendReady) {
      await rpc('shell/update_component', {
        id: inspectorEntity.id,
        component_type: componentType,
        data: { [fieldName]: value },
      }).catch(() => {});
      await refreshSceneTree();
    }
  };

  const addComponent = async () => {
    if (!backendReady || !inspectorEntity?.id) return;
    await rpc('shell/add_component', { id: inspectorEntity.id, component_type: addComponentType }).catch(() => {});
    await refreshSceneTree();
  };

  const removeComponent = async (componentType: string) => {
    if (!backendReady || !inspectorEntity?.id) return;
    await rpc('shell/remove_component', { id: inspectorEntity.id, component_type: componentType }).catch(() => {});
    await refreshSceneTree();
  };

  const openScript = (path: string) => {
    setSelectedScript(path);
    setActiveNav('scripts');
  };

  const saveScript = async () => {
    if (!backendReady || !selectedScript) return;
    setScriptSaving(true);
    try {
      await rpc('project/write_file', { path: selectedScript, content: scriptContent });
      setScriptSavedContent(scriptContent);
      await refreshConsole();
    } finally {
      setScriptSaving(false);
    }
  };

  const runBuild = async () => {
    if (!backendReady) {
      setBuildMessage('Build packaging needs the Tauri backend. Browser preview is showing prototype data.');
      return;
    }
    setBuildBusy(true);
    setBuildMessage(null);
    try {
      const result = await rpc<BuildPackageResult>('project/package', {
        target: buildTarget,
        format: buildFormat,
        channel: buildChannel,
        optimize_assets: true,
        include_debug_symbols: false,
      });
      setBuildMessage([
        `Packaged ${result.project}.`,
        `Target: ${result.target}`,
        `Format: ${result.format}`,
        `Channel: ${result.channel}`,
        `Output: ${result.path}`,
      ].join('\n'));
      await refreshConsole();
    } catch (error) {
      setBuildMessage(`Package failed.\n${error instanceof Error ? error.message : String(error)}`);
      await refreshConsole().catch(() => {});
    } finally {
      setBuildBusy(false);
    }
  };

  const navItems: Array<{ id: NavSection; label: string }> = [
    { id: 'scene', label: 'Scene' },
    { id: 'assets', label: 'Assets' },
    { id: 'scripts', label: 'Scripts' },
    { id: 'build', label: 'Build' },
    { id: 'ai', label: 'AI' },
  ];

  const commands = [
    { title: 'Create Camera', detail: 'Add a camera object to Meadow.scene' },
    { title: 'Open Selected Script', detail: selectedScript ?? 'No script selected' },
    { title: 'Run Scene Validation', detail: 'Check components, assets, and policy rules' },
    { title: 'Package Current Build', detail: `${selectedBuildTarget.label} / ${buildFormat} / ${buildChannel}` },
    { title: isPlaying ? 'Stop Play Mode' : 'Enter Play Mode', detail: 'Run Meadow.scene in editor' },
  ];

  useEffect(() => {
    refreshSceneTree();
    refreshAssets();
    refreshConsole();
    viewportPresentationCapabilities()
      .then((capabilities) => {
        setViewportPresentationAdapters(capabilities.adapters);
        setViewportPresentation(capabilities.default_mode);
      })
      .catch(() => {});
    viewportPresentationStatus()
      .then((status) => {
        setViewportPresentationDiagnostics(status);
        if (status.adapters) setViewportPresentationAdapters(status.adapters);
        setViewportPresentation(status.default_mode ?? null);
      })
      .catch(() => {
        setViewportPresentationDiagnostics(null);
      });
    const interval = window.setInterval(() => {
      refreshShellState();
      refreshConsole();
    }, 2000);
    return () => window.clearInterval(interval);
  }, []);

  useEffect(() => {
    if (!hierarchyContextMenu) return undefined;
    const close = () => setHierarchyContextMenu(null);
    window.addEventListener('click', close);
    window.addEventListener('keydown', close);
    return () => {
      window.removeEventListener('click', close);
      window.removeEventListener('keydown', close);
    };
  }, [hierarchyContextMenu]);

  useEffect(() => {
    if (
      !backendReady
      || !noCpuReadbackPresentation
      || viewMode !== '3d'
      || isPlaying
    ) return;
    let cancelled = false;
    openNativeSceneViewport()
      .then(() => {
        if (!cancelled) setNativeSceneError(null);
      })
      .catch((error) => {
        if (cancelled) return;
        setNativeSceneError(error instanceof Error ? error.message : String(error));
      });
    return () => {
      cancelled = true;
    };
  }, [backendReady, isPlaying, noCpuReadbackPresentation, openNativeSceneViewport, sceneVersion, viewportPresentation, viewMode]);

  useEffect(() => {
    if (!zeroCopySceneActive) return;
    const frame = viewportFrameRef.current;
    if (!frame) return;
    let raf: number | null = null;
    const syncViewport = () => {
      if (raf !== null) window.cancelAnimationFrame(raf);
      raf = window.requestAnimationFrame(() => {
        raf = null;
        const viewport = embeddedSceneViewport();
        if (!viewport) return;
        const camera = cameraRef.current;
        const syncPromise = isWaylandEmbeddedCompositorPresentation(viewportPresentation)
          ? syncWaylandEmbeddedCompositorViewport({ viewport })
          : syncNoCpuReadbackSceneView({
            viewport,
            yaw: camera.yaw,
            pitch: camera.pitch,
            distance: camera.distance,
            targetX: camera.targetX,
            targetY: camera.targetY,
            targetZ: camera.targetZ,
          });
        syncPromise.catch((error) => {
          setNativeSceneError(error instanceof Error ? error.message : String(error));
        });
      });
    };
    const observer = new ResizeObserver(syncViewport);
    observer.observe(frame);
    window.addEventListener('resize', syncViewport);
    window.addEventListener('scroll', syncViewport, true);
    syncViewport();
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', syncViewport);
      window.removeEventListener('scroll', syncViewport, true);
      if (raf !== null) window.cancelAnimationFrame(raf);
    };
  }, [cameraRevision, embeddedSceneViewport, viewportPresentation, zeroCopySceneActive]);

  useEffect(() => {
    if (!backendReady) return;
    const frame = viewportFrameRef.current;
    if (!frame) return;
    let raf: number | null = null;
    const syncViewport = () => {
      if (raf !== null) window.cancelAnimationFrame(raf);
      raf = window.requestAnimationFrame(() => {
        raf = null;
        const viewport = embeddedSceneViewport();
        if (!viewport) return;
        const syncCompositorViewport = isWaylandEmbeddedCompositorPresentation(viewportPresentation)
          ? syncWaylandEmbeddedCompositorViewport
          : syncEditorCompositorViewport;
        syncCompositorViewport({ viewport }).catch(() => {});
      });
    };
    const observer = new ResizeObserver(syncViewport);
    observer.observe(frame);
    window.addEventListener('resize', syncViewport);
    window.addEventListener('scroll', syncViewport, true);
    syncViewport();
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', syncViewport);
      window.removeEventListener('scroll', syncViewport, true);
      if (raf !== null) window.cancelAnimationFrame(raf);
    };
  }, [backendReady, embeddedSceneViewport, viewportPresentation]);

  useEffect(() => {
    if (
      noCpuReadbackPresentation
      && viewMode === '3d'
      && !isPlaying
      && !nativeSceneError
    ) return;
    closeNativeSceneView().catch(() => {});
  }, [isPlaying, nativeSceneError, noCpuReadbackPresentation, viewportPresentation, viewMode]);

  useEffect(() => {
    setSelectedScript((current) => {
      if (current && scriptAssets.includes(current)) return current;
      return scriptAssets[0] ?? null;
    });
  }, [scriptAssets]);

  useEffect(() => {
    if (!selectedScript || !backendReady) {
      setScriptContent(selectedScript ? '// Script preview is available in the desktop editor.' : '');
      setScriptSavedContent('');
      setScriptDiagnostics([]);
      return;
    }
    rpc<{ content: string }>('project/read_file', { path: selectedScript })
      .then((result) => {
        setScriptContent(result.content);
        setScriptSavedContent(result.content);
      })
      .catch(() => {
        setScriptContent('// Unable to load this script.');
        setScriptSavedContent('');
      });
  }, [backendReady, selectedScript]);

  useEffect(() => {
    if (!selectedScript || !backendReady) {
      setScriptDiagnostics([]);
      return;
    }
    const lowerPath = selectedScript.toLowerCase();
    const checkMethod = lowerPath.endsWith('.varg') || lowerPath.endsWith('.vscene') || lowerPath.endsWith('.vasset')
      ? 'project/check_script'
      : lowerPath.endsWith('.amdl')
        ? 'project/check_amdl'
        : null;
    if (!checkMethod) {
      setScriptDiagnostics([]);
      return;
    }
    const timer = window.setTimeout(() => {
      rpc<{ valid: boolean; diagnostics: TextAssetDiagnostic[] }>(checkMethod, {
        path: selectedScript,
        source: scriptContent,
      })
        .then((result) => setScriptDiagnostics(result.diagnostics))
        .catch(() => setScriptDiagnostics([]));
    }, 350);
    return () => window.clearTimeout(timer);
  }, [backendReady, scriptContent, selectedScript]);

  useEffect(() => {
    if (!selectedBuildTarget.formats.includes(buildFormat)) {
      setBuildFormat(selectedBuildTarget.formats[0]);
    }
  }, [buildFormat, selectedBuildTarget]);

  useEffect(() => {
    if (!backendReady || !selectedEntityId) {
      setSelectedEntityDetails(null);
      return;
    }
    rpc<EntityDetails>('shell/get_entity', { id: selectedEntityId })
      .then((entity) => setSelectedEntityDetails(entity))
      .catch(() => setSelectedEntityDetails(null));
  }, [backendReady, selectedEntityId, sceneVersion]);

  const selectEntity = (id: string) => {
    setSelectedEntityId(id);
    if (backendReady) rpc('shell/select_entity', { id }).catch(() => {});
  };

  const saveScene = async () => {
    await rpc('shell/save_scene').catch(() => {});
    await refreshSceneTree();
  };

  const undoScene = async () => {
    await rpc('shell/undo').catch(() => {});
    await refreshSceneTree();
  };

  const redoScene = async () => {
    await rpc('shell/redo').catch(() => {});
    await refreshSceneTree();
  };

  const createCameraObject = async () => {
    if (!backendReady) return;
    const created = await rpc<SceneObject>('shell/create_object', { name: 'Camera' }).catch(() => null);
    if (created) {
      await rpc('shell/add_component', { id: created.id, component_type: 'Camera' }).catch(() => {});
    }
    await refreshSceneTree();
    if (created) selectEntity(created.id);
  };

  const deleteSceneObject = async (id: string) => {
    if (!backendReady) return;
    await rpc('shell/delete_object', { id }).catch(() => {});
    if (selectedEntityId === id) {
      const fallback = entities.find((entity) => entity.id !== id);
      setSelectedEntityId(fallback?.id ?? 'world');
      setSelectedEntityDetails(null);
    }
    await refreshSceneTree();
  };

  const runGame = async () => {
    if (backendReady) {
      await openGameView().catch(() => setIsPlaying((value) => !value));
      return;
    }
    setIsPlaying((value) => !value);
  };

  const runCommand = async (title: string) => {
    if (title === 'Create Camera') await createCameraObject();
    else if (title === 'Open Selected Script' && selectedScript) openScript(selectedScript);
    else if (title === 'Run Scene Validation') setBottomTab('problems');
    else if (title === 'Package Current Build') await runBuild();
    else if (title.includes('Play') || title.includes('Stop')) await runGame();
    setCommandOpen(false);
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setCommandOpen(false);
        return;
      }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        setCommandOpen(true);
      }
    };

    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  return (
    <div
      className={cx(
        'flex h-full min-h-0 w-full flex-col overflow-hidden bg-[var(--bg-base)] text-[13px] text-[var(--text-primary)]',
      )}
    >
      <header className="flex h-12 min-h-12 items-center gap-3 border-b border-[var(--border)] bg-[rgba(18,19,22,0.88)] px-3 backdrop-blur-xl">
        <div className="flex min-w-[232px] items-center gap-2">
          <div className="grid size-7 place-items-center rounded-[var(--radius-md)] border border-[rgba(34,197,94,0.22)] bg-[rgba(34,197,94,0.08)]">
            <VargLogo size={17} />
          </div>
          <div className="min-w-0">
            <div className="truncate text-[12px] font-semibold text-[var(--text-primary)]">Varg / {shellState?.project_name || 'Meadow Run'}</div>
            <div className="truncate font-mono text-[10px] text-[var(--text-muted)]">Scenes / Meadow.scene</div>
          </div>
        </div>

        <button
          type="button"
          className="flex h-8 min-w-0 flex-1 items-center gap-2 rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-base)] px-3 text-left text-[12px] text-[var(--text-muted)] transition-colors hover:border-[var(--border-light)] hover:bg-[var(--bg-hover)]"
          onClick={() => setCommandOpen(true)}
        >
          <IconSearch size={14} />
          <span className="min-w-0 flex-1 truncate">Search actions, assets, entities...</span>
          <span className="rounded border border-[var(--border)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--text-muted)]">⌘K</span>
        </button>

        <div className="flex items-center gap-1">
          {[
            { label: 'Save', icon: <IconSave />, action: saveScene, disabled: backendReady && !shellState?.scene_dirty },
            { label: 'Undo', icon: <IconUndo />, action: undoScene, disabled: backendReady && !shellState?.can_undo },
            { label: 'Redo', icon: <IconRedo />, action: redoScene, disabled: backendReady && !shellState?.can_redo },
          ].map((item) => (
            <button
              key={item.label}
              type="button"
              title={item.label}
              className="grid size-8 place-items-center rounded-[var(--radius-md)] text-[var(--text-secondary)] transition-colors hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] disabled:cursor-not-allowed disabled:opacity-40"
              onClick={item.action}
              disabled={item.disabled}
            >
              {item.icon}
            </button>
          ))}
          <div className="mx-1 h-5 w-px bg-[var(--border)]" />
          <button
            type="button"
            className={cx(
              'flex h-8 items-center gap-2 rounded-[var(--radius-md)] border px-3 text-[12px] font-semibold transition-colors',
              isPlaying
                ? 'border-[rgba(248,113,113,0.38)] bg-[rgba(248,113,113,0.12)] text-[var(--danger)]'
                : 'border-[rgba(34,197,94,0.32)] bg-[rgba(34,197,94,0.12)] text-[var(--brand)] hover:bg-[rgba(34,197,94,0.18)]',
            )}
            onClick={runGame}
          >
            {isPlaying ? <IconStop size={14} /> : <IconPlay size={14} />}
            {isPlaying ? 'Stop' : 'Play'}
          </button>
          <button
            type="button"
            className="grid size-8 place-items-center rounded-[var(--radius-md)] text-[var(--text-secondary)] transition-colors hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
            title="Open Quest"
            onClick={onOpenQuest}
          >
            <IconSparkles />
          </button>
          <button
            type="button"
            className="grid size-8 place-items-center rounded-[var(--radius-md)] text-[var(--text-secondary)] transition-colors hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
            title="Settings"
            onClick={onOpenSettings}
          >
            <IconSettings />
          </button>
        </div>
      </header>

      {questArtifact && (
        <div className="flex h-9 items-center justify-between border-b border-[rgba(34,197,94,0.22)] bg-[rgba(34,197,94,0.08)] px-3 text-[12px] text-[var(--text-secondary)]">
          <span className="truncate">
            Quest artifact ready: <span className="font-semibold text-[var(--text-primary)]">{questArtifact.label}</span>
          </span>
          <button type="button" className="text-[var(--brand)] hover:text-[var(--brand-hover)]" onClick={onDismissQuestArtifact}>
            Dismiss
          </button>
        </div>
      )}

      <main className="flex min-h-0 flex-1 overflow-hidden">
        <nav className="flex w-14 shrink-0 flex-col items-center border-r border-[var(--border)] bg-[var(--bg-surface)] py-2">
          {navItems.map((item) => (
            <button
              key={item.id}
              type="button"
              title={item.label}
              className={cx(
                'mb-1 grid size-10 place-items-center rounded-[var(--radius-md)] border text-[var(--text-muted)] transition-colors',
                activeNav === item.id
                  ? 'border-[rgba(34,197,94,0.28)] bg-[rgba(34,197,94,0.12)] text-[var(--brand)]'
                  : 'border-transparent hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]',
              )}
              onClick={() => setActiveNav(item.id)}
            >
              {iconForNav(item.id)}
            </button>
          ))}
          <div className="flex-1" />
          <button
            type="button"
            title="Close Project"
            className="mb-1 grid size-10 place-items-center rounded-[var(--radius-md)] border border-transparent text-[var(--text-muted)] transition-colors hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
            onClick={onCloseProject}
          >
            <IconFolder />
          </button>
        </nav>

        <aside className="flex w-[276px] shrink-0 flex-col border-r border-[var(--border)] bg-[rgba(21,22,25,0.92)]">
          <div className="flex h-11 items-center justify-between border-b border-[var(--border)] px-3">
            <div>
              <div className="text-[12px] font-semibold text-[var(--text-primary)]">
                {activeNav === 'scene' ? 'Scene' : activeNav === 'assets' ? 'Assets' : activeNav === 'scripts' ? 'Scripts' : activeNav === 'build' ? 'Build' : 'AI'}
              </div>
              <div className="font-mono text-[10px] text-[var(--text-muted)]">{backendReady ? 'Live backend' : 'Prototype data'}</div>
            </div>
            <div className="flex items-center gap-1">
              <button type="button" className="grid size-7 place-items-center rounded-[var(--radius-sm)] text-[var(--text-muted)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] disabled:cursor-not-allowed disabled:opacity-40" onClick={createCameraObject} disabled={!backendReady} title="Create camera">
                <IconPlus size={13} />
              </button>
              <button type="button" className="grid size-7 place-items-center rounded-[var(--radius-sm)] text-[var(--text-muted)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]">
                <IconChevronDown size={13} />
              </button>
            </div>
          </div>

          <div className="border-b border-[var(--border)] p-2">
            <label className="flex h-8 items-center gap-2 rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-base)] px-2 text-[12px] text-[var(--text-muted)]">
              <IconSearch size={13} />
              <input
                className="min-w-0 flex-1 bg-transparent text-[var(--text-primary)] outline-none placeholder:text-[var(--text-muted)]"
                placeholder={activeNav === 'assets' ? 'Filter assets' : 'Filter entities'}
                value={activeNav === 'assets' ? assetSearch : sceneSearch}
                onChange={(event) => {
                  if (activeNav === 'assets') setAssetSearch(event.target.value);
                  else setSceneSearch(event.target.value);
                }}
              />
            </label>
          </div>

          <div className="min-h-0 flex-1 overflow-auto py-1">
            {activeNav === 'assets' ? (
              filteredAssets.map((asset) => (
                <button
                  key={asset.path}
                  type="button"
                  className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-[var(--bg-hover)]"
                  onClick={() => {
                    if (/script/i.test(asset.kind) || /\.(varg|vscene|vasset|amdl|js|ts|lua)$/i.test(asset.path)) openScript(asset.path);
                  }}
                >
                  <span className="grid size-7 shrink-0 place-items-center rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] text-[var(--text-secondary)]">
                    {asset.kind === 'script' ? <IconCode /> : asset.kind === 'scene' ? <IconView /> : <IconFile />}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[12px] text-[var(--text-primary)]">{asset.name}</span>
                    <span className="block truncate font-mono text-[10px] text-[var(--text-muted)]">{asset.path}</span>
                  </span>
                  <span className="shrink-0 text-[10px] text-[var(--text-muted)]">{asset.updated}</span>
                </button>
              ))
            ) : activeNav === 'scripts' ? (
              <div className="space-y-2 px-3 py-2">
                {scriptAssets.length === 0 && (
                  <div className="rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-base)] p-3 text-[12px] text-[var(--text-muted)]">
                    No script-like assets found.
                  </div>
                )}
                {scriptAssets.map((path) => (
                  <button
                    key={path}
                    type="button"
                    className={cx(
                      'flex w-full items-center gap-2 rounded-[var(--radius-sm)] px-2 py-2 text-left text-[12px]',
                      selectedScript === path ? 'bg-[rgba(34,197,94,0.10)] text-[var(--text-primary)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)]',
                    )}
                    onClick={() => openScript(path)}
                  >
                    <IconCode size={13} />
                    <span className="min-w-0 flex-1 truncate">{path}</span>
                    {selectedScript === path && <IconCheck size={12} className="text-[var(--brand)]" />}
                  </button>
                ))}
                {selectedScript && (
                  <div className="overflow-hidden rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-base)]">
                    <div className="flex h-8 items-center justify-between border-b border-[var(--border)] px-2">
                      <span className="truncate font-mono text-[10px] text-[var(--text-muted)]">{selectedScript}</span>
                      <button
                        type="button"
                        className="rounded-[var(--radius-sm)] px-2 py-1 text-[11px] text-[var(--brand)] hover:bg-[var(--bg-hover)] disabled:opacity-40"
                        onClick={saveScript}
                        disabled={!backendReady || scriptSaving || scriptContent === scriptSavedContent}
                      >
                        {scriptSaving ? 'Saving' : 'Save'}
                      </button>
                    </div>
                    <textarea
                      className="h-52 w-full resize-none bg-transparent p-2 font-mono text-[11px] leading-5 text-[var(--text-secondary)] outline-none"
                      value={scriptContent}
                      spellCheck={false}
                      onChange={(event) => setScriptContent(event.target.value)}
                    />
                    {scriptDiagnostics.length > 0 && (
                      <div className="border-t border-[var(--border)] px-2 py-2">
                        {scriptDiagnostics.slice(0, 3).map((diagnostic, index) => (
                          <div key={`${diagnostic.message}-${index}`} className="mb-1 text-[11px] text-[var(--warning)]">
                            {diagnostic.line ?? 0}:{diagnostic.column ?? 0} {diagnostic.message}
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </div>
            ) : activeNav === 'build' ? (
              <div className="space-y-3 p-3">
                <FieldRow label="Target">
                  <select className="h-7 w-full rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] px-2 text-[12px] text-[var(--text-primary)] outline-none" value={buildTarget} onChange={(event) => setBuildTarget(event.target.value as BuildTarget)}>
                    {buildTargets.map((target) => <option key={target.id} value={target.id}>{target.label}</option>)}
                  </select>
                </FieldRow>
                <FieldRow label="Format">
                  <select className="h-7 w-full rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] px-2 text-[12px] text-[var(--text-primary)] outline-none" value={buildFormat} onChange={(event) => setBuildFormat(event.target.value as BuildFormat)}>
                    {selectedBuildTarget.formats.map((format) => <option key={format} value={format}>{format}</option>)}
                  </select>
                </FieldRow>
                <FieldRow label="Channel">
                  <select className="h-7 w-full rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] px-2 text-[12px] text-[var(--text-primary)] outline-none" value={buildChannel} onChange={(event) => setBuildChannel(event.target.value as 'debug' | 'release')}>
                    <option value="debug">debug</option>
                    <option value="release">release</option>
                  </select>
                </FieldRow>
                <div className="rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-base)] p-3 text-[11px] leading-5 text-[var(--text-muted)]">
                  <div className="mb-1 flex items-center justify-between">
                    <span>{selectedBuildTarget.status}</span>
                    <span className="font-mono">{selectedBuildTarget.id}</span>
                  </div>
                  {selectedBuildTarget.note}
                </div>
                <button
                  type="button"
                  className="flex h-8 w-full items-center justify-center gap-2 rounded-[var(--radius-md)] border border-[rgba(34,197,94,0.32)] bg-[rgba(34,197,94,0.12)] text-[12px] font-semibold text-[var(--brand)] hover:bg-[rgba(34,197,94,0.18)] disabled:opacity-40"
                  onClick={runBuild}
                  disabled={buildBusy}
                >
                  <IconPackage size={13} /> {buildBusy ? 'Packaging...' : 'Package'}
                </button>
                {buildMessage && (
                  <pre className="max-h-36 overflow-auto whitespace-pre-wrap rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-base)] p-2 font-mono text-[10px] leading-4 text-[var(--text-secondary)]">{buildMessage}</pre>
                )}
              </div>
            ) : activeNav === 'ai' ? (
              <div className="h-full min-h-0">
                <AiPanelBoundary>
                  <AiPanel
                    projectName={shellState?.project_name}
                    selectedEntity={selectedEntityId}
                    selectedEntityName={inspectorEntity.name}
                    sceneObjectCount={entities.length}
                    sceneObjects={entities.map(entity => ({ id: entity.id, name: entity.name }))}
                    onQuickAction={(action) => {
                      if (action === 'play') {
                        openGameView();
                      } else if (action === 'save') {
                        rpc('shell/save').then(() => refreshSceneTree());
                      } else if (action === 'undo') {
                        rpc('shell/undo').then(() => refreshSceneTree());
                      }
                    }}
                    onSceneChanged={() => {
                      refreshSceneTree();
                      refreshConsole();
                    }}
                    chatOnly
                    contextualRequest={contextualRequest}
                    onContextualRequestConsumed={id => setContextualRequest(current => current?.id === id ? null : current)}
                    onOpenSettings={onOpenSettings}
                    onOpenQuest={onOpenQuest}
                    compact
                  />
                </AiPanelBoundary>
              </div>
            ) : (
              Object.entries(groupedEntities).map(([group, groupEntities]) => (
                <div key={group} className="pb-1">
                  <div className="px-3 py-2 text-[10px] font-semibold uppercase tracking-[0.08em] text-[var(--text-muted)]">{group}</div>
                  {groupEntities.map((entity) => (
                    <button
                      key={entity.id}
                      type="button"
                      className={cx(
                        'flex h-8 w-full items-center gap-2 px-3 text-left transition-colors',
                        selectedEntity.id === entity.id ? 'bg-[rgba(34,197,94,0.10)] text-[var(--text-primary)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]',
                      )}
                      onClick={() => selectEntity(entity.id)}
                      onContextMenu={(event) => {
                        event.preventDefault();
                        selectEntity(entity.id);
                        setHierarchyContextMenu({
                          x: event.clientX,
                          y: event.clientY,
                          entity,
                        });
                      }}
                    >
                      <span className={cx('size-1.5 rounded-full', entity.visible ? 'bg-[var(--brand)]' : 'bg-[var(--text-muted)]')} />
                      <span className="min-w-0 flex-1 truncate">{entity.name}</span>
                      <span className="shrink-0 font-mono text-[10px] text-[var(--text-muted)]">{entity.type}</span>
                    </button>
                  ))}
                </div>
              ))
            )}
          </div>

          <div className="border-t border-[var(--border)] p-2">
            <div className="mb-2 flex items-center justify-between text-[11px] text-[var(--text-muted)]">
              <span>Recent Assets</span>
              <span>{projectAssets.length}</span>
            </div>
            <div className="space-y-1">
              {projectAssets.slice(0, 3).map((asset) => (
                <button key={asset.path} type="button" className="flex h-7 w-full items-center gap-2 rounded-[var(--radius-sm)] px-2 text-left text-[11px] text-[var(--text-secondary)] hover:bg-[var(--bg-hover)]">
                  <IconFile size={12} />
                  <span className="min-w-0 flex-1 truncate">{asset.name}</span>
                </button>
              ))}
            </div>
          </div>
        </aside>

        <section
          className={cx(
            'relative flex min-w-0 flex-1 flex-col overflow-hidden bg-[#101114]',
            nativeHostSceneActive && 'bg-transparent',
          )}
        >
          <div className="flex h-10 items-center justify-between border-b border-[var(--border)] bg-[rgba(15,16,18,0.82)] px-3">
            <div className="flex min-w-0 items-center gap-1">
              {(['select', 'move', 'rotate', 'scale'] as TransformTool[]).map((item) => (
                <button
                  key={item}
                  type="button"
                  className={cx(
                    'flex h-7 items-center gap-1.5 rounded-[var(--radius-sm)] px-2 text-[11px] capitalize transition-colors',
                    tool === item ? 'bg-[var(--bg-hover)] text-[var(--text-primary)]' : 'text-[var(--text-muted)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]',
                  )}
                  onClick={() => setTool(item)}
                >
                  {iconForTool(item)}
                  {item}
                </button>
              ))}
              <div className="mx-1 h-4 w-px bg-[var(--border)]" />
              <button
                type="button"
                className={cx(
                  'h-7 rounded-[var(--radius-sm)] px-2 text-[11px] transition-colors',
                  snapEnabled ? 'bg-[rgba(34,197,94,0.12)] text-[var(--brand)]' : 'text-[var(--text-muted)] hover:bg-[var(--bg-hover)]',
                )}
                onClick={() => setSnapEnabled((value) => !value)}
              >
                Snap 0.25m
              </button>
              <button type="button" className="h-7 rounded-[var(--radius-sm)] px-2 text-[11px] text-[var(--text-muted)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]">
                Lit
              </button>
              <div className="ml-1 flex rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] p-0.5">
                {(['3d', '2d'] as ViewMode[]).map((mode) => (
                  <button
                    key={mode}
                    type="button"
                    className={cx(
                      'h-6 rounded-[3px] px-2 text-[10px] uppercase transition-colors',
                      viewMode === mode ? 'bg-[var(--bg-hover)] text-[var(--text-primary)]' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)]',
                    )}
                    onClick={() => setViewMode(mode)}
                  >
                    {mode}
                  </button>
                ))}
              </div>
            </div>
            <div className="flex items-center gap-2 font-mono text-[10px] text-[var(--text-muted)]">
              <span className={cx('size-1.5 rounded-full', isPlaying ? 'bg-[var(--brand)]' : 'bg-[var(--text-muted)]')} />
              <span>{isPlaying ? 'Play mode' : 'Editor mode'}</span>
              <span>{viewportSize.width}x{viewportSize.height}</span>
            </div>
          </div>

          <div
            ref={viewportFrameRef}
            className={cx(
              'relative min-h-0 flex-1 overflow-hidden border-t border-white/[0.03] shadow-[inset_0_1px_0_rgba(255,255,255,0.035)]',
              nativeHostSceneActive
                ? 'bg-transparent'
                : 'bg-[radial-gradient(circle_at_50%_18%,rgba(96,165,250,0.10),transparent_32%),linear-gradient(180deg,#101721_0%,#081018_55%,#070A0F_100%)]',
            )}
          >
            {zeroCopySceneActive ? (
              <div className="pointer-events-none absolute inset-0 z-[1] bg-[linear-gradient(180deg,rgba(7,10,15,0.10),transparent_18%,transparent_78%,rgba(7,10,15,0.18))]" aria-hidden="true">
                <ViewportGrid />
              </div>
            ) : !zeroCopySceneActive ? (
              <ViewportCanvas
                key={`${viewMode}-${cameraRevision > -1}`}
                sceneObjects={sceneObjects}
                sceneVersion={sceneVersion + cameraRevision}
                selectedEntityId={inspectorEntity?.id ?? null}
                viewMode={viewMode}
                playMode={isPlaying}
                cameraRef={cameraRef}
                onCameraChange={handleCameraChange}
                onResize={handleViewportResize}
                onSelectEntity={selectEntity}
              />
            ) : null}

            {zeroCopySceneActive && (
              <ViewportCanvas
                key={`native-input-${viewMode}-${cameraRevision > -1}`}
                sceneObjects={sceneObjects}
                sceneVersion={sceneVersion + cameraRevision}
                selectedEntityId={inspectorEntity?.id ?? null}
                viewMode={viewMode}
                playMode={isPlaying}
                cameraRef={cameraRef}
                onCameraChange={handleCameraChange}
                onResize={handleViewportResize}
                onSelectEntity={selectEntity}
                inputOnly
              />
            )}

            {nativeSceneError && (
              <div className="absolute left-4 top-4 z-[4] max-w-[420px] rounded-[var(--radius-md)] border border-[rgba(245,158,11,0.36)] bg-[rgba(24,18,10,0.88)] px-3 py-2 text-[11px] leading-5 text-[var(--text-secondary)] backdrop-blur-xl">
                Native Scene View unavailable: {nativeSceneError}
              </div>
            )}

            <div className="absolute bottom-4 left-4 z-[4] flex max-w-[min(520px,60%)] items-center gap-2 rounded-[var(--radius-md)] border border-[var(--border)] bg-[rgba(13,14,16,0.72)] px-3 py-2 backdrop-blur-xl">
              <span className="font-mono text-[11px] text-[var(--text-muted)]">World &gt;</span>
              <span className="min-w-0 truncate text-[12px] font-semibold">{inspectorEntity.name}</span>
              <span className="font-mono text-[11px] text-[var(--text-muted)]">
                x {inspectorEntity.position[0].toFixed(2)} y {inspectorEntity.position[1].toFixed(2)} z {inspectorEntity.position[2].toFixed(2)}
              </span>
            </div>

            <form
              className="absolute bottom-4 right-4 z-[4] flex w-[min(360px,42%)] items-center gap-2 rounded-[var(--radius-md)] border border-[var(--border)] bg-[rgba(13,14,16,0.78)] p-2 backdrop-blur-xl"
              onSubmit={(event) => {
                event.preventDefault();
                const prompt = aiPrompt.trim();
                if (prompt) {
                  setActiveNav('ai');
                  setContextualRequest({ id: Date.now(), prompt });
                }
                setAiPrompt('');
              }}
            >
              <IconBot size={14} className="shrink-0 text-[var(--brand)]" />
              <input
                className="min-w-0 flex-1 bg-transparent text-[12px] text-[var(--text-primary)] outline-none placeholder:text-[var(--text-muted)]"
                placeholder={`Ask about ${inspectorEntity.name}`}
                value={aiPrompt}
                onChange={(event) => setAiPrompt(event.target.value)}
              />
              <button type="submit" className="grid size-7 place-items-center rounded-[var(--radius-sm)] bg-[var(--bg-hover)] text-[var(--text-secondary)] hover:text-[var(--text-primary)]">
                <IconSend size={13} />
              </button>
            </form>
          </div>

          {showDrawer && (
            <div className="h-[164px] min-h-[164px] border-t border-[var(--border)] bg-[var(--bg-surface)]">
              <div className="flex h-9 items-center justify-between border-b border-[var(--border)] px-2">
                <div className="flex items-center gap-1">
                  {(['console', 'problems', 'tasks', 'assets'] as BottomTab[]).map((tab) => (
                    <button
                      key={tab}
                      type="button"
                      className={cx(
                        'h-7 rounded-[var(--radius-sm)] px-2 text-[11px] capitalize transition-colors',
                        bottomTab === tab ? 'bg-[var(--bg-hover)] text-[var(--text-primary)]' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)]',
                      )}
                      onClick={() => setBottomTab(tab)}
                    >
                      {tab}
                      {tab === 'problems' && <span className="ml-1 text-[var(--warning)]">{visibleProblems.length}</span>}
                    </button>
                  ))}
                </div>
                <button type="button" className="text-[11px] text-[var(--text-muted)] hover:text-[var(--text-primary)]" onClick={() => setShowDrawer(false)}>
                  Hide
                </button>
              </div>
              <div className="h-[124px] overflow-auto">
                {bottomTab === 'problems' && visibleProblems.map((problem) => (
                  <div key={problem.title} className="grid grid-cols-[20px_1fr_auto] items-center gap-2 border-b border-[rgba(212,212,216,0.08)] px-3 py-2 text-[12px]">
                    {problem.severity === 'error' ? <IconAlertCircle className="text-[var(--danger)]" /> : <IconAlertTriangle className="text-[var(--warning)]" />}
                    <div className="min-w-0">
                      <div className="truncate text-[var(--text-primary)]">{problem.title}</div>
                      <div className="truncate font-mono text-[10px] text-[var(--text-muted)]">{problem.file}:{problem.line}</div>
                    </div>
                    <button type="button" className="rounded-[var(--radius-sm)] border border-[var(--border)] px-2 py-1 text-[11px] text-[var(--text-secondary)] hover:bg-[var(--bg-hover)]">Open</button>
                  </div>
                ))}
                {bottomTab === 'console' && (consoleEntries.length > 0 ? consoleEntries.map((entry) => ({
                  level: entry.level,
                  source: entry.subsystem,
                  message: entry.message,
                })) : mockConsoleRows).map((row) => (
                  <div key={`${row.source}-${row.message}`} className="grid grid-cols-[64px_96px_1fr] border-b border-[rgba(212,212,216,0.08)] px-3 py-2 font-mono text-[11px]">
                    <span className={row.level === 'warn' ? 'text-[var(--warning)]' : 'text-[var(--text-muted)]'}>{row.level}</span>
                    <span className="text-[var(--text-muted)]">{row.source}</span>
                    <span className="truncate text-[var(--text-secondary)]">{row.message}</span>
                  </div>
                ))}
                {bottomTab === 'tasks' && tasks.map((task) => (
                  <div key={task.title} className="grid grid-cols-[1fr_96px] border-b border-[rgba(212,212,216,0.08)] px-3 py-2 text-[12px]">
                    <span>{task.title}</span>
                    <span className="font-mono text-[10px] text-[var(--text-muted)]">{task.status}</span>
                  </div>
                ))}
                {bottomTab === 'assets' && filteredAssets.map((asset) => (
                  <div key={asset.path} className="grid grid-cols-[1fr_72px_84px] border-b border-[rgba(212,212,216,0.08)] px-3 py-2 text-[12px]">
                    <span className="truncate">{asset.path}</span>
                    <span className="font-mono text-[10px] text-[var(--text-muted)]">{asset.kind}</span>
                    <span className="text-right text-[11px] text-[var(--text-muted)]">{asset.updated}</span>
                  </div>
                ))}
              </div>
            </div>
          )}
        </section>

        <aside className="flex w-[326px] shrink-0 flex-col border-l border-[var(--border)] bg-[rgba(21,22,25,0.94)]">
          <div className="flex h-14 items-center justify-between border-b border-[var(--border)] px-3">
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <span className="min-w-0 truncate text-[13px] font-semibold">{inspectorEntity.name}</span>
                <span className="shrink-0 rounded-full border border-[var(--border)] px-2 py-0.5 font-mono text-[10px] text-[var(--text-muted)]">{inspectorEntity.type}</span>
              </div>
            </div>
            <div className="flex items-center gap-1">
              <button type="button" className="grid size-7 place-items-center rounded-[var(--radius-sm)] text-[var(--text-muted)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]">
                <IconView size={13} />
              </button>
              <button type="button" className="grid size-7 place-items-center rounded-[var(--radius-sm)] text-[var(--text-muted)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]">
                <IconSettings size={13} />
              </button>
            </div>
          </div>

          <div className="min-h-0 flex-1 overflow-auto">
            <SectionHeader title="Transform" open={openInspector.transform} onToggle={() => toggleInspector('transform')} />
            {openInspector.transform && (
              <div className="py-2">
                {(['position', 'rotation', 'scale'] as const).map((axis) => (
                  <FieldRow key={axis} label={axis[0].toUpperCase() + axis.slice(1)}>
                    <div className="grid grid-cols-3 gap-1.5">
                      {inspectorEntity[axis].map((value, index) => (
                        <NumberField key={`${axis}-${index}`} value={value} onChange={(next) => setSelectedTransform(axis, index, next)} />
                      ))}
                    </div>
                  </FieldRow>
                ))}
              </div>
            )}

            <div className="border-t border-[var(--border)] px-3 py-2">
              <div className="mb-2 flex items-center justify-between">
                <span className="text-[12px] font-semibold text-[var(--text-secondary)]">Components</span>
                <div className="flex items-center gap-1">
                  <select
                    className="h-7 max-w-[130px] rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] px-2 text-[11px] text-[var(--text-primary)] outline-none"
                    value={addComponentType}
                    onChange={(event) => setAddComponentType(event.target.value)}
                  >
                    {componentTypes.map((type) => <option key={type}>{type}</option>)}
                  </select>
                  <button
                    type="button"
                    className="grid size-7 place-items-center rounded-[var(--radius-sm)] border border-[var(--border)] text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] disabled:opacity-40"
                    onClick={addComponent}
                    disabled={!backendReady}
                    title="Add component"
                  >
                    <IconPlus size={13} />
                  </button>
                </div>
              </div>
            </div>

            {(inspectorEntity.components ?? []).length === 0 && (
              <div className="mx-3 mb-3 rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-base)] p-3 text-[12px] text-[var(--text-muted)]">
                No component data returned for this entity.
              </div>
            )}

            {(inspectorEntity.components ?? []).map((component) => (
              <div key={component.type}>
                <SectionHeader
                  title={component.type}
                  open={openInspector.script}
                  onToggle={() => toggleInspector('script')}
                >
                  <button
                    type="button"
                    className="grid size-6 place-items-center rounded-[var(--radius-sm)] text-[var(--text-muted)] hover:bg-[var(--bg-hover)] hover:text-[var(--danger)] disabled:opacity-40"
                    title="Remove component"
                    disabled={!backendReady}
                    onClick={(event) => {
                      event.stopPropagation();
                      removeComponent(component.type);
                    }}
                  >
                    <IconTrash size={12} />
                  </button>
                </SectionHeader>
                {openInspector.script && (
                  <div className="py-2">
                    {Object.entries(component.data ?? {}).length === 0 ? (
                      <div className="px-3 text-[12px] text-[var(--text-muted)]">No editable fields.</div>
                    ) : (
                      Object.entries(component.data ?? {}).map(([fieldName, value]) => (
                        <ComponentField
                          key={`${component.type}-${fieldName}`}
                          name={fieldName}
                          value={value}
                          onChange={(next) => updateComponentField(component.type, fieldName, next)}
                        />
                      ))
                    )}
                    {component.type.toLowerCase().includes('script') && selectedScript && (
                      <div className="px-3 pt-1">
                        <button
                          type="button"
                          className="flex h-8 w-full items-center justify-center gap-2 rounded-[var(--radius-md)] border border-[var(--border)] text-[12px] text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
                          onClick={() => openScript(selectedScript)}
                        >
                          <IconEdit size={13} /> Open script
                        </button>
                      </div>
                    )}
                  </div>
                )}
              </div>
            ))}
          </div>
        </aside>

        {hierarchyContextMenu && (
          <div
            className={hierarchyContextMenuClass.root}
            style={{ left: hierarchyContextMenu.x, top: hierarchyContextMenu.y }}
            onClick={(event) => event.stopPropagation()}
          >
            <button
              type="button"
              className={hierarchyContextMenuClass.item}
              onClick={() => {
                selectEntity(hierarchyContextMenu.entity.id);
                setHierarchyContextMenu(null);
              }}
            >
              <IconView size={13} /> Inspect
            </button>
            <div className={hierarchyContextMenuClass.separator} />
            <button
              type="button"
              className={cx(hierarchyContextMenuClass.item, hierarchyContextMenuClass.danger)}
              disabled={!backendReady}
              onClick={() => {
                const id = hierarchyContextMenu.entity.id;
                setHierarchyContextMenu(null);
                deleteSceneObject(id);
              }}
            >
              <IconTrash size={13} /> Delete
            </button>
          </div>
        )}
      </main>

      <footer className="flex h-6 min-h-6 items-center justify-between border-t border-[var(--border)] bg-[rgba(18,19,22,0.92)] px-3 font-mono text-[10px] text-[var(--text-muted)]">
        <div className="flex min-w-0 items-center gap-3">
          <span className="flex items-center gap-1"><IconCheck size={11} className="text-[var(--brand)]" /> saved</span>
          <span>runtime-min</span>
          <span>{inspectorEntity.name}</span>
          {!showDrawer && (
            <button type="button" className="text-[var(--brand)] hover:text-[var(--brand-hover)]" onClick={() => setShowDrawer(true)}>
              show drawer
            </button>
          )}
        </div>
        <div className="flex items-center gap-3">
          <span>{visibleProblems.length} problems</span>
          <span>{entities.reduce((count, entity) => count + (entity.components?.length ?? 1), 0)} components</span>
          <span>60 FPS</span>
        </div>
      </footer>

      {commandOpen && (
        <div className="absolute inset-0 z-50 grid place-items-start justify-center bg-[rgba(0,0,0,0.36)] pt-[12vh] backdrop-blur-sm" onMouseDown={() => setCommandOpen(false)}>
          <div className="w-[min(640px,calc(100vw-32px))] overflow-hidden rounded-[var(--radius-lg)] border border-[var(--border-light)] bg-[var(--bg-surface)] shadow-[var(--shadow-lg)]" onMouseDown={(event) => event.stopPropagation()}>
            <div className="flex h-12 items-center gap-3 border-b border-[var(--border)] px-4">
              <IconSearch size={15} className="text-[var(--text-muted)]" />
              <input autoFocus className="min-w-0 flex-1 bg-transparent text-[14px] text-[var(--text-primary)] outline-none placeholder:text-[var(--text-muted)]" placeholder="Search actions, assets, entities..." />
              <span className="font-mono text-[10px] text-[var(--text-muted)]">Esc</span>
            </div>
            <div className="max-h-[360px] overflow-auto p-2">
              {commands.map((command) => (
                <button
                  key={command.title}
                  type="button"
                  className="flex w-full items-center gap-3 rounded-[var(--radius-md)] px-3 py-2 text-left hover:bg-[var(--bg-hover)]"
                  onClick={() => runCommand(command.title)}
                >
                  <span className="grid size-8 shrink-0 place-items-center rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] text-[var(--text-secondary)]">
                    <IconSparkles size={14} />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[13px] text-[var(--text-primary)]">{command.title}</span>
                    <span className="block truncate text-[11px] text-[var(--text-muted)]">{command.detail}</span>
                  </span>
                </button>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
