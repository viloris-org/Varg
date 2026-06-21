import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  fetchSceneGuides,
  openGameView,
  openNativeSceneView,
  rpc,
  viewportReadback,
} from '../api';
import { useTranslation } from '../i18n';
import {
  buttonClass,
  productEmptyClass,
  productEmptyIconClass,
  productEmptyTextClass,
  productEmptyTitleClass,
  taskOperationPermissionLabelClass,
  toolButtonClass,
} from '../uiClasses';
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
  IconAudio,
  IconBot,
  IconCheck,
  IconChevronDown,
  IconChevronRight,
  IconCode,
  IconCopy,
  IconFile,
  IconLoader,
  IconModel,
  IconPackage,
  IconPlay,
  IconPlus,
  IconProjects,
  IconRedo,
  IconSave,
  IconSparkles,
  IconSun,
  IconTrash,
  IconUndo,
  IconView,
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

interface AsterScriptDiagnostic {
  code: string;
  severity: 'error' | 'warning';
  line?: number;
  column?: number;
  message: string;
  suggestion: string;
  source_line?: string;
}

type TextAssetDiagnostic = AsterScriptDiagnostic;

interface Props {
  onCloseProject: () => void;
  onOpenSettings?: () => void;
  onOpenQuest?: () => void;
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

function cx(...classes: Array<string | false | null | undefined>): string {
  return classes.filter(Boolean).join(' ');
}

const shellClass = {
  loading: 'flex h-screen items-center justify-center text-[var(--text-secondary)]',
  root: 'flex h-full w-full min-h-0 flex-col bg-[linear-gradient(135deg,rgba(255,255,255,0.025),transparent_34%),var(--bg-base)]',
  toolbar: 'flex min-h-[44px] items-center gap-2 border-b border-[var(--border)] bg-[var(--bg-overlay)] py-[5px] pr-2 pl-3 shadow-[var(--shadow-sm)] backdrop-blur-xl',
  toolbarProject: 'flex min-w-[120px] flex-col justify-center leading-[1.15]',
  toolbarProjectKicker: 'text-[10px] font-semibold tracking-[0.08em] text-[var(--text-muted)] uppercase',
  toolbarProjectName: 'text-[13px] font-semibold text-[var(--text-primary)]',
  toolbarSpacer: 'flex-1',
  body: 'flex min-h-0 flex-1 overflow-hidden',
  statusbar: 'flex h-[23px] min-h-[23px] select-none items-center justify-between border-t border-[var(--border)] bg-[var(--bg-overlay)] px-2 text-[11px] backdrop-blur-xl',
  statusGroup: 'flex min-w-0 items-center gap-[7px]',
  statusDivider: 'h-2.5 w-px flex-none bg-[var(--border)]',
  statusItem: 'min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-[var(--text-secondary)]',
  statusSelection: 'text-[var(--accent)]',
  statusSaved: 'text-[var(--success)]',
  statusDirty: 'flex items-center gap-[5px]',
  statusDot: 'size-1.5 rounded-full bg-[var(--warning)] shadow-[0_0_8px_var(--warning)]',
  version: 'text-[var(--accent)]',
};

const workspaceClass = {
  root: 'flex min-w-0 flex-1 flex-col overflow-hidden bg-[var(--bg-base)]',
  tabs: 'flex min-h-[38px] items-stretch gap-0.5 border-b border-[var(--border)] bg-[var(--bg-surface)] px-2',
  tab: 'group relative flex cursor-pointer items-center gap-2 border-0 bg-transparent px-3 text-[12px] font-medium text-[var(--text-muted)] transition-colors duration-150 hover:text-[var(--text-primary)] [&_svg]:size-[14px] [&_svg]:opacity-70 hover:[&_svg]:opacity-100',
  tabActive: 'tab-active text-[var(--text-primary)] [&_svg]:text-[var(--brand)] [&_svg]:opacity-100 after:absolute after:inset-x-2 after:bottom-0 after:h-[2px] after:rounded-full after:bg-[var(--brand)]',
  tabBadge: 'min-w-[16px] rounded-full bg-[var(--bg-active)] px-1.5 text-center text-[10px] font-semibold leading-[15px] text-[var(--text-secondary)] group-[.tab-active]:bg-[var(--brand-dim)] group-[.tab-active]:text-[var(--brand)]',
  view: 'min-h-0 flex-1 overflow-auto',
  viewGame: 'flex overflow-hidden',
  aiPanel: 'flex min-w-[320px] flex-col overflow-hidden border-l border-[var(--border)] bg-[var(--bg-surface)] max-[900px]:min-w-[320px] [&_.ai-context-selected]:hidden',
  aiRail: 'flex w-12 shrink-0 flex-col items-center gap-2 border-l border-[var(--border)] bg-[var(--bg-surface)] px-1.5 py-2',
  aiRailButton: 'grid size-8 cursor-pointer place-items-center rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-base)] text-[var(--text-secondary)] hover:border-[var(--border-light)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]',
  aiRailBadge: 'grid size-5 place-items-center rounded-full bg-[var(--warning)] font-mono text-[10px] font-bold text-black',
  resizeHandle: 'relative z-10 w-1 shrink-0 cursor-col-resize hover:bg-[var(--accent)] hover:opacity-30 active:bg-[var(--accent)] active:opacity-30 focus-visible:bg-[var(--accent-dim)] focus-visible:outline focus-visible:outline-1 focus-visible:-outline-offset-1 focus-visible:outline-[var(--accent)]',
};

const prdClass = {
  document: 'mx-auto mt-[34px] mb-16 w-[min(820px,calc(100%_-_64px))] text-[var(--text-secondary)]',
  header: 'border-b border-[var(--border)] pb-7',
  kicker: 'text-[10px] font-bold tracking-[0.1em] text-[var(--text-secondary)] uppercase',
  title: 'my-2 block text-[28px] tracking-[-0.03em] text-[var(--text-primary)]',
  description: 'm-0 text-xs text-[var(--text-muted)]',
  section: 'border-b border-[var(--border)] py-[25px]',
  sectionTitle: 'mb-[13px] text-sm text-[var(--text-primary)]',
  bodyText: 'text-xs leading-[1.75]',
  list: 'm-0 pl-[18px]',
  grid: 'grid grid-cols-2 gap-2.5 max-[900px]:grid-cols-1',
  gridCard: 'rounded-[7px] border border-[var(--border)] bg-[var(--bg-surface)] p-3.5',
  gridLabel: 'mb-[5px] block text-[10px] text-[var(--text-muted)]',
  gridValue: 'block text-xs text-[var(--text-primary)]',
};

const taskClass = {
  board: 'mx-auto mt-7 mb-[60px] w-[min(880px,calc(100%_-_56px))]',
  header: 'flex items-end justify-between border-b border-[var(--border)] pb-5',
  kicker: prdClass.kicker,
  title: 'mt-1.5 mb-0 text-[21px] text-[var(--text-primary)] capitalize',
  meta: 'text-[var(--text-muted)]',
  artifactCard: 'mb-[18px] grid grid-cols-[minmax(0,1fr)_auto] gap-3.5 rounded-lg border border-[var(--border-light)] bg-[var(--accent-dim)] p-3.5',
  artifactKicker: 'block text-[10px] font-bold text-[var(--text-secondary)] uppercase',
  artifactTitle: 'mt-[5px] block text-sm text-[var(--text-primary)]',
  artifactDescription: 'mt-[7px] mb-0 text-[11px] leading-normal text-[var(--text-secondary)]',
  artifactPath: 'mt-2 block overflow-hidden text-ellipsis whitespace-nowrap font-mono text-[10px] text-[var(--text-muted)]',
  artifactActions: 'flex items-start gap-2',
  artifactButton: 'inline-flex cursor-pointer items-center gap-1.5 rounded-md border border-[var(--border)] bg-[var(--bg-elevated)] px-[9px] py-[7px] text-[10px] text-[var(--text-secondary)] hover:border-[var(--border-light)] hover:text-[var(--text-primary)]',
  operations: 'overflow-hidden rounded-lg border border-[var(--border)] bg-[var(--bg-surface)]',
  operationsTitle: 'flex min-h-[42px] items-center justify-between border-b border-[var(--border)] px-[13px] text-[11px] font-semibold text-[var(--text-secondary)]',
  operationRow: 'grid grid-cols-[58px_1fr_auto] items-start gap-[9px] border-b border-[var(--border)] px-[13px] py-3 last:border-b-0',
  operationPermission: 'pt-0.5 font-mono text-[10px] font-bold',
  operationPreview: 'm-0 text-[11px] leading-normal text-[var(--text-secondary)]',
  operationState: 'whitespace-nowrap text-[10px] text-[var(--text-muted)]',
  footer: 'mt-3.5 flex justify-end gap-2',
};

const surfaceClass = {
  root: 'flex h-full min-h-0 flex-col bg-[var(--bg-base)]',
  header: 'flex min-h-[46px] items-center justify-between gap-3 border-b border-[var(--border)] bg-[var(--bg-overlay)] px-3.5 backdrop-blur-xl',
  buildHeader: 'flex min-h-[52px] items-center justify-between gap-3 border-b border-[var(--border)] bg-[var(--bg-overlay)] px-4 backdrop-blur-xl',
  headerKicker: 'block text-[10px] text-[var(--text-muted)] uppercase',
  buildKicker: 'block text-[10px] font-bold tracking-[0.08em] text-[var(--text-muted)] uppercase',
  headerTitle: 'mt-0.5 block text-xs text-[var(--text-primary)]',
  buildHeaderTitle: 'mt-[3px] block text-[13px] text-[var(--text-primary)]',
  toolbar: 'flex min-w-0 flex-wrap justify-end gap-1.5',
  button: 'min-h-7 cursor-pointer rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-elevated)] px-2.5 text-[10px] text-[var(--text-secondary)] shadow-[var(--shadow-sm)] hover:border-[var(--accent)] hover:text-[var(--text-primary)] disabled:cursor-not-allowed disabled:opacity-50',
  primaryButton: 'inline-flex min-h-8 cursor-pointer items-center gap-[7px] rounded-[var(--radius-md)] border border-[var(--accent-hover)] bg-[var(--accent-dim)] px-3 text-[11px] text-[var(--accent)] shadow-[var(--shadow-sm)] hover:border-[var(--accent)] disabled:cursor-not-allowed disabled:opacity-60',
  list: 'min-h-0 flex-1 overflow-auto p-2',
  empty: 'grid min-h-[220px] place-items-center text-xs text-[var(--text-muted)]',
};

const assetsClass = {
  row: 'grid grid-cols-[18px_minmax(0,1fr)_92px_92px_auto] items-center gap-2.5 border-b border-[var(--border)] px-2.5 py-[9px] text-[var(--text-secondary)] [&_svg]:text-[var(--text-muted)]',
  rowMain: 'min-w-0',
  rowTitle: 'block overflow-hidden text-ellipsis whitespace-nowrap text-xs text-[var(--text-primary)]',
  rowMeta: 'block overflow-hidden text-ellipsis whitespace-nowrap text-[10px] text-[var(--text-muted)]',
  actions: 'flex gap-1.5',
};

const buildClass = {
  layout: 'grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_300px] max-[1100px]:grid-cols-1',
  main: 'min-w-0 overflow-auto p-4',
  presets: 'mb-3.5 grid grid-cols-4 gap-2 max-[1100px]:grid-cols-2',
  presetButton: 'grid min-w-0 cursor-pointer grid-cols-[18px_minmax(0,1fr)] gap-[7px] rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--bg-elevated)] p-[11px] text-left text-[var(--text-secondary)] shadow-[var(--shadow-sm)] transition-[border-color,background-color,transform] duration-150 hover:-translate-y-px hover:border-[var(--border-light)] [&_svg]:row-span-2 [&_svg]:mt-px [&_svg]:text-[var(--accent)]',
  selectedButton: 'border-[var(--accent)] bg-[var(--accent-dim)]',
  card: 'mb-3.5 rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--bg-elevated)] shadow-[var(--shadow-sm)]',
  sectionTitle: 'flex min-h-[42px] items-center justify-between gap-3 border-b border-[var(--border)] px-3',
  sectionValue: 'text-[11px] text-[var(--text-secondary)]',
  targetGrid: 'grid grid-cols-3 gap-2 p-2.5 max-[1100px]:grid-cols-2',
  targetButton: 'grid min-w-0 cursor-pointer gap-[5px] rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--bg-elevated)] p-[11px] text-left text-[var(--text-secondary)] transition-[border-color,background-color] duration-150 hover:border-[var(--border-light)] hover:bg-[var(--bg-hover)]',
  itemTitle: 'overflow-hidden text-ellipsis whitespace-nowrap text-[11px] font-bold text-[var(--text-primary)]',
  itemMeta: 'overflow-hidden text-ellipsis whitespace-nowrap text-[10px] text-[var(--text-muted)]',
  status: 'justify-self-start rounded-full px-1.5 py-0.5 font-mono text-[10px] font-bold uppercase',
  formGrid: 'grid grid-cols-2 gap-2.5 p-3',
  formLabel: 'grid min-w-0 gap-1.5',
  formLabelText: 'text-[10px] text-[var(--text-muted)]',
  select: 'min-h-8 rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-base)] text-[11px] text-[var(--text-primary)]',
  checkbox: 'grid grid-cols-[16px_minmax(0,1fr)] items-center gap-1.5',
  checkboxInput: 'size-3.5',
  output: 'mb-3.5 rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--bg-elevated)] p-3 shadow-[var(--shadow-sm)]',
  outputPath: 'mt-[5px] block [overflow-wrap:anywhere] font-mono text-[11px] text-[var(--text-primary)]',
  outputNote: 'mt-2.5 mb-0 text-[11px] leading-[1.55] text-[var(--text-secondary)]',
  outputPre: 'mt-3 overflow-auto whitespace-pre-wrap rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-base)] p-2.5 font-mono text-[10px] leading-normal text-[var(--text-secondary)]',
  sidebar: 'min-h-0 overflow-auto border-l border-[var(--border)] bg-[var(--bg-surface)] px-3.5 py-4 max-[1100px]:border-t max-[1100px]:border-l-0',
  sidebarSection: 'mb-3.5 rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--bg-elevated)] p-3 shadow-[var(--shadow-sm)]',
  sidebarList: 'mt-3 grid list-none gap-2.5 p-0',
  sidebarItem: 'flex items-center gap-2 text-[11px] text-[var(--text-muted)]',
  sidebarDl: 'mt-3 grid gap-2',
  sidebarDlRow: 'flex justify-between gap-3',
  sidebarDt: 'text-[10px] text-[var(--text-muted)]',
  sidebarDd: 'm-0 overflow-hidden text-ellipsis whitespace-nowrap text-right text-[10px] text-[var(--text-secondary)]',
};

const gameClass = {
  surface: 'grid h-full min-h-0 w-full flex-1 bg-[var(--bg-base)]',
  surfaceOpen: 'grid-cols-[minmax(190px,240px)_minmax(0,1fr)_minmax(230px,280px)]',
  surfaceInspectorClosed: 'grid-cols-[minmax(190px,240px)_minmax(0,1fr)]',
  surfaceHierarchyClosed: 'grid-cols-[minmax(0,1fr)_minmax(230px,280px)]',
  surfaceOnlyMain: 'grid-cols-[minmax(0,1fr)]',
  sidePanel: 'flex min-h-0 min-w-0 flex-col border-r border-[var(--border)] bg-[var(--bg-surface)] shadow-[var(--shadow-sm)]',
  inspectorPanel: 'flex min-h-0 min-w-0 flex-col border-l border-[var(--border)] bg-[var(--bg-surface)] shadow-[var(--shadow-sm)]',
  panelHeader: 'flex min-h-[42px] items-center justify-between gap-2 border-b border-[var(--border)] bg-[var(--bg-overlay)] px-2.5 backdrop-blur-xl',
  panelHeaderText: 'text-[10px] text-[var(--text-muted)] uppercase',
  panelHeaderTitle: 'mt-0.5 block text-[11px] text-[var(--text-primary)] normal-case',
  panelHeaderActions: 'flex items-center gap-1.5',
  iconButton: 'grid size-[26px] cursor-pointer place-items-center rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-elevated)] text-[var(--text-secondary)] shadow-[var(--shadow-sm)] hover:border-[var(--accent)] hover:text-[var(--text-primary)]',
  hierarchyList: 'flex min-h-0 flex-1 flex-col overflow-auto py-1',
  hierarchyItem: 'group/row relative flex min-h-[28px] w-full cursor-pointer items-center gap-1.5 border-l-2 border-transparent pr-2 text-left text-[12px] text-[var(--text-secondary)] transition-colors duration-100 hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]',
  hierarchyItemSelected: 'border-l-[var(--brand)] bg-[var(--brand-dim)] text-[var(--text-primary)]',
  hierarchyTwisty: 'grid size-4 flex-none place-items-center rounded-[3px] text-[var(--text-muted)] hover:bg-[var(--bg-active)] hover:text-[var(--text-primary)] [&_svg]:size-3',
  hierarchyTwistySpacer: 'size-4 flex-none',
  hierarchyIcon: 'flex-none text-[var(--text-muted)] group-hover/row:text-[var(--text-secondary)]',
  hierarchyIconSelected: 'flex-none text-[var(--brand)]',
  hierarchyName: 'min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap',
  hierarchyTag: 'flex-none rounded-[3px] bg-[var(--bg-active)] px-1.5 py-px text-[10px] font-medium text-[var(--text-muted)] opacity-0 group-hover/row:opacity-100',
  mainPanel: 'flex min-h-0 min-w-0 flex-col',
  previewBar: 'flex min-h-[42px] items-center justify-between border-b border-[var(--border)] bg-[var(--bg-overlay)] px-3 text-[10px] text-[var(--text-secondary)] backdrop-blur-xl',
  previewBarGroup: 'flex items-center gap-[7px]',
  liveDot: 'size-1.5 rounded-full bg-[var(--success)] shadow-[0_0_7px_var(--success)]',
  modeSwitch: 'inline-flex gap-0.5 rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-base)] p-0.5',
  modeButton: 'flex h-[27px] cursor-pointer items-center gap-[5px] rounded-[4px] bg-transparent px-[9px] text-[10px] font-medium text-[var(--text-muted)] hover:bg-[var(--bg-active)] hover:text-[var(--text-primary)] disabled:cursor-not-allowed disabled:opacity-50',
  modeButtonActive: 'bg-[var(--bg-active)] text-[var(--text-primary)]',
  createPresets: 'flex min-h-[38px] items-center gap-1.5 overflow-x-auto border-b border-[var(--border)] bg-[var(--bg-base)] px-2.5 py-[5px]',
  createButton: 'inline-flex min-h-[26px] flex-none cursor-pointer items-center gap-[5px] rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-elevated)] px-2 text-[10px] font-semibold text-[var(--text-secondary)] hover:border-[var(--accent)] hover:text-[var(--text-primary)]',
  previewCanvas: 'relative flex min-h-0 min-w-0 flex-1 overflow-hidden bg-[#070A0F]',
  empty: 'p-3.5 text-[11px] text-[var(--text-muted)]',
};

const diagnosticsClass = {
  headerActions: 'flex gap-1.5',
  entry: 'grid grid-cols-[132px_minmax(0,1fr)] gap-2.5 border-b border-[var(--border)] px-2.5 py-[9px] text-[11px] text-[var(--text-secondary)]',
  meta: 'min-w-0',
  level: 'mb-[3px] inline-flex font-mono text-[10px] font-bold uppercase',
  subsystem: 'block overflow-hidden text-ellipsis whitespace-nowrap text-[10px] font-medium text-[var(--text-muted)]',
  message: 'm-0 min-w-0 whitespace-pre-wrap [overflow-wrap:anywhere]',
  source: 'col-start-2 font-mono text-[10px] text-[var(--text-muted)]',
};

const artifactPopoverClass = {
  root: 'fixed z-[90] translate-x-2.5 translate-y-2.5 drop-shadow-[0_10px_24px_rgba(0,0,0,0.42)]',
  button: 'flex h-[30px] cursor-pointer items-center gap-1.5 rounded-md border border-[var(--border-light)] bg-[var(--bg-elevated)] px-2.5 text-[10px] font-semibold text-[var(--text-secondary)] hover:border-[var(--accent)] hover:bg-[var(--bg-hover)]',
  panel: 'w-[310px] overflow-hidden rounded-lg border border-[var(--border-light)] bg-[var(--bg-elevated)]',
  header: 'flex h-8 items-center justify-between gap-2 border-b border-[var(--border)] px-[9px] font-mono text-[10px] text-[var(--text-secondary)]',
  label: 'overflow-hidden text-ellipsis whitespace-nowrap',
  closeButton: 'grid size-[22px] flex-none cursor-pointer place-items-center border-0 bg-transparent text-[var(--text-muted)]',
  form: 'flex gap-1.5 p-2',
  input: 'min-w-0 flex-1 rounded-[5px] border border-[var(--border-light)] bg-[var(--bg-base)] px-2 py-[7px] text-[10px] text-[var(--text-primary)] outline-none focus:border-[var(--accent)]',
  submit: 'cursor-pointer rounded-[5px] border-0 bg-[var(--brand)] px-2.5 text-[10px] font-semibold text-white transition-[background] duration-[var(--transition-fast)] hover:not-disabled:bg-[var(--brand-hover)] disabled:cursor-default disabled:opacity-40',
};

const questBannerClass = {
  root: 'flex min-h-[46px] items-center gap-2 border-b border-[var(--border)] bg-[var(--accent-dim)] px-3.5 text-[var(--text-secondary)]',
  error: 'bg-[var(--danger-dim)]',
  icon: 'text-[var(--accent)]',
  errorIcon: 'text-[var(--danger)]',
  content: 'min-w-0 flex-1',
  kicker: 'block text-[10px] font-bold tracking-[0.08em] text-[var(--text-muted)] uppercase',
  title: 'block overflow-hidden text-ellipsis whitespace-nowrap text-xs text-[var(--text-primary)]',
  meta: 'block overflow-hidden text-ellipsis whitespace-nowrap text-[10px] text-[var(--text-muted)]',
  button: 'inline-flex min-h-7 cursor-pointer items-center gap-1.5 rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-elevated)] px-2.5 text-[10px] text-[var(--text-secondary)] shadow-[var(--shadow-sm)] hover:border-[var(--accent)] hover:text-[var(--text-primary)]',
  iconButton: 'grid size-7 cursor-pointer place-items-center rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-elevated)] text-[var(--text-secondary)] shadow-[var(--shadow-sm)] hover:border-[var(--accent)] hover:text-[var(--text-primary)]',
};

const workspaceSelectionClass = {
  card: 'm-[0_8px_10px] flex flex-col gap-1.5 rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--bg-elevated)] p-2.5 shadow-[var(--shadow-sm)]',
  title: 'flex items-start justify-between gap-2',
  titleText: 'flex min-w-0 flex-col gap-0.5',
  name: 'overflow-hidden text-ellipsis whitespace-nowrap text-xs',
  tag: 'text-[10px] text-[var(--text-muted)]',
  liveBadge: 'rounded-lg bg-[var(--success-dim)] px-[5px] py-0.5 font-mono text-[10px] font-bold text-[var(--success)] uppercase',
  label: 'text-[10px] font-bold tracking-[0.06em] text-[var(--text-muted)] uppercase',
  positionGrid: 'grid grid-cols-3 gap-1',
  positionInputWrap: 'grid grid-cols-[14px_1fr] items-center overflow-hidden rounded border border-[var(--border)] bg-[var(--bg-base)] focus-within:border-[var(--accent)]',
  positionAxis: 'text-center font-mono text-[10px] text-[var(--text-muted)]',
  positionInput: 'w-full min-w-0 border-0 bg-transparent px-[3px] py-[5px] font-mono text-[10px] text-[var(--text-secondary)] outline-none',
  button: 'cursor-pointer rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-base)] px-2 py-1.5 text-[10px] font-medium text-[var(--text-secondary)] hover:border-[var(--accent)] hover:text-[var(--accent)]',
};

const viewportClass = {
  container: 'relative flex h-full w-full min-h-0 min-w-0 flex-1 overflow-hidden bg-[#0B0F16]',
  canvas: 'block h-full w-full object-fill',
  selectionOverlay: 'pointer-events-none absolute inset-0 z-20 h-full w-full',
};

const inspectorClass = {
  root: 'flex flex-col gap-2.5 p-2.5',
  section: 'rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--bg-elevated)] shadow-[var(--shadow-sm)]',
  sectionTitle: 'border-b border-[var(--border)] px-2.5 py-2 text-[10px] font-bold uppercase tracking-[0.08em] text-[var(--text-muted)]',
  field: 'grid gap-1.5 px-2.5 py-2 text-[11px] text-[var(--text-secondary)] [&>span]:text-[10px] [&>span]:font-semibold [&>span]:uppercase [&>span]:tracking-[0.06em] [&>span]:text-[var(--text-muted)]',
  fieldRow: 'grid-cols-[minmax(0,1fr)_auto] items-center',
  input: 'w-full min-w-0 rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] px-2 py-1.5 font-[var(--font-sans)] text-[11px] text-[var(--text-primary)] outline-none focus:border-[var(--accent)]',
  select: 'w-full min-w-0 appearance-none rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] px-2 py-1.5 pr-[26px] font-[var(--font-sans)] text-[11px] text-[var(--text-primary)] outline-none focus:border-[var(--accent)]',
  json: 'min-h-20 resize-y font-[var(--font-mono)]',
  colorField: 'grid gap-2 px-2.5 py-2 text-[11px] text-[var(--text-secondary)] [&>span]:text-[10px] [&>span]:font-semibold [&>span]:uppercase [&>span]:tracking-[0.06em] [&>span]:text-[var(--text-muted)]',
  colorCustom: 'flex items-center gap-2',
  colorPicker: 'relative grid size-7 cursor-pointer place-items-center overflow-hidden rounded border border-[var(--border)] [&_input]:absolute [&_input]:inset-0 [&_input]:cursor-pointer [&_input]:opacity-0 [&_span]:size-full',
  colorHex: 'min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] px-2 py-1.5 font-[var(--font-mono)] text-[11px] text-[var(--text-primary)] outline-none focus:border-[var(--accent)]',
  colorPresets: 'grid grid-cols-10 gap-1',
  colorPreset: 'size-5 cursor-pointer rounded border border-[var(--border)] hover:border-[var(--accent)]',
  colorPresetActive: 'ring-1 ring-[var(--accent)]',
  colorChannels: 'grid grid-cols-3 gap-1.5',
  channelInput: 'grid grid-cols-[16px_1fr] items-center overflow-hidden rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] focus-within:border-[var(--accent)] [&_span]:text-center [&_span]:font-mono [&_span]:text-[10px] [&_input]:min-w-0 [&_input]:border-0 [&_input]:bg-transparent [&_input]:px-1 [&_input]:py-1.5 [&_input]:font-mono [&_input]:text-[10px] [&_input]:text-[var(--text-primary)] [&_input]:outline-none',
  vec3: 'grid grid-cols-3 gap-1.5',
  vec4: 'grid grid-cols-4 gap-1.5',
  vecInputWrap: 'grid grid-cols-[16px_1fr] items-center overflow-hidden rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] focus-within:border-[var(--accent)]',
  vecLabel: 'text-center font-mono text-[10px] text-[var(--text-muted)]',
  vecInput: 'min-w-0 border-0 bg-transparent px-1 py-1.5 font-mono text-[10px] text-[var(--text-primary)] outline-none',
  actionRow: 'mt-2 flex gap-1.5',
  actionButton: 'inline-flex min-h-7 cursor-pointer items-center gap-1.5 rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-base)] px-2 text-[10px] text-[var(--text-secondary)] hover:border-[var(--accent)] hover:text-[var(--text-primary)]',
  component: 'border-t border-[var(--border)] first:border-t-0',
  componentHeader: 'flex items-center justify-between gap-2 px-2.5 py-2',
  componentType: 'text-[11px] font-semibold text-[var(--text-primary)]',
  removeButton: 'grid size-6 cursor-pointer place-items-center rounded border border-[var(--border)] bg-transparent text-[var(--text-muted)] hover:border-[var(--danger)] hover:text-[var(--danger)]',
  componentFields: 'border-t border-[var(--border)]',
  emptyField: 'px-2.5 py-2 text-[10px] text-[var(--text-muted)]',
  addRow: 'mt-2 flex gap-1.5',
};

const scriptSurfaceClass = {
  root: 'grid h-full grid-cols-[230px_minmax(0,1fr)] max-[900px]:grid-cols-[170px_minmax(0,1fr)]',
  sidebar: 'overflow-auto border-r border-[var(--border)] bg-[var(--bg-surface)]',
  sidebarHeader: 'flex h-[42px] items-center justify-between border-b border-[var(--border)] bg-[var(--bg-overlay)] px-3 text-[10px] font-semibold text-[var(--text-secondary)] backdrop-blur-xl',
  sidebarEmpty: 'p-3 text-[10px] text-[var(--text-muted)]',
  scriptButton: 'grid w-full cursor-pointer grid-cols-[16px_1fr] gap-x-[7px] gap-y-0.5 border-0 border-b border-[var(--border)] bg-transparent px-[11px] py-[9px] text-left text-[var(--text-muted)] transition-colors duration-150 hover:bg-[var(--bg-hover)] hover:text-[var(--accent)] [&_svg]:row-span-2 [&_svg]:mt-0.5',
  scriptButtonActive: 'bg-[var(--accent-dim)] text-[var(--accent)] shadow-[inset_3px_0_0_var(--accent)]',
  scriptName: 'text-[10px] text-[var(--text-secondary)]',
  scriptPath: 'overflow-hidden text-ellipsis whitespace-nowrap text-[10px]',
  editor: 'flex min-w-0 flex-col bg-[var(--bg-base)]',
  editorHeader: 'flex h-[42px] items-center justify-between border-b border-[var(--border)] bg-[var(--bg-overlay)] px-[13px] font-mono text-[10px] text-[var(--text-secondary)] backdrop-blur-xl',
  editorActions: 'flex items-center gap-2',
  editorHint: 'font-[var(--font-sans)] text-[10px] font-bold tracking-[0.08em] text-[var(--text-muted)]',
  editorButton: 'min-h-[26px] cursor-pointer rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-elevated)] px-[9px] text-[10px] font-semibold text-[var(--text-secondary)] shadow-[var(--shadow-sm)] hover:not-disabled:border-[var(--accent)] hover:not-disabled:text-[var(--text-primary)] disabled:cursor-not-allowed disabled:opacity-50',
  editorPane: 'grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_220px]',
  textarea: 'h-full min-h-0 w-full min-w-0 resize-none overflow-auto whitespace-pre border-0 border-r border-[var(--border)] bg-[#070A0F] px-5 py-[18px] font-mono text-[11px] leading-[1.65] text-[var(--text-primary)] outline-none [tab-size:2] focus:shadow-[inset_0_0_0_1px_var(--accent)]',
  gutter: 'm-0 flex-1 overflow-auto border-l border-white/[0.03] bg-[var(--bg-base)] px-5 py-[18px] font-mono text-[11px] leading-[1.65] text-[var(--text-secondary)] [tab-size:2] [&_code]:block [&_code]:min-w-max',
  gutterButton: 'grid w-full cursor-text grid-cols-[42px_minmax(max-content,1fr)] border-0 bg-transparent p-0 text-left font-inherit text-inherit whitespace-pre hover:bg-[var(--accent-dim)]',
  gutterButtonSelected: 'bg-[var(--accent-dim)]',
  gutterLineNumber: 'select-none text-[#4b5563]',
  gutterLineText: 'pr-6 not-italic text-[var(--text-secondary)]',
  diagnostics: 'max-h-32 overflow-auto border-t border-[var(--border)] bg-[var(--bg-overlay)] px-3 py-2 font-mono text-[10px]',
  diagnostic: 'mb-1.5 grid gap-0.5 last:mb-0',
  diagnosticMessage: 'text-[var(--danger)]',
  diagnosticSuggestion: 'text-[var(--text-secondary)]',
};

function gameSurfaceClass(hierarchyOpen: boolean, inspectorOpen: boolean): string {
  return cx(
    gameClass.surface,
    hierarchyOpen && inspectorOpen && gameClass.surfaceOpen,
    hierarchyOpen && !inspectorOpen && gameClass.surfaceInspectorClosed,
    !hierarchyOpen && inspectorOpen && gameClass.surfaceHierarchyClosed,
    !hierarchyOpen && !inspectorOpen && gameClass.surfaceOnlyMain,
  );
}

function buildStatusClass(status: BuildTargetOption['status']): string {
  return cx(
    buildClass.status,
    status === 'ready' && 'bg-[var(--success-dim)] text-[var(--success)]',
    status === 'planned' && 'bg-[var(--accent-dim)] text-[var(--accent)]',
    status === 'blocked' && 'bg-[var(--warning-dim)] text-[var(--warning)]',
  );
}

function diagnosticLevelClass(level: string): string {
  if (level === 'error') return 'text-[var(--danger)]';
  if (level === 'warn' || level === 'warning') return 'text-[var(--warning)]';
  if (level === 'info') return 'text-[var(--accent)]';
  return '';
}

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

function isVec3Record(value: unknown): value is { x: number; y: number; z: number } {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const record = value as Record<string, unknown>;
  return typeof record.x === 'number' && typeof record.y === 'number' && typeof record.z === 'number';
}

function componentFieldOptions(componentType: string, fieldName: string): string[] | null {
  if (componentType === 'Light' && fieldName === 'kind') return ['directional', 'point', 'spot'];
  if (componentType === 'Rigidbody' && fieldName === 'body_type') return ['dynamic', 'kinematic', 'static'];
  if (componentType === 'Collider' && fieldName === 'shape') return ['box', 'sphere', 'capsule'];
  if (componentType === 'AudioSource' && fieldName === 'spatial_mode') return ['direct', 'spatial'];
  if (componentType === 'AudioSource' && fieldName === 'shape') return ['point', 'cone'];
  if (componentType === 'AudioSource' && fieldName === 'attenuation') return ['none', 'linear', 'inverse'];
  if (componentType === 'AudioListener' && fieldName === 'output_mode') return ['stereo', 'surround'];
  if (componentType === 'AudioListener' && fieldName === 'hrtf_quality') return ['low', 'medium', 'high'];
  return null;
}

function formatFieldLabel(fieldName: string): string {
  return fieldName.replaceAll('_', ' ');
}

function numericWheelDelta(event: React.WheelEvent<HTMLInputElement>, baseStep = 0.1): number {
  const multiplier = event.shiftKey ? 10 : event.altKey ? 0.1 : 1;
  return (event.deltaY < 0 ? baseStep : -baseStep) * multiplier;
}

function nudgeNumericInput(
  input: HTMLInputElement,
  delta: number,
  options: { min?: number; max?: number; precision?: number } = {},
): number | null {
  const current = Number(input.value);
  if (!Number.isFinite(current)) return null;
  const precision = options.precision ?? 2;
  const factor = 10 ** precision;
  let next = Math.round((current + delta) * factor) / factor;
  if (options.min !== undefined) next = Math.max(options.min, next);
  if (options.max !== undefined) next = Math.min(options.max, next);
  input.value = next.toFixed(precision);
  return next;
}

function componentFieldLabel(componentType: string, fieldName: string): string {
  if (componentType === 'Camera' && fieldName === 'clear_color') return 'background';
  if (componentType === 'Light' && fieldName === 'color') return 'light color';
  return formatFieldLabel(fieldName);
}

function usesColorPicker(componentType: string, fieldName: string): boolean {
  return componentType === 'Light' && fieldName === 'color';
}

function vec3ToHex(value: { x: number; y: number; z: number }): string {
  return `#${[value.x, value.y, value.z]
    .map(channel => Math.round(Math.max(0, Math.min(1, channel)) * 255).toString(16).padStart(2, '0'))
    .join('')}`;
}

function hexToVec3(hex: string): { x: number; y: number; z: number } | null {
  const match = /^#?([0-9a-f]{6})$/i.exec(hex);
  if (!match) return null;
  const value = match[1];
  return {
    x: parseInt(value.slice(0, 2), 16) / 255,
    y: parseInt(value.slice(2, 4), 16) / 255,
    z: parseInt(value.slice(4, 6), 16) / 255,
  };
}

const COLOR_PRESETS = [
  '#ffffff',
  '#f8fafc',
  '#fef3c7',
  '#fed7aa',
  '#fecaca',
  '#fbcfe8',
  '#ddd6fe',
  '#d4d4d8',
  '#a7f3d0',
  '#111827',
];

function ComponentFieldEditor({ componentType, fieldName, value, onCommit }: {
  componentType: string;
  fieldName: string;
  value: unknown;
  onCommit: (fieldName: string, value: unknown) => Promise<void>;
}) {
  const [draft, setDraft] = useState(() => formatInspectorValue(value));
  const [hexDraft, setHexDraft] = useState(() => isVec3Record(value) ? vec3ToHex(value) : '');

  useEffect(() => {
    setDraft(formatInspectorValue(value));
    setHexDraft(isVec3Record(value) ? vec3ToHex(value) : '');
  }, [value]);

  const commit = useCallback(async () => {
    const next = parseInspectorValue(draft, value);
    if (JSON.stringify(next) === JSON.stringify(value)) {
      setDraft(formatInspectorValue(value));
      return;
    }
    await onCommit(fieldName, next);
  }, [draft, fieldName, onCommit, value]);

  const options = typeof value === 'string' ? componentFieldOptions(componentType, fieldName) : null;

  if (options) {
    return (
      <label className={inspectorClass.field}>
        <span>{componentFieldLabel(componentType, fieldName)}</span>
        <select
          className={inspectorClass.select}
          value={typeof value === 'string' ? value : ''}
          onChange={event => onCommit(fieldName, event.currentTarget.value)}
        >
          {options.map(option => (
            <option key={option} value={option}>{option}</option>
          ))}
        </select>
      </label>
    );
  }

  if (typeof value === 'boolean') {
    return (
      <label className={cx(inspectorClass.field, inspectorClass.fieldRow)}>
        <span>{componentFieldLabel(componentType, fieldName)}</span>
        <input
          type="checkbox"
          checked={value}
          onChange={event => onCommit(fieldName, event.currentTarget.checked)}
        />
      </label>
    );
  }

  if (isVec3Record(value)) {
    const commitAxis = (axis: 'x' | 'y' | 'z', raw: string) => {
      const nextAxisValue = Number(raw);
      if (!Number.isFinite(nextAxisValue) || nextAxisValue === value[axis]) return;
      onCommit(fieldName, { ...value, [axis]: nextAxisValue });
    };
    const wheelAxis = (
      event: React.WheelEvent<HTMLInputElement>,
      axis: 'x' | 'y' | 'z',
      options?: { min?: number; max?: number; precision?: number },
    ) => {
      event.preventDefault();
      const nextAxisValue = nudgeNumericInput(
        event.currentTarget,
        numericWheelDelta(event, options?.precision === 2 && options?.min === 0 && options?.max === 1 ? 0.01 : 0.1),
        options,
      );
      if (nextAxisValue === null || nextAxisValue === value[axis]) return;
      onCommit(fieldName, { ...value, [axis]: nextAxisValue });
    };
    const isColor = usesColorPicker(componentType, fieldName);
    const hex = vec3ToHex(value);

    if (isColor) {
      const commitColor = (hexValue: string) => {
        const next = hexToVec3(hexValue);
        if (!next) {
          setHexDraft(hex);
          return;
        }
        setHexDraft(vec3ToHex(next));
        onCommit(fieldName, next);
      };

      return (
        <div className={inspectorClass.colorField}>
          <span>{componentFieldLabel(componentType, fieldName)}</span>
          <div className={inspectorClass.colorCustom}>
            <label className={inspectorClass.colorPicker} title="Open color palette">
              <input type="color" value={hex} onChange={event => commitColor(event.currentTarget.value)} />
              <span style={{ backgroundColor: hex }} />
            </label>
            <input
              className={inspectorClass.colorHex}
              value={hexDraft}
              spellCheck={false}
              onChange={event => setHexDraft(event.currentTarget.value)}
              onBlur={() => commitColor(hexDraft)}
              onKeyDown={event => {
                if (event.key === 'Enter') event.currentTarget.blur();
                if (event.key === 'Escape') {
                  setHexDraft(hex);
                  event.currentTarget.blur();
                }
              }}
            />
          </div>
          <div className={inspectorClass.colorPresets} aria-label={`${componentFieldLabel(componentType, fieldName)} presets`}>
            {COLOR_PRESETS.map(preset => (
              <button
                key={preset}
                type="button"
                className={cx(inspectorClass.colorPreset, preset === hex && inspectorClass.colorPresetActive)}
                style={{ backgroundColor: preset }}
                title={preset}
                onClick={() => commitColor(preset)}
              />
            ))}
          </div>
          <div className={inspectorClass.colorChannels}>
            {(['x', 'y', 'z'] as const).map((axis, index) => (
              <label className={inspectorClass.channelInput} key={`${fieldName}-${axis}`}>
                <span>{['R', 'G', 'B'][index]}</span>
                <input
                  defaultValue={value[axis].toFixed(2)}
                  inputMode="decimal"
                  onBlur={event => commitAxis(axis, event.currentTarget.value)}
                  onWheel={event => wheelAxis(event, axis, { min: 0, max: 1, precision: 2 })}
                  onKeyDown={event => {
                    if (event.key === 'Enter') event.currentTarget.blur();
                    if (event.key === 'Escape') event.currentTarget.blur();
                  }}
                />
              </label>
            ))}
          </div>
        </div>
      );
    }

    return (
      <div className={inspectorClass.field}>
        <span>{componentFieldLabel(componentType, fieldName)}</span>
        <div className={inspectorClass.vec3}>
          {(['x', 'y', 'z'] as const).map(axis => (
            <label className={inspectorClass.vecInputWrap} key={`${fieldName}-${axis}`}>
              <span className={inspectorClass.vecLabel}>{axis.toUpperCase()}</span>
              <input
                className={inspectorClass.vecInput}
                defaultValue={value[axis].toFixed(2)}
                inputMode="decimal"
                onBlur={event => commitAxis(axis, event.currentTarget.value)}
                onWheel={event => wheelAxis(event, axis)}
                onKeyDown={event => {
                  if (event.key === 'Enter') event.currentTarget.blur();
                  if (event.key === 'Escape') event.currentTarget.blur();
                }}
              />
            </label>
          ))}
        </div>
      </div>
    );
  }

  const isObject = Boolean(value && typeof value === 'object' && !Array.isArray(value));
  const wheelNumberField = async (event: React.WheelEvent<HTMLInputElement>) => {
    if (typeof value !== 'number') return;
    event.preventDefault();
    const current = Number(draft);
    if (!Number.isFinite(current)) return;
    const next = Math.round((current + numericWheelDelta(event)) * 100) / 100;
    const formatted = next.toFixed(2);
    setDraft(formatted);
    await onCommit(fieldName, next);
  };

  return (
    <label className={inspectorClass.field}>
      <span>{componentFieldLabel(componentType, fieldName)}</span>
      {isObject ? (
        <textarea
          className={cx(inspectorClass.input, inspectorClass.json)}
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
          className={inspectorClass.input}
          value={draft}
          inputMode={typeof value === 'number' || (Array.isArray(value) && value.every(item => typeof item === 'number')) ? 'decimal' : undefined}
          onChange={event => setDraft(event.currentTarget.value)}
          onBlur={commit}
          onWheel={wheelNumberField}
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
  return Boolean(path && /\.(aster|rhai|js|ts|tsx|lua|rs)$/i.test(path));
}

// Infer a scene-tree icon from a node's name/tag, the way Godot shows a
// type glyph per node. Heuristic only — the backend exposes no component
// list on the lightweight scene-tree payload.
function sceneNodeIcon(object: SceneObject, selected: boolean): React.ReactNode {
  const hint = `${object.name} ${object.tag}`.toLowerCase();
  const cls = selected ? gameClass.hierarchyIconSelected : gameClass.hierarchyIcon;
  let glyph: React.ReactNode;
  if (/camera/.test(hint)) glyph = <IconView size={14} />;
  else if (/light|lamp|sun/.test(hint)) glyph = <IconSun size={14} />;
  else if (/audio|sound|music|speaker/.test(hint)) glyph = <IconAudio size={14} />;
  else if (/script|behavior|behaviour/.test(hint)) glyph = <IconCode size={14} />;
  else if (/mesh|model|cube|sphere|player|prop/.test(hint)) glyph = <IconModel size={14} />;
  else glyph = <span className="size-1.5 rounded-full bg-current" />;
  return <span className={cx(cls, 'grid size-3.5 place-items-center')}>{glyph}</span>;
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
    <div className={workspaceSelectionClass.card}>
      <div className={workspaceSelectionClass.title}>
        <div className={workspaceSelectionClass.titleText}>
          <strong className={workspaceSelectionClass.name}>{object.name}</strong>
          <span className={workspaceSelectionClass.tag}>{object.tag || t('entity_untagged')}</span>
        </div>
        <span className={workspaceSelectionClass.liveBadge}>{t('badge_live')}</span>
      </div>
      <label className={workspaceSelectionClass.label}>{t('prop_position')}</label>
      <div className={workspaceSelectionClass.positionGrid}>
        {position.map((value, index) => (
          <label className={workspaceSelectionClass.positionInputWrap} key={index}>
            <span
              className={cx(
                workspaceSelectionClass.positionAxis,
                index === 0 && 'text-[var(--axis-x)]',
                index === 1 && 'text-[var(--axis-y)]',
                index === 2 && 'text-[var(--axis-z)]',
              )}
            >
              {['X', 'Y', 'Z'][index]}
            </span>
            <input
              className={workspaceSelectionClass.positionInput}
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
      <button className={workspaceSelectionClass.button} onClick={onFocus}>{t('editor_focus_viewport')}</button>
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

function ViewportCanvas({ sceneVersion = 0, cameraRef, onCameraChange, onResize, viewMode, playMode, editorCamera }: {
  sceneVersion?: number;
  cameraRef?: React.MutableRefObject<{
    yaw: number; pitch: number; distance: number;
    targetX: number; targetY: number; targetZ: number;
  }>;
  onCameraChange?: () => void;
  onResize?: (size: { width: number; height: number }) => void;
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
  const onResizeRef = useRef(onResize);
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
  onResizeRef.current = onResize;

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
    onResizeRef.current?.(sizeRef.current);
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width, height } = entry.contentRect;
        if (width > 0 && height > 0) {
          const nextSize = { width: Math.round(width), height: Math.round(height) };
          sizeRef.current = nextSize;
          onResizeRef.current?.(nextSize);
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
      className={viewportClass.container}
      onMouseDown={onMouseDown}
      onContextMenu={(e) => e.preventDefault()}
    >
      <canvas ref={canvasRef} className={viewportClass.canvas} />
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
      className={viewportClass.selectionOverlay}
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
    >
      {/* Selection ring */}
      <circle
        cx={screenPos.x}
        cy={screenPos.y}
        r={18}
        fill="none"
        stroke="var(--accent, #A1A1AA)"
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
        fill="rgba(39, 39, 42, 0.9)"
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

// ─── Editor Page ────────────────────────────────────────────────────────────

export default function EditorPage({
  onCloseProject,
  onOpenSettings,
  onOpenQuest,
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
  const [aiPanelOpen, setAiPanelOpen] = useState(() => (
    window.localStorage.getItem('aster.aiPanelOpen') !== 'false'
  ));
  const [inspectorOpen, setInspectorOpen] = useState(() => (
    window.localStorage.getItem('aster.inspectorOpen') !== 'false'
  ));
  const [hierarchyOpen, setHierarchyOpen] = useState(() => (
    window.localStorage.getItem('aster.hierarchyOpen') !== 'false'
  ));
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
  const [scriptDiagnostics, setScriptDiagnostics] = useState<AsterScriptDiagnostic[]>([]);
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
  const [collapsedNodes, setCollapsedNodes] = useState<Set<string>>(() => new Set());
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

  // Flatten the scene tree into ordered rows with depth, parent-child grouping,
  // and collapse handling — the render layer just maps over this.
  const hierarchyRows = useMemo(() => {
    const childrenOf = new Map<string | null, SceneObject[]>();
    for (const object of sceneTree) {
      const key = object.parent_id ?? null;
      const list = childrenOf.get(key);
      if (list) list.push(object);
      else childrenOf.set(key, [object]);
    }
    const rows: Array<{ object: SceneObject; depth: number; hasChildren: boolean }> = [];
    const walk = (parentId: string | null, depth: number) => {
      for (const object of childrenOf.get(parentId) ?? []) {
        const hasChildren = childrenOf.has(object.id);
        rows.push({ object, depth, hasChildren });
        if (hasChildren && !collapsedNodes.has(object.id)) {
          walk(object.id, depth + 1);
        }
      }
    };
    walk(null, 0);
    return rows;
  }, [sceneTree, collapsedNodes]);

  const toggleNodeCollapsed = useCallback((id: string) => {
    setCollapsedNodes(current => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  // Gizmo state
  const [activeTool] = useState<'view' | 'move' | 'rotate' | 'scale'>('move');
  const [transformSpace] = useState<'global' | 'local'>('global');
  const [selectedPosition, setSelectedPosition] = useState<Vec3 | null>(null);
  const handleViewportResize = useCallback((size: { width: number; height: number }) => {
    setViewportSize(current => {
      if (current.width === size.width && current.height === size.height) return current;
      return size;
    });
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

  useEffect(() => {
    window.localStorage.setItem('aster.aiPanelOpen', String(aiPanelOpen));
  }, [aiPanelOpen]);

  useEffect(() => {
    if (contextualRequest || aiWorkspace?.plan || aiWorkspace?.completedBundle || aiWorkspace?.status === 'thinking' || aiWorkspace?.status === 'executing' || aiWorkspace?.status === 'error') {
      setAiPanelOpen(true);
    }
  }, [aiWorkspace?.completedBundle, aiWorkspace?.plan, aiWorkspace?.status, contextualRequest]);

  useEffect(() => {
    window.localStorage.setItem('aster.inspectorOpen', String(inspectorOpen));
  }, [inspectorOpen]);

  useEffect(() => {
    window.localStorage.setItem('aster.hierarchyOpen', String(hierarchyOpen));
  }, [hierarchyOpen]);

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
        .filter(entry => /script|model/i.test(entry.kind) || /\.(aster|rhai|amdl|js|ts|lua)$/i.test(entry.path))
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
    setScriptDiagnostics([]);
    setArtifactSelection(null);
    setArtifactQuestionOpen(false);
  }, [selectedScript, workspaceView]);

  useEffect(() => {
    const lowerPath = selectedScript?.toLowerCase() ?? '';
    const checkMethod = lowerPath.endsWith('.aster')
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
        .then(result => setScriptDiagnostics(result.diagnostics))
        .catch(() => setScriptDiagnostics([]));
    }, 350);
    return () => window.clearTimeout(timer);
  }, [scriptContent, selectedScript]);

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

  const nudgeSelectedTransform = useCallback(async (
    field: 'position' | 'rotation' | 'scale',
    index: number,
    event: React.WheelEvent<HTMLInputElement>,
  ) => {
    if (!selectedId || !selectedEntityDetails) return;
    event.preventDefault();
    const nextValue = nudgeNumericInput(event.currentTarget, numericWheelDelta(event));
    if (nextValue === null) return;
    const next = [...selectedEntityDetails.transform[field]];
    next[index] = nextValue;
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
      const lowerPath = selectedScript.toLowerCase();
      const checkMethod = lowerPath.endsWith('.aster')
        ? 'project/check_script'
        : lowerPath.endsWith('.amdl')
          ? 'project/check_amdl'
          : null;
      if (checkMethod) {
        const validation = await rpc<{ valid: boolean; diagnostics: TextAssetDiagnostic[] }>(
          checkMethod,
          { path: selectedScript, source: scriptContent },
        );
        setScriptDiagnostics(validation.diagnostics);
        if (!validation.valid) return;
      }
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

  // Derive selected entity name
  const selectedEntityName = selectedId
    ? sceneTree.find(o => o.id === selectedId)?.name ?? null
    : null;
  const selectedObject = selectedId
    ? sceneTree.find(o => o.id === selectedId) ?? null
    : null;
  const scriptDirty = scriptContent !== scriptSavedContent;
  const hasDiagnostics = consoleEntries.length > 0;
  const visibleWorkspaceTabs = ([
    ['game', 'Scene', <IconView key="game" />] as const,
    ['assets', 'Assets', <IconFile key="assets" />] as const,
    ['scripts', t('tab_scripts'), <IconCode key="scripts" />] as const,
    ['build', 'Build', <IconPackage key="build" />] as const,
  ]);
  const pendingAiDecisionCount = aiWorkspace?.plan?.operations.filter(operation => operation.permission_kind !== 'read').length ?? 0;

  useEffect(() => {
    const visible = new Set<WorkspaceView>([
      ...visibleWorkspaceTabs.map(([view]) => view),
      ...(aiWorkspace?.plan ? ['tasks' as WorkspaceView] : []),
      ...(hasDiagnostics ? ['diagnostics' as WorkspaceView] : []),
    ]);
    if (!visible.has(workspaceView)) {
      setWorkspaceView('game');
    }
  }, [aiWorkspace?.plan, hasDiagnostics, visibleWorkspaceTabs, workspaceView]);

  // ── Render ──

  if (!shellState) {
    return <div className={shellClass.loading}>{t('loading_editor')}</div>;
  }

  return (
    <div className={shellClass.root}>
      {/* Editor toolbar */}
      <div className={shellClass.toolbar}>
        <div className={shellClass.toolbarProject}>
          <span className={shellClass.toolbarProjectKicker}>{t('editor_workspace_kicker')}</span>
          <span className={shellClass.toolbarProjectName}>{shellState.project_name || t('editor_untitled')}</span>
        </div>
        <div className={shellClass.toolbarSpacer} />
        <button
          className={toolButtonClass({ size: 'toolbar' })}
          onClick={() => rpc('shell/save_scene').then(() => refreshSceneTree())}
          disabled={!shellState.scene_dirty}
          title="Save scene"
        >
          <IconSave />
        </button>
        <button
          className={toolButtonClass({ size: 'toolbar' })}
          onClick={() => rpc('shell/undo').then(() => refreshSceneTree())}
          disabled={!shellState.can_undo}
          title="Undo"
        >
          <IconUndo />
        </button>
        <button
          className={toolButtonClass({ size: 'toolbar' })}
          onClick={() => rpc('shell/redo').then(() => refreshSceneTree())}
          disabled={!shellState.can_redo}
          title="Redo"
        >
          <IconRedo />
        </button>
        <button className={toolButtonClass({ variant: 'play', size: 'toolbar' })} onClick={openGameView} title={t('editor_open_game_view')}><IconPlay /></button>
        <button className={toolButtonClass({ size: 'toolbar' })} onClick={() => setWorkspaceView('build')} title="Build and package"><IconPackage /></button>
        <button className={toolButtonClass({ size: 'toolbar', extra: 'quest-mode-btn select-none' })} onClick={onOpenQuest} title="Open Quest Mode"><IconProjects /> <span>Quests</span></button>
        <button className={toolButtonClass({ size: 'toolbar' })} onClick={handleClose} title={t('editor_close')}><IconX /></button>
      </div>

      {/* Main body: viewport + AI panel */}
      <div className={shellClass.body}>
        <main className={workspaceClass.root}>
          {questArtifact && (
            <div className={questBannerClass.root}>
              <IconProjects className={questBannerClass.icon} />
              <div className={questBannerClass.content}>
                <span className={questBannerClass.kicker}>Opened from Quest</span>
                <strong className={questBannerClass.title}>{questArtifact.questTitle}</strong>
                <small className={questBannerClass.meta}>{questArtifact.kind.replaceAll('_', ' ')} · {questArtifact.label}</small>
              </div>
              <button className={questBannerClass.button} onClick={onOpenQuest}><IconProjects /> Return</button>
              <button className={questBannerClass.iconButton} onClick={onDismissQuestArtifact} title="Dismiss"><IconX /></button>
            </div>
          )}
          <nav className={workspaceClass.tabs} role="tablist" aria-label="Editor surfaces">
            {visibleWorkspaceTabs.map(([view, label, icon]) => (
              <button
                key={view}
                className={cx(workspaceClass.tab, workspaceView === view && workspaceClass.tabActive)}
                onClick={() => setWorkspaceView(view)}
                role="tab"
                aria-selected={workspaceView === view}
              >
                {icon}<span>{label}</span>
                {view === 'assets' && assets.length > 0 && <b className={workspaceClass.tabBadge}>{assets.length}</b>}
                {view === 'scripts' && scripts.length > 0 && <b className={workspaceClass.tabBadge}>{scripts.length}</b>}
              </button>
            ))}
            {aiWorkspace?.plan && (
              <button
                className={cx(workspaceClass.tab, workspaceView === 'tasks' && workspaceClass.tabActive)}
                onClick={() => {
                  setWorkspaceView('tasks');
                  setAiPanelOpen(true);
                }}
                role="tab"
                aria-selected={workspaceView === 'tasks'}
              >
                <IconCheck /><span>Review changes</span>
                <b className={workspaceClass.tabBadge}>{aiWorkspace.plan.operations.length}</b>
              </button>
            )}
            {hasDiagnostics && (
              <button
                className={cx(workspaceClass.tab, workspaceView === 'diagnostics' && workspaceClass.tabActive)}
                onClick={() => setWorkspaceView('diagnostics')}
                role="tab"
                aria-selected={workspaceView === 'diagnostics'}
              >
                <IconAlertCircle /><span>View diagnostics</span>
                {consoleEntries.length > 0 && <b className={workspaceClass.tabBadge}>{consoleEntries.length}</b>}
              </button>
            )}
          </nav>

          <section className={cx(workspaceClass.view, workspaceView === 'game' && workspaceClass.viewGame)}>
            {workspaceView === 'prd' && openedQuestArtifact && <article className={prdClass.document} onMouseUp={selectDocumentText}>
              <header className={prdClass.header}><span className={prdClass.kicker}>{t('prd_header')}</span><strong className={prdClass.title}>{shellState.project_name || t('prd_untitled_game')}</strong><p className={prdClass.description}>{t('prd_brief_desc')}</p></header>
              <section className={prdClass.section}><h2 className={prdClass.sectionTitle}>{t('prd_vision')}</h2><p className={prdClass.bodyText}>{t('prd_vision_text').replace('{scene_count}', String(sceneTree.length)).replace('{script_count}', String(scripts.length))}</p></section>
              <section className={prdClass.section}><h2 className={prdClass.sectionTitle}>{t('prd_current_scope')}</h2><div className={prdClass.grid}><div className={prdClass.gridCard}><span className={prdClass.gridLabel}>{t('prd_scope_player_exp')}</span><strong className={prdClass.gridValue}>{t('prd_scope_playable')}</strong></div><div className={prdClass.gridCard}><span className={prdClass.gridLabel}>{t('prd_scope_world')}</span><strong className={prdClass.gridValue}>{sceneTree.length} {t('prd_scope_authored')}</strong></div><div className={prdClass.gridCard}><span className={prdClass.gridLabel}>{t('prd_scope_automation')}</span><strong className={prdClass.gridValue}>{t('prd_scope_review')}</strong></div><div className={prdClass.gridCard}><span className={prdClass.gridLabel}>{t('prd_scope_delivery')}</span><strong className={prdClass.gridValue}>{t('prd_scope_verification')}</strong></div></div></section>
              <section className={prdClass.section}><h2 className={prdClass.sectionTitle}>{t('prd_acceptance')}</h2><ul className={prdClass.list}><li className={prdClass.bodyText}>{t('prd_criteria_1')}</li><li className={prdClass.bodyText}>{t('prd_criteria_2')}</li><li className={prdClass.bodyText}>{t('prd_criteria_3')}</li><li className={prdClass.bodyText}>{t('prd_criteria_4')}</li></ul></section>
            </article>}

            {workspaceView === 'tasks' && <div className={taskClass.board}>
              <header className={taskClass.header}><div><span className={taskClass.kicker}>AI artifact</span><h1 className={taskClass.title}>{aiWorkspace?.plan ? 'Changes need review' : openedQuestArtifact ? openedQuestArtifact.title : 'AI work'}</h1></div><small className={taskClass.meta}>{shellState.project_name} · {sceneTree.length} {t('label_objects')}</small></header>
              {openedQuestArtifact && (
                <section className={taskClass.artifactCard}>
                  <div>
                    <span className={taskClass.artifactKicker}>{questArtifact?.kind.replaceAll('_', ' ')}</span>
                    <strong className={taskClass.artifactTitle}>{openedQuestArtifact.title}</strong>
                    <p className={taskClass.artifactDescription}>{openedQuestArtifact.description}</p>
                    {openedQuestArtifact.focusPath && <small className={taskClass.artifactPath}>{openedQuestArtifact.focusPath}</small>}
                  </div>
                  <div className={taskClass.artifactActions}>
                    {openedQuestArtifact.surface !== 'tasks' && (
                      <button className={taskClass.artifactButton} onClick={() => setWorkspaceView(openedQuestArtifact.surface)}>
                        <IconFile /> Open surface
                      </button>
                    )}
                    <button
                      className={taskClass.artifactButton}
                      onClick={() => setContextualRequest({
                        id: Date.now(),
                        prompt: `Inspect this Quest artifact and suggest the next local editor check.\n\n${openedQuestArtifact.title}\n${openedQuestArtifact.description}\n${openedQuestArtifact.focusPath ?? ''}`,
                      })}
                    >
                      <IconSparkles /> Ask AI
                    </button>
                    <button className={taskClass.artifactButton} onClick={onOpenQuest}><IconProjects /> Return</button>
                  </div>
                </section>
              )}
              <section className={taskClass.operations}><div className={taskClass.operationsTitle}><span>{t('task_proposed_ops')}</span></div>
                {!aiWorkspace?.plan ? <div className={productEmptyClass}><IconProjects className={productEmptyIconClass} /><strong className={productEmptyTitleClass}>{t('task_no_plan')}</strong><span className={productEmptyTextClass}>{t('task_no_plan_desc')}</span></div> : aiWorkspace.plan.operations.map(operation => <div className={taskClass.operationRow} key={operation.index}><span className={cx(taskClass.operationPermission, taskOperationPermissionLabelClass(operation.permission_kind))}>{operation.permission_kind.toUpperCase()}</span><p className={taskClass.operationPreview}>{operation.preview}</p><small className={taskClass.operationState}>{operation.permission_kind === 'read' ? t('op_auto_allowed') : aiWorkspace.approved.has(operation.index) ? t('op_allowed') : aiWorkspace.denied.has(operation.index) ? t('op_denied_once') : t('op_awaiting')}</small></div>)}
              </section>
              {aiWorkspace?.plan && <footer className={taskClass.footer}><button className={buttonClass('ghost')} onClick={aiWorkspace.discardProposal}>{t('btn_discard')}</button><button className={buttonClass('primary')} disabled={aiWorkspace.approved.size === 0} onClick={aiWorkspace.applyApproved}>{t('btn_continue_allowed').replace('{count}', String(aiWorkspace.approved.size))}</button></footer>}
            </div>}

            {workspaceView === 'game' && <div className={gameSurfaceClass(hierarchyOpen, inspectorOpen)}>
              {hierarchyOpen && <aside className={gameClass.sidePanel}>
                <header className={gameClass.panelHeader}>
                  <div className={gameClass.panelHeaderText}><span>Hierarchy</span><strong className={gameClass.panelHeaderTitle}>{sceneTree.length} objects</strong></div>
                  <div className={gameClass.panelHeaderActions}>
                    <button className={gameClass.iconButton} onClick={() => createSceneObject()} title="Create object"><IconPlus /></button>
                    <button className={gameClass.iconButton} onClick={() => setHierarchyOpen(false)} title="Close Hierarchy"><IconX /></button>
                  </div>
                </header>
                <div className={gameClass.hierarchyList}>
                  {hierarchyRows.length === 0 ? (
                    <p className={gameClass.empty}>No scene objects.</p>
                  ) : hierarchyRows.map(({ object, depth, hasChildren }) => {
                    const selected = selectedId === object.id;
                    const collapsed = collapsedNodes.has(object.id);
                    return (
                      <div
                        key={object.id}
                        className={cx(gameClass.hierarchyItem, selected && gameClass.hierarchyItemSelected)}
                        onClick={() => selectSceneObject(object.id)}
                        style={{ paddingLeft: depth * 14 + 6 }}
                      >
                        {hasChildren ? (
                          <span
                            className={gameClass.hierarchyTwisty}
                            onClick={event => { event.stopPropagation(); toggleNodeCollapsed(object.id); }}
                            title={collapsed ? 'Expand' : 'Collapse'}
                          >
                            {collapsed ? <IconChevronRight /> : <IconChevronDown />}
                          </span>
                        ) : (
                          <span className={gameClass.hierarchyTwistySpacer} />
                        )}
                        {sceneNodeIcon(object, selected)}
                        <span className={gameClass.hierarchyName}>{object.name}</span>
                        {object.tag && <span className={gameClass.hierarchyTag}>{object.tag}</span>}
                      </div>
                    );
                  })}
                </div>
              </aside>}

              <section className={gameClass.mainPanel}>
                <div className={gameClass.previewBar}>
                  <div className={gameClass.previewBarGroup}><span className={gameClass.liveDot} />Scene/Game View</div>
                  <div className={gameClass.modeSwitch}>
                    {!hierarchyOpen && (
                      <button className={gameClass.modeButton} onClick={() => setHierarchyOpen(true)}>Hierarchy</button>
                    )}
                    <button className={cx(gameClass.modeButton, viewMode === '2d' && gameClass.modeButtonActive)} onClick={() => setViewMode('2d')}>2D</button>
                    <button className={cx(gameClass.modeButton, viewMode === '3d' && gameClass.modeButtonActive)} onClick={() => setViewMode('3d')}>3D</button>
                    <button className={gameClass.modeButton} onClick={() => rpc('shell/undo').then(() => refreshSceneTree())} disabled={!shellState.can_undo}>Undo</button>
                    <button className={gameClass.modeButton} onClick={() => rpc('shell/redo').then(() => refreshSceneTree())} disabled={!shellState.can_redo}>Redo</button>
                    <button className={gameClass.modeButton} onClick={() => openNativeSceneView({
                      yaw: cameraRef.current.yaw,
                      pitch: cameraRef.current.pitch,
                      distance: cameraRef.current.distance,
                      targetX: cameraRef.current.targetX,
                      targetY: cameraRef.current.targetY,
                      targetZ: cameraRef.current.targetZ,
                    })}>Native View</button>
                    <button className={gameClass.modeButton} onClick={openGameView}><IconPlay /> Run</button>
                    {!inspectorOpen && (
                      <button className={gameClass.modeButton} onClick={() => setInspectorOpen(true)}>Inspector</button>
                    )}
                  </div>
                </div>
                <div className={gameClass.createPresets} aria-label="Create scene preset">
                  <button className={gameClass.createButton} onClick={() => createSceneObject()}><IconPlus /> Empty</button>
                  <button className={gameClass.createButton} onClick={() => createPresetObject('Camera', 'Camera')}><IconPlus /> Camera</button>
                  <button className={gameClass.createButton} onClick={() => createPresetObject('Light', 'Light')}><IconPlus /> Light</button>
                  <button className={gameClass.createButton} onClick={() => createPresetObject('Mesh Object', 'MeshRenderer')}><IconPlus /> Mesh</button>
                  <button className={gameClass.createButton} onClick={() => createPresetObject('Audio Source', 'AudioSource')}><IconPlus /> Audio</button>
                  <button className={gameClass.createButton} onClick={() => createPresetObject('Rigid Body', 'Rigidbody')}><IconPlus /> Rigidbody</button>
                  <button className={gameClass.createButton} onClick={() => createPresetObject('Collider', 'Collider')}><IconPlus /> Collider</button>
                  <button className={gameClass.createButton} onClick={createBehaviorObject}><IconCode /> Behavior</button>
                </div>
                <div className={gameClass.previewCanvas} onClick={handleViewportClick}>
                  <ViewportCanvas
                    sceneVersion={sceneVersion}
                    cameraRef={cameraRef}
                    onCameraChange={handleCameraChange}
                    onResize={handleViewportResize}
                    viewMode={viewMode}
                    editorCamera
                  />
                  <SelectionOverlay sceneTree={sceneTree} selectedId={selectedId} camera={cameraRef.current} width={viewportSize.width} height={viewportSize.height} viewMode={viewMode} />
                </div>
              </section>

              {inspectorOpen && <aside className={gameClass.inspectorPanel}>
                <header className={gameClass.panelHeader}>
                  <div className={gameClass.panelHeaderText}>
                    <span>Inspector</span>
                    {selectedEntityDetails && <strong className={gameClass.panelHeaderTitle}>{selectedEntityDetails.name}</strong>}
                  </div>
                  <button className={gameClass.iconButton} onClick={() => setInspectorOpen(false)} title="Close Inspector">
                    <IconX />
                  </button>
                </header>
                {!selectedEntityDetails ? (
                  <div className={gameClass.empty}>Select an object in the viewport or hierarchy.</div>
                ) : (
                  <div className={inspectorClass.root}>
                    <section className={inspectorClass.section}>
                      <div className={inspectorClass.sectionTitle}>Object</div>
                      <label className={inspectorClass.field}>
                        <span>Name</span>
                        <input
                          className={inspectorClass.input}
                          value={selectedEntityNameDraft}
                          onChange={event => setSelectedEntityNameDraft(event.target.value)}
                          onBlur={renameSelectedObject}
                          onKeyDown={event => {
                            if (event.key === 'Enter') event.currentTarget.blur();
                            if (event.key === 'Escape') setSelectedEntityNameDraft(selectedEntityDetails.name);
                          }}
                        />
                      </label>
                      <label className={inspectorClass.field}>
                        <span>Parent</span>
                        <select
                          className={inspectorClass.select}
                          value={selectedObject?.parent_id ?? ''}
                          onChange={event => reparentSelectedObject(event.currentTarget.value)}
                        >
                          <option value="">Scene root</option>
                          {validParentOptions.map(object => (
                            <option key={object.id} value={object.id}>{object.name}</option>
                          ))}
                        </select>
                      </label>
                      <div className={inspectorClass.actionRow}>
                        <button className={inspectorClass.actionButton} onClick={duplicateSelectedObject}><IconCopy /> Duplicate</button>
                        <button className={inspectorClass.actionButton} onClick={deleteSelectedObject}><IconTrash /> Delete</button>
                      </div>
                    </section>
                    {(['position', 'rotation', 'scale'] as const).map(field => (
                      <section className={inspectorClass.section} key={field}>
                        <div className={inspectorClass.sectionTitle}>{field}</div>
                        <div className={field === 'rotation' ? inspectorClass.vec4 : inspectorClass.vec3}>
                          {selectedEntityDetails.transform[field].map((value, index) => (
                            <label className={inspectorClass.vecInputWrap} key={`${field}-${index}`}>
                              <span className={inspectorClass.vecLabel}>{['X', 'Y', 'Z', 'W'][index]}</span>
                              <input
                                className={inspectorClass.vecInput}
                                defaultValue={value.toFixed(2)}
                                inputMode="decimal"
                                onBlur={event => updateSelectedTransform(field, index, event.currentTarget.value)}
                                onWheel={event => nudgeSelectedTransform(field, index, event)}
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
                    <section className={inspectorClass.section}>
                      <div className={inspectorClass.sectionTitle}>Components</div>
                      {selectedEntityDetails.components.map(component => (
                        <div className={inspectorClass.component} key={component.type}>
                          <div className={inspectorClass.componentHeader}>
                            <span className={inspectorClass.componentType}>{component.type}</span>
                            <button className={inspectorClass.removeButton} onClick={() => removeSelectedComponent(component.type)} title="Remove component">×</button>
                          </div>
                          <div className={inspectorClass.componentFields}>
                            {Object.entries(component.data ?? {}).length === 0 ? (
                              <div className={inspectorClass.emptyField}>No editable fields</div>
                            ) : Object.entries(component.data ?? {}).map(([fieldName, value]) => (
                              <ComponentFieldEditor
                                key={`${component.type}-${fieldName}`}
                                componentType={component.type}
                                fieldName={fieldName}
                                value={value}
                                onCommit={(name, nextValue) => updateSelectedComponentField(component.type, name, nextValue)}
                              />
                            ))}
                          </div>
                        </div>
                      ))}
                      <div className={inspectorClass.addRow}>
                        <select className={inspectorClass.select} value={addComponentType} onChange={event => setAddComponentType(event.target.value)}>
                          {['Camera', 'Light', 'MeshRenderer', 'Rigidbody', 'Collider', 'AudioSource', 'AudioListener', 'Script'].map(type => (
                            <option key={type} value={type}>{type}</option>
                          ))}
                        </select>
                        <button className={inspectorClass.actionButton} onClick={addSelectedComponent}><IconPlus /> Add</button>
                      </div>
                    </section>
                  </div>
                )}
              </aside>}
            </div>}

            {workspaceView === 'assets' && <div className={surfaceClass.root}>
              <header className={surfaceClass.header}>
                <div>
                  <span className={surfaceClass.headerKicker}>Project/Assets</span>
                  <strong className={surfaceClass.headerTitle}>{assets.length} assets</strong>
                </div>
                <div className={surfaceClass.toolbar}>
                  <button className={surfaceClass.button} onClick={() => createProjectAsset('script')} disabled={assetsBusy}>Script</button>
                  <button className={surfaceClass.button} onClick={() => createProjectAsset('material')} disabled={assetsBusy}>Material</button>
                  <button className={surfaceClass.button} onClick={() => createProjectAsset('prefab')} disabled={assetsBusy}>Prefab</button>
                  <button className={surfaceClass.button} onClick={() => createProjectAsset('scene')} disabled={assetsBusy}>Scene</button>
                  <button className={surfaceClass.button} onClick={refreshProjectAssets} disabled={assetsBusy}>
                    {assetsBusy ? 'Refreshing' : 'Refresh'}
                  </button>
                </div>
              </header>
              <div className={surfaceClass.list}>
                {assets.length === 0 ? (
                  <div className={surfaceClass.empty}>No imported assets found.</div>
                ) : assets.map(asset => {
                  const canOpenScript = isScriptPath(asset.source_path);
                  return (
                    <article className={assetsClass.row} key={asset.guid || asset.source_path}>
                      <IconFile />
                      <div className={assetsClass.rowMain}>
                        <strong className={assetsClass.rowTitle}>{asset.source_path.split('/').pop() || asset.source_path}</strong>
                        <span className={assetsClass.rowMeta}>{asset.source_path}</span>
                      </div>
                      <small className={assetsClass.rowMeta}>{asset.kind}</small>
                      <small className={assetsClass.rowMeta}>{asset.importer || 'default'}</small>
                      <div className={assetsClass.actions}>
                        {canOpenScript ? (
                          <button className={surfaceClass.button} onClick={() => {
                            setSelectedScript(asset.source_path);
                            setWorkspaceView('scripts');
                          }}>Open</button>
                        ) : (
                          <button className={surfaceClass.button} onClick={event => {
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
                        <button className={surfaceClass.button} onClick={event => revealAssetReferences(asset, event)} disabled={assetsBusy}>Refs</button>
                        <button className={surfaceClass.button} onClick={() => reloadAsset(asset.source_path)} disabled={assetsBusy}>Reload</button>
                      </div>
                    </article>
                  );
                })}
              </div>
            </div>}

            {workspaceView === 'scripts' && <div className={scriptSurfaceClass.root}>
              <aside className={scriptSurfaceClass.sidebar}>
                <header className={scriptSurfaceClass.sidebarHeader}>{t('scripts_header')} <span>{scripts.length}</span></header>
                {scripts.length === 0 ? <p className={scriptSurfaceClass.sidebarEmpty}>{t('scripts_empty')}</p> : scripts.map(path => (
                  <button key={path} className={cx(scriptSurfaceClass.scriptButton, selectedScript === path && scriptSurfaceClass.scriptButtonActive)} onClick={() => setSelectedScript(path)}>
                    <IconCode /><span className={scriptSurfaceClass.scriptName}>{path.split('/').pop()}</span><small className={scriptSurfaceClass.scriptPath}>{path}</small>
                  </button>
                ))}
              </aside>
              <article className={scriptSurfaceClass.editor}>
                <header className={scriptSurfaceClass.editorHeader}>
                  <span>{selectedScript || t('scripts_select')}{scriptDirty ? ' *' : ''}</span>
                  <div className={scriptSurfaceClass.editorActions}>
                    {scriptDiagnostics.length > 0 && <b className="text-[var(--danger)]">{scriptDiagnostics.length} errors</b>}
                    <b className={scriptSurfaceClass.editorHint}>{t('scripts_line_select_hint')}</b>
                    <button className={scriptSurfaceClass.editorButton} onClick={saveSelectedScript} disabled={!selectedScript || !scriptDirty || scriptSaving}>
                      {scriptSaving ? 'Saving' : 'Save'}
                    </button>
                  </div>
                </header>
                <div className={scriptSurfaceClass.editorPane}>
                  <textarea
                    className={scriptSurfaceClass.textarea}
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
                  <pre className={scriptSurfaceClass.gutter}><code>{(scriptContent || '// Aster-generated scripts will appear here.').split('\n').map((line, index) => { const lineNumber = index + 1; const selected = scriptLineRange && lineNumber >= scriptLineRange[0] && lineNumber <= scriptLineRange[1]; return <button key={lineNumber} className={cx(scriptSurfaceClass.gutterButton, selected && scriptSurfaceClass.gutterButtonSelected)} onClick={event => selectScriptLine(lineNumber, event.shiftKey, event)}><span className={scriptSurfaceClass.gutterLineNumber}>{lineNumber}</span><i className={scriptSurfaceClass.gutterLineText}>{line || ' '}</i></button>; })}</code></pre>
                </div>
                {scriptDiagnostics.length > 0 && <div className={scriptSurfaceClass.diagnostics}>
                  {scriptDiagnostics.map((diagnostic, index) => <div className={scriptSurfaceClass.diagnostic} key={`${diagnostic.code}-${diagnostic.line ?? 0}-${index}`}>
                    <strong className={scriptSurfaceClass.diagnosticMessage}>
                      {diagnostic.code} {diagnostic.line ? `line ${diagnostic.line}${diagnostic.column ? `:${diagnostic.column}` : ''}` : ''}: {diagnostic.message}
                    </strong>
                    <span className={scriptSurfaceClass.diagnosticSuggestion}>Fix: {diagnostic.suggestion}</span>
                  </div>)}
                </div>}
              </article>
            </div>}

            {workspaceView === 'build' && <div className={surfaceClass.root}>
              <header className={surfaceClass.buildHeader}>
                <div>
                  <span className={surfaceClass.buildKicker}>Build & Package</span>
                  <strong className={surfaceClass.buildHeaderTitle}>{shellState.project_name || 'Untitled project'}</strong>
                </div>
                <button className={surfaceClass.primaryButton} onClick={requestBuildPackage} disabled={buildBusy}>
                  {buildBusy ? <IconLoader className="animate-spin" /> : <IconPackage />}
                  {buildBusy ? 'Preparing' : 'Package'}
                </button>
              </header>

              <div className={buildClass.layout}>
                <section className={buildClass.main}>
                  <div className={buildClass.presets} aria-label="Build presets">
                    {BUILD_PRESETS.map(preset => (
                      <button
                        key={preset.id}
                        className={cx(
                          buildClass.presetButton,
                          buildTarget === preset.target && buildFormat === preset.format && buildChannel === preset.channel && buildClass.selectedButton,
                        )}
                        onClick={() => applyBuildPreset(preset)}
                      >
                        <IconPackage />
                        <span className={buildClass.itemTitle}>{preset.label}</span>
                        <small className={buildClass.itemMeta}>{preset.channel} · {preset.format}</small>
                      </button>
                    ))}
                  </div>

                  <div className={buildClass.card}>
                    <div className={buildClass.sectionTitle}>
                      <span className={surfaceClass.buildKicker}>Target</span>
                      <strong className={buildStatusClass(selectedBuildTarget.status)}>{selectedBuildTarget.status}</strong>
                    </div>
                    <div className={buildClass.targetGrid}>
                      {BUILD_TARGETS.map(target => (
                        <button
                          key={target.id}
                          className={cx(buildClass.targetButton, buildTarget === target.id && buildClass.selectedButton)}
                          onClick={() => {
                            setBuildTarget(target.id);
                            setBuildFormat(target.formats[0]);
                          }}
                        >
                          <span className={buildClass.itemTitle}>{target.label}</span>
                          <small className={buildClass.itemMeta}>{target.formats.join(', ')}</small>
                          <b className={buildStatusClass(target.status)}>{target.status}</b>
                        </button>
                      ))}
                    </div>
                  </div>

                  <div className={buildClass.card}>
                    <div className={buildClass.sectionTitle}>
                      <span className={surfaceClass.buildKicker}>Package</span>
                      <strong className={buildClass.sectionValue}>{selectedBuildTarget.label}</strong>
                    </div>
                    <div className={buildClass.formGrid}>
                      <label className={buildClass.formLabel}>
                        <span className={buildClass.formLabelText}>Format</span>
                        <select className={buildClass.select} value={buildFormat} onChange={event => setBuildFormat(event.currentTarget.value as BuildFormat)}>
                          {selectedBuildTarget.formats.map(format => (
                            <option key={format} value={format}>{format}</option>
                          ))}
                        </select>
                      </label>
                      <label className={buildClass.formLabel}>
                        <span className={buildClass.formLabelText}>Channel</span>
                        <select className={buildClass.select} value={buildChannel} onChange={event => setBuildChannel(event.currentTarget.value as 'debug' | 'release')}>
                          <option value="release">release</option>
                          <option value="debug">debug</option>
                        </select>
                      </label>
                      <label className={cx(buildClass.formLabel, buildClass.checkbox)}>
                        <input
                          className={buildClass.checkboxInput}
                          type="checkbox"
                          checked={buildOptimizeAssets}
                          onChange={event => setBuildOptimizeAssets(event.currentTarget.checked)}
                        />
                        <span className={buildClass.formLabelText}>Optimize assets</span>
                      </label>
                      <label className={cx(buildClass.formLabel, buildClass.checkbox)}>
                        <input
                          className={buildClass.checkboxInput}
                          type="checkbox"
                          checked={buildIncludeDebugSymbols}
                          onChange={event => setBuildIncludeDebugSymbols(event.currentTarget.checked)}
                        />
                        <span className={buildClass.formLabelText}>Debug symbols</span>
                      </label>
                    </div>
                  </div>

                  <div className={buildClass.output}>
                    <div>
                      <span className={surfaceClass.buildKicker}>Output</span>
                      <strong className={buildClass.outputPath}>exports/{shellState.project_name || 'project'}/{buildTarget}/{buildChannel}</strong>
                    </div>
                    <p className={buildClass.outputNote}>{selectedBuildTarget.note}</p>
                    {buildMessage && <pre className={buildClass.outputPre}>{buildMessage}</pre>}
                  </div>
                </section>

                <aside className={buildClass.sidebar}>
                  <section className={buildClass.sidebarSection}>
                    <span className={surfaceClass.buildKicker}>Pipeline</span>
                    <ol className={buildClass.sidebarList}>
                      <li className={cx(buildClass.sidebarItem, 'text-[#4ade80]')}><IconCheck /> Validate project</li>
                      <li className={cx(buildClass.sidebarItem, 'text-[var(--text-secondary)]')}><IconPackage /> Export runtime</li>
                      <li className={buildClass.sidebarItem}><IconPackage /> Bundle assets</li>
                      <li className={buildClass.sidebarItem}><IconPackage /> Create installer</li>
                      <li className={buildClass.sidebarItem}><IconCheck /> Sign & verify</li>
                    </ol>
                  </section>
                  <section className={buildClass.sidebarSection}>
                    <span className={surfaceClass.buildKicker}>Current request</span>
                    <dl className={buildClass.sidebarDl}>
                      <div className={buildClass.sidebarDlRow}><dt className={buildClass.sidebarDt}>Project</dt><dd className={buildClass.sidebarDd}>{shellState.project_name || 'project'}</dd></div>
                      <div className={buildClass.sidebarDlRow}><dt className={buildClass.sidebarDt}>Target</dt><dd className={buildClass.sidebarDd}>{selectedBuildTarget.label}</dd></div>
                      <div className={buildClass.sidebarDlRow}><dt className={buildClass.sidebarDt}>Format</dt><dd className={buildClass.sidebarDd}>{buildFormat}</dd></div>
                      <div className={buildClass.sidebarDlRow}><dt className={buildClass.sidebarDt}>Assets</dt><dd className={buildClass.sidebarDd}>{buildOptimizeAssets ? 'optimized' : 'raw'}</dd></div>
                      <div className={buildClass.sidebarDlRow}><dt className={buildClass.sidebarDt}>Symbols</dt><dd className={buildClass.sidebarDd}>{buildIncludeDebugSymbols ? 'included' : 'stripped'}</dd></div>
                    </dl>
                  </section>
                </aside>
              </div>
            </div>}

            {workspaceView === 'diagnostics' && <div className={surfaceClass.root}>
              <header className={surfaceClass.header}>
                <div>
                  <span className={surfaceClass.headerKicker}>Console</span>
                  <strong className={surfaceClass.headerTitle}>{consoleEntries.length} diagnostics</strong>
                </div>
                <div className={diagnosticsClass.headerActions}>
                  <button className={surfaceClass.button} onClick={() => refreshConsoleEntries()} disabled={consoleBusy}>{consoleBusy ? 'Refreshing' : 'Refresh'}</button>
                  <button className={surfaceClass.button} onClick={clearConsoleEntries} disabled={consoleBusy || consoleEntries.length === 0}>Clear</button>
                </div>
              </header>
              <div className={surfaceClass.list}>
                {consoleEntries.length === 0 ? (
                  <div className={surfaceClass.empty}>No diagnostics or tool output yet.</div>
                ) : consoleEntries.map((entry, index) => (
                  <article className={diagnosticsClass.entry} key={`${entry.timestamp}-${index}`}>
                    <div className={diagnosticsClass.meta}>
                      <span className={cx(diagnosticsClass.level, diagnosticLevelClass(entry.level))}>{entry.level}</span>
                      <strong className={diagnosticsClass.subsystem}>{entry.subsystem || 'editor'}</strong>
                    </div>
                    <p className={diagnosticsClass.message}>{entry.message}</p>
                    {(entry.file || entry.line) && (
                      <small className={diagnosticsClass.source}>{entry.file ?? 'source'}{entry.line ? `:${entry.line}` : ''}</small>
                    )}
                  </article>
                ))}
              </div>
            </div>}
          </section>

          {artifactSelection && <div className={artifactPopoverClass.root} style={{ left: artifactSelection.x, top: artifactSelection.y }}>
            {!artifactQuestionOpen ? <button className={artifactPopoverClass.button} onClick={() => setArtifactQuestionOpen(true)}><IconSparkles /> {t('artifact_ask_about').replace('{kind}', artifactSelection.kind)}</button> : <div className={artifactPopoverClass.panel}><header className={artifactPopoverClass.header}><span className={artifactPopoverClass.label}>{artifactSelection.label}</span><button className={artifactPopoverClass.closeButton} onClick={() => setArtifactSelection(null)}><IconX /></button></header><div className={artifactPopoverClass.form}><input className={artifactPopoverClass.input} autoFocus value={artifactQuestion} onChange={event => setArtifactQuestion(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') submitArtifactQuestion(); if (event.key === 'Escape') setArtifactQuestionOpen(false); }} placeholder={t('artifact_ask_placeholder')} /><button className={artifactPopoverClass.submit} onClick={submitArtifactQuestion} disabled={!artifactQuestion.trim()}>{t('btn_ask')}</button></div></div>}
          </div>}
        </main>

        {aiPanelOpen ? (
          <>
            <div
              className={workspaceClass.resizeHandle}
              onMouseDown={handleResizeDown}
              role="separator"
              aria-label="Resize AI assistant"
              aria-orientation="vertical"
              aria-valuemin={320}
              aria-valuemax={560}
              aria-valuenow={aiPanelWidth}
              tabIndex={0}
              onKeyDown={event => {
                if (event.key === 'ArrowLeft') setAiPanelWidth(width => Math.min(560, width + 16));
                if (event.key === 'ArrowRight') setAiPanelWidth(width => Math.max(320, width - 16));
              }}
            />

            <aside className={workspaceClass.aiPanel} style={{ width: aiPanelWidth }}>
              <div className="flex min-h-8 items-center justify-between border-b border-[var(--border)] px-2 text-[10px] text-[var(--text-muted)]">
                <span>Assistant</span>
                <button className={toolButtonClass({ size: 'icon' })} onClick={() => setAiPanelOpen(false)} title="Collapse assistant">
                  <IconChevronRight />
                </button>
              </div>
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
          </>
        ) : (
          <aside className={workspaceClass.aiRail} aria-label="AI assistant">
            <button className={workspaceClass.aiRailButton} onClick={() => setAiPanelOpen(true)} title="Open assistant">
              <IconBot />
            </button>
            {pendingAiDecisionCount > 0 && <span className={workspaceClass.aiRailBadge}>{pendingAiDecisionCount}</span>}
            {aiWorkspace?.status === 'thinking' || aiWorkspace?.status === 'executing' ? (
              <IconLoader className="mt-1 size-4 animate-spin text-[var(--text-muted)]" />
            ) : null}
          </aside>
        )}
      </div>

      {/* Status Bar */}
      <footer className={shellClass.statusbar}>
        <div className={shellClass.statusGroup}>
          <span className={shellClass.statusItem}>{shellState.project_name || t('status_no_project')}</span>
          <span className={shellClass.statusDivider} />
          <span className={shellClass.statusItem}>{sceneTree.length} {t('label_objects')}</span>
          {selectedEntityName && <><span className={shellClass.statusDivider} /><span className={cx(shellClass.statusItem, shellClass.statusSelection)}>{t('status_selected')} {selectedEntityName}</span></>}
        </div>
        <div className={shellClass.statusGroup}>
          {shellState.scene_dirty ? (
            <span className={cx(shellClass.statusItem, shellClass.statusDirty)}><span className={shellClass.statusDot} />{t('status_unsaved')}</span>
          ) : (
            <span className={cx(shellClass.statusItem, shellClass.statusSaved)}>{t('status_saved')}</span>
          )}
          <span className={shellClass.statusDivider} />
          <span className={cx(shellClass.statusItem, shellClass.version)}>v0.1.0</span>
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
