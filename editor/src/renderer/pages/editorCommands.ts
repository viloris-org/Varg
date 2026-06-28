export type EditorCommandId =
  | 'scene.save'
  | 'scene.undo'
  | 'scene.redo'
  | 'scene.createEmpty'
  | 'scene.createCamera'
  | 'scene.createLight'
  | 'scene.validate'
  | 'script.save'
  | 'script.openSelected'
  | 'build.package'
  | 'layout.reset'
  | 'play.toggle';

export interface EditorCommand {
  id: EditorCommandId;
  title: string;
  detail: string;
  category: string;
  shortcut?: string;
  disabled?: boolean;
  disabledReason?: string;
}

export interface EditorCommandContext {
  canSaveScene: boolean;
  canUndo: boolean;
  canRedo: boolean;
  backendReady: boolean;
  isPlaying: boolean;
  selectedEditorPath: string | null;
  canSaveScript: boolean;
  buildTargetLabel: string;
  buildFormat: string;
  buildChannel: string;
}

export function buildEditorCommands(context: EditorCommandContext): EditorCommand[] {
  const backendReason = 'Requires the desktop editor backend.';

  return [
    {
      id: 'scene.save',
      title: 'Save Scene',
      detail: context.canSaveScene ? 'Write the current scene to disk' : 'Scene has no unsaved changes',
      category: 'Scene',
      shortcut: 'Ctrl+S',
      disabled: !context.canSaveScene,
    },
    {
      id: 'scene.undo',
      title: 'Undo',
      detail: 'Revert the last scene edit',
      category: 'Edit',
      shortcut: 'Ctrl+Z',
      disabled: !context.canUndo,
    },
    {
      id: 'scene.redo',
      title: 'Redo',
      detail: 'Reapply the last undone scene edit',
      category: 'Edit',
      shortcut: 'Ctrl+Y',
      disabled: !context.canRedo,
    },
    {
      id: 'scene.createEmpty',
      title: 'Create Empty Object',
      detail: 'Add a blank entity to the current scene',
      category: 'Scene',
      shortcut: 'Ctrl+Shift+N',
      disabled: !context.backendReady,
      disabledReason: backendReason,
    },
    {
      id: 'scene.createCamera',
      title: 'Create Camera',
      detail: 'Add a camera object to the current scene',
      category: 'Scene',
      disabled: !context.backendReady,
      disabledReason: backendReason,
    },
    {
      id: 'scene.createLight',
      title: 'Create Light',
      detail: 'Add a light object to the current scene',
      category: 'Scene',
      disabled: !context.backendReady,
      disabledReason: backendReason,
    },
    {
      id: 'script.save',
      title: 'Save Script',
      detail: context.selectedEditorPath
        ? `Write ${context.selectedEditorPath} to disk`
        : 'No script file selected',
      category: 'Scripts',
      shortcut: 'Ctrl+S',
      disabled: !context.canSaveScript,
      disabledReason: context.selectedEditorPath ? undefined : 'No script file selected.',
    },
    {
      id: 'script.openSelected',
      title: 'Open Selected Script',
      detail: context.selectedEditorPath ?? 'No script selected',
      category: 'Scripts',
      disabled: !context.selectedEditorPath,
    },
    {
      id: 'scene.validate',
      title: 'Run Scene Validation',
      detail: 'Check components, asset references, and scene structure',
      category: 'Diagnostics',
    },
    {
      id: 'build.package',
      title: 'Package Current Build',
      detail: `${context.buildTargetLabel} / ${context.buildFormat} / ${context.buildChannel}`,
      category: 'Build',
      disabled: !context.backendReady,
      disabledReason: backendReason,
    },
    {
      id: 'layout.reset',
      title: 'Reset Layout',
      detail: 'Restore editor panels to the default workspace',
      category: 'Window',
    },
    {
      id: 'play.toggle',
      title: context.isPlaying ? 'Stop Play Mode' : 'Enter Play Mode',
      detail: 'Run the scene in editor',
      category: 'Play',
      shortcut: 'Ctrl+P',
    },
  ];
}

export function filterEditorCommands(commands: EditorCommand[], query: string): EditorCommand[] {
  const normalized = query.trim().toLowerCase();
  if (!normalized) return commands;
  return commands.filter((command) => (
    command.title.toLowerCase().includes(normalized)
    || command.detail.toLowerCase().includes(normalized)
    || command.category.toLowerCase().includes(normalized)
    || command.id.toLowerCase().includes(normalized)
    || Boolean(command.shortcut?.toLowerCase().includes(normalized))
  ));
}
