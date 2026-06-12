// Tauri IPC wrapper — matches the old window.aster.rpc() signature.
// Swap to direct typed invoke() calls later if needed.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/**
 * Call an editor backend method via JSON-RPC-style dispatch.
 *
 * @param method  Method name, e.g. "hub/get_state", "shell/get_scene_tree"
 * @param params  Optional parameters object
 */
export function rpc<T = unknown>(method: string, params?: unknown): Promise<T> {
  return invoke<T>('rpc', { method, params: params ?? {} });
}

let nextStreamRequestId = 1;

interface CopilotStreamEvent {
  request_id: string;
  kind: 'text' | 'thinking' | 'tool_call';
  delta: string;
}

/** Run a Copilot planning request while receiving model response deltas. */
export async function streamCopilotPlan<T>(
  params: Record<string, unknown>,
  onDelta: (delta: string, kind: CopilotStreamEvent['kind']) => void,
): Promise<T> {
  const requestId = `${Date.now()}-${nextStreamRequestId++}`;
  const unlistenDelta = await listen<CopilotStreamEvent>('copilot-stream', (event) => {
    if (event.payload.request_id === requestId) {
      onDelta(event.payload.delta, event.payload.kind ?? 'text');
    }
  });
  let resolveComplete: (() => void) | undefined;
  const completed = new Promise<void>((resolve) => { resolveComplete = resolve; });
  const unlistenComplete = await listen<{ request_id: string }>('copilot-stream-complete', (event) => {
    if (event.payload.request_id === requestId) resolveComplete?.();
  });

  try {
    await invoke('start_copilot_plan', { requestId, params });
    await completed;
    return await invoke<T>('finish_copilot_plan', { requestId });
  } finally {
    unlistenDelta();
    unlistenComplete();
  }
}

/**
 * Viewport readback as raw RGBA via binary IPC, with lazy rendering support.
 *
 * When `lastVersion` matches the backend's scene version, the backend skips
 * rendering entirely and returns a 0-size buffer — no GPU work, no IPC transfer.
 *
 * Returns an ArrayBuffer with layout:
 *   [0..4)   width  (u32 LE) — 0 means "no change"
 *   [4..8)   height (u32 LE)
 *   [8..end) RGBA pixels (width × height × 4 bytes)
 */
export function viewportReadback(params: {
  width: number;
  height: number;
  lastVersion?: number;
  yaw?: number;
  pitch?: number;
  distance?: number;
  targetX?: number;
  targetY?: number;
  targetZ?: number;
  playMode?: boolean;
  editorCamera?: boolean;
  viewMode?: '2d' | '3d';
  entityId?: string;
}): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>('viewport_readback_raw', {
    width: params.width,
    height: params.height,
    yaw: params.yaw ?? -0.5,
    pitch: params.pitch ?? 0.3,
    distance: params.distance ?? 6.0,
    target_x: params.targetX ?? 0,
    target_y: params.targetY ?? 0,
    target_z: params.targetZ ?? 0,
    last_version: params.lastVersion ?? null,
    play_mode: params.playMode ?? false,
    editor_camera: params.editorCamera ?? false,
    view_mode: params.viewMode ?? '3d',
    entity_id: params.entityId ?? null,
  });
}

/**
 * Listen for push events from the Rust host.
 * Returns an unsubscribe function.
 */

export function onHostEvent(callback: (event: unknown) => void): Promise<UnlistenFn> {
  return listen<unknown>('host-event', (event) => {
    callback(event.payload);
  });
}

/**
 * Open the Game View in a separate Tauri window.
 */
export async function openGameView(): Promise<void> {
  await invoke('open_game_view');
}

export function selectProjectLocation(): Promise<string | null> {
  return invoke<string | null>('select_project_location');
}

/**
 * Show native file-open dialog for scene JSON files,
 * then load the selected scene via RPC.
 * Returns the opened path, or null if cancelled.
 */
export async function openScene(): Promise<string | null> {
  const selected = await invoke<string | null>('open_scene_dialog');
  if (!selected) return null;
  const result = await rpc<{ path: string }>('shell/open_scene', { path: selected });
  return result.path;
}

/**
 * Show native Save-As dialog for scene JSON files,
 * then save the current scene to the selected path via RPC.
 * Returns the saved path, or null if cancelled.
 */
export async function saveSceneAs(): Promise<string | null> {
  const selected = await invoke<string | null>('save_scene_as_dialog');
  if (!selected) return null;
  const result = await rpc<{ path: string }>('shell/save_scene_as', { path: selected });
  return result.path;
}

/**
 * Show native file-open dialog, then import the selected file into project assets.
 * Returns the source path, or null if cancelled.
 */
export async function importAsset(): Promise<string | null> {
  const selected = await invoke<string | null>('import_asset_dialog');
  if (!selected) return null;
  const result = await rpc<{ imported: string }>('project/import_file', { path: selected });
  return result.imported;
}

export function startPlayMode(): Promise<unknown> {
  return rpc('play/start');
}

export function stopPlayMode(): Promise<unknown> {
  return rpc('play/stop');
}

/** Fetch scene guide entities (cameras and lights) for viewport overlay. */
export function fetchSceneGuides(): Promise<{ guides: import('./pages/SceneGuides').GuideEntity[] }> {
  return rpc('scene/get_guides');
}

/** Read a text file from the project root (relative path). */
export function readProjectFile(path: string): Promise<{ content: string }> {
  return rpc('project/read_file', { path });
}

/** Write a text file to the project root (relative path). */
export function writeProjectFile(path: string, content: string): Promise<{ saved: boolean }> {
  return rpc('project/write_file', { path, content });
}
