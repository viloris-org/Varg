import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  addQuestNote,
  branchQuest,
  cancelQuest,
  continueQuest,
  createOpenAIRealtimeTranscriptionSession,
  createQuest,
  deleteQuest,
  discardQuest,
  discardQuestTransactionGroups,
  approveKnowledge,
  applyQuestTransactionGroups,
  applyQuest,
  executeQuest,
  exportQuest,
  getQuest,
  listKnowledge,
  listQuests,
  renameQuest,
  revalidateKnowledge,
  removeKnowledge,
  rejectKnowledge,
  rejectQuest,
  reopenQuest,
  rollbackQuest,
  rewriteQuestPrompt,
  requestQuestRevision,
  requestQuestQuickFix,
  transitionQuest,
  updateQuestIntent,
  updateQuestExecutionConfig,
  updateQuestKnowledgeContext,
  updateQuestSpec,
  type KnowledgeEntry,
  type QuestDetail,
  type QuestAiStreamHandle,
  type QuestMode,
  type QuestModelConfig,
  type QuestRecord,
  type QuestReviewAction,
  type QuestStatus,
} from '../quest';
import { rpc } from '../api';
import { useTranslation } from '../i18n';
import {
  contextMenuClass,
  contextMenuDangerItemClass,
  contextMenuItemClass,
  contextMenuSeparatorClass,
} from '../uiClasses';
import {
  IconAlertCircle,
  IconCheck,
  IconChevronDown,
  IconChevronRight,
  IconCode,
  IconEdit,
  IconFile,
  IconLoader,
  IconMic,
  IconPlay,
  IconPlus,
  IconRefresh,
  IconSend,
  IconSparkles,
  IconTrash,
  IconWand,
  IconX,
} from '../icons';
import type { QuestEditorArtifact } from '../App';

interface Props {
  currentProjectPath: string | null;
  initialQuestId?: string | null;
  onOpenEditor: (projectPath: string, artifact?: QuestEditorArtifact) => Promise<void>;
  onCloseProject: () => void;
}

type QuestPanel = 'overview' | 'intent' | 'spec' | 'review' | 'knowledge' | 'artifact';
type QuestInputMode = 'steer' | 'clarify' | 'manual_intervention' | 'pause';
type VoiceInputStatus = 'idle' | 'connecting' | 'recording';
type QuestMenuAction = 'open' | 'rename' | 'open_editor' | 'branch' | 'export' | 'cancel' | 'archive' | 'reopen' | 'delete';

const defaultQuestModelConfig: QuestModelConfig = {
  provider: 'inherit',
  model: '',
  api_endpoint: null,
  max_tokens: 4096,
  thinking_effort: 'medium',
};

interface QuestModelOption {
  id: string;
  display_name: string;
  provider: string;
  default_max_tokens?: number;
}

interface QuestSelectOption {
  value: string;
  label: string;
  description?: string;
}

interface RealtimeTranscriptionHandle {
  peer: RTCPeerConnection;
  stream: MediaStream;
  dataChannel: RTCDataChannel;
  audioContext: AudioContext;
  analyser: AnalyserNode;
  animationFrame: number;
}

const executionLockedStatuses: QuestStatus[] = [
  'prepared',
  'running',
  'validating',
  'repairing',
  'ready_for_review',
  'applying',
  'completed',
];

interface QuestArtifactSelection {
  kind: QuestEditorArtifact['kind'];
  label: string;
  path?: string;
}

const ACTIVE_STATUSES: QuestStatus[] = [
  'draft',
  'clarifying',
  'specified',
  'planning',
  'prepared',
  'running',
  'waiting_for_user',
  'validating',
  'repairing',
  'ready_for_review',
  'applying',
  'blocked',
];

function cn(...classes: Array<string | false | null | undefined>): string {
  return classes.filter(Boolean).join(' ');
}

const buttonBase = 'inline-flex h-[30px] cursor-pointer items-center gap-[6px] whitespace-nowrap rounded-[5px] border border-[var(--border-light)] bg-[var(--bg-surface)] px-[10px] text-[11px] font-semibold text-[var(--text-primary)] hover:border-[var(--accent)] hover:bg-[var(--bg-hover)] disabled:cursor-default disabled:opacity-40';
const sectionHeadingButton = buttonBase;
const primaryButton = 'border-[var(--accent-hover)] bg-[var(--accent-hover)] text-[var(--bg-base)] hover:border-[var(--text-primary)] hover:bg-[var(--text-primary)]';
const mutedText = 'm-0 text-[12px] text-[var(--text-muted)]';
const panelSection = '[&_section]:mb-5 [&_section]:border-b [&_section]:border-[var(--border)] [&_section]:pb-4 [&_h2]:mb-[10px] [&_h2]:mt-0 [&_h2]:flex [&_h2]:items-center [&_h2]:justify-between [&_h2]:gap-[10px] [&_h2]:text-[12px] [&_h2]:font-medium [&_h2]:text-[var(--text-secondary)] [&_h2_b]:text-[11px] [&_h2_b]:font-medium [&_h2_b]:text-[var(--text-muted)]';
const artifactRowClass = 'box-border grid w-full cursor-pointer grid-cols-[18px_minmax(0,1fr)] items-center gap-[9px] rounded-[5px] border-0 bg-transparent py-2 text-left text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] disabled:cursor-default disabled:opacity-50 [&_small]:mt-[3px] [&_small]:block [&_small]:overflow-hidden [&_small]:text-ellipsis [&_small]:whitespace-nowrap [&_small]:text-[11px] [&_small]:text-[var(--text-muted)] [&_span]:min-w-0 [&_strong]:block [&_strong]:overflow-hidden [&_strong]:text-ellipsis [&_strong]:whitespace-nowrap [&_strong]:text-[12px] [&_strong]:font-medium [&_strong]:text-[var(--text-primary)]';
const fileRowClass = 'box-border grid min-h-[34px] w-full grid-cols-[18px_minmax(0,1fr)_auto] items-center gap-[9px] rounded-[5px] border-0 bg-transparent py-[7px] text-left text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] [&_small]:mt-[3px] [&_small]:block [&_small]:text-[10px] [&_small]:text-[var(--text-muted)] [&_span]:min-w-0 [&_strong]:block [&_strong]:overflow-hidden [&_strong]:text-ellipsis [&_strong]:whitespace-nowrap [&_strong]:text-[12px] [&_strong]:font-medium [&_strong]:text-[var(--text-secondary)] [&>b]:text-[11px] [&>b]:font-mono [&>b]:text-[var(--success)] [&>b_i]:not-italic [&>b_i]:text-[var(--danger)]';
const validationRowClass = cn(fileRowClass, 'cursor-pointer font-[inherit] [&>b]:text-[10px] [&>b]:uppercase [&_svg]:text-[#4ade80]');
const selectableFileRowClass = cn(fileRowClass, 'cursor-pointer grid-cols-[18px_16px_minmax(0,1fr)_auto] [&_input]:m-0 [&_input]:h-[13px] [&_input]:w-[13px]');
const transactionRowClass = cn(selectableFileRowClass, 'min-h-[46px] py-[9px] [&_small]:whitespace-normal [&_strong]:text-[var(--text-primary)]');
const documentPanelClass = 'flex min-h-0 flex-col p-[14px] [&_textarea]:box-border [&_textarea]:min-h-0 [&_textarea]:w-full [&_textarea]:flex-1 [&_textarea]:resize-none [&_textarea]:rounded-lg [&_textarea]:border [&_textarea]:border-[var(--border-light)] [&_textarea]:bg-[var(--bg-base)] [&_textarea]:px-[26px] [&_textarea]:py-[22px] [&_textarea]:font-mono [&_textarea]:text-[11px] [&_textarea]:leading-[1.75] [&_textarea]:text-[var(--text-primary)] [&_textarea]:outline-none [&_textarea:disabled]:text-[var(--text-muted)] [&_textarea:disabled]:opacity-75 [&_textarea:focus]:border-[var(--accent)]';
const issueClass = 'flex items-start gap-2 rounded-md border border-[#713f12] bg-[rgba(120,53,15,0.12)] px-3 py-[11px] text-[9px] leading-[1.5] text-[#fcd34d] [&>button:last-child]:ml-auto [&>button:last-child]:cursor-pointer [&>button:last-child]:rounded [&>button:last-child]:border [&>button:last-child]:border-[#854d0e] [&>button:last-child]:bg-transparent [&>button:last-child]:px-2 [&>button:last-child]:py-[5px] [&>button:last-child]:text-[10px] [&>button:last-child]:font-bold [&>button:last-child]:text-[#fde68a] [&_svg]:shrink-0';
const clearIssueClass = 'border-[#14532d] bg-[rgba(20,83,45,0.14)] text-[#86efac]';
const issueOpenClass = 'flex min-w-0 flex-1 cursor-pointer items-start gap-2 border-0 bg-transparent p-0 text-left font-[inherit] text-inherit hover:underline hover:underline-offset-2';
const reviewActionButtonClass = 'h-[30px] cursor-pointer rounded-[5px] border border-[var(--border-light)] bg-[var(--bg-hover)] px-[10px] text-[9px] font-bold text-[var(--text-primary)] disabled:cursor-default disabled:opacity-40';
const decisionButtonClass = 'inline-flex h-8 cursor-pointer items-center gap-[6px] rounded-[5px] border border-[var(--border-light)] bg-[var(--bg-surface)] px-[11px] text-[9px] font-bold text-[var(--text-secondary)] disabled:cursor-default disabled:opacity-40';
const executionSelectClass = 'h-8 min-w-0 rounded-[5px] border border-[var(--border-light)] bg-[var(--bg-surface)] px-2 text-[11px] text-[var(--text-primary)] outline-none disabled:cursor-default disabled:opacity-45';
const executionToggleClass = 'inline-flex h-8 items-center gap-[7px] rounded-[5px] border border-[var(--border-light)] bg-[var(--bg-surface)] px-[10px] text-[11px] font-medium text-[var(--text-secondary)] [&_input]:m-0 [&_input]:h-[13px] [&_input]:w-[13px]';
const modeSwitchClass = 'inline-grid h-8 grid-cols-2 rounded-[5px] border border-[var(--border-light)] bg-[var(--bg-surface)] p-0.5';
const modeSwitchButtonClass = (active: boolean) => cn(
  'cursor-pointer rounded-[4px] border-0 bg-transparent px-2 text-[11px] font-semibold text-[var(--text-muted)] disabled:cursor-default disabled:opacity-45',
  active && 'bg-[var(--bg-active)] text-[var(--text-primary)] shadow-[var(--shadow-sm)]',
);

const questClasses = {
  shell: 'grid h-screen w-screen grid-rows-[48px_minmax(0,1fr)_24px] overflow-hidden bg-[var(--bg-base)] font-[Inter,var(--font-sans)] text-[var(--text-primary)]',
  globalHeader: 'grid grid-cols-[220px_minmax(0,1fr)_auto] items-center border-b border-[var(--border)] bg-[var(--bg-overlay)] text-[var(--text-primary)] max-[900px]:grid-cols-[150px_minmax(0,1fr)_auto] [&_nav]:flex [&_nav]:h-full [&_nav]:items-stretch [&_nav]:gap-0.5',
  brand: 'flex items-baseline gap-2 px-4 [&_span]:text-[13px] [&_span]:font-extrabold [&_span]:text-[var(--text-primary)] [&_strong]:text-[11px] [&_strong]:text-[var(--text-secondary)]',
  topNavButton: 'cursor-pointer border-0 border-b-2 border-transparent bg-transparent px-[14px] text-[11px] font-semibold text-[var(--text-secondary)] disabled:cursor-default disabled:opacity-40 max-[900px]:px-[7px]',
  topNavButtonActive: 'border-b-[var(--text-primary)] text-[var(--text-primary)]',
  globalActions: 'flex gap-[6px] pr-[10px]',
  layout: 'grid min-h-0 grid-cols-[280px_minmax(0,1fr)] bg-[var(--bg-base)] max-[900px]:grid-cols-[220px_minmax(0,1fr)]',
  sidebar: 'grid grid-rows-[auto_minmax(0,auto)_minmax(0,auto)_1fr] overflow-y-auto border-r border-[var(--border)] bg-[var(--bg-surface)] text-[var(--text-primary)]',
  sidebarHeading: 'flex min-h-[88px] items-center justify-between px-3 py-[14px]',
  newButton: 'grid h-[38px] w-full cursor-pointer grid-cols-[18px_minmax(0,1fr)_auto] items-center gap-2 rounded-lg border border-[var(--border-light)] bg-[var(--bg-elevated)] px-[11px] text-left text-[13px] text-[var(--text-primary)] shadow-[var(--shadow-sm)] disabled:cursor-default disabled:opacity-40 [&_kbd]:font-mono [&_kbd]:text-[10px] [&_kbd]:text-[var(--text-muted)] [&_svg]:text-[var(--text-primary)]',
  sidebarFooter: 'self-end grid gap-1 p-3 [&_button]:h-8 [&_button]:rounded-[7px] [&_button]:border-0 [&_button]:bg-transparent [&_button]:text-left [&_button]:text-[12px] [&_button]:text-[var(--text-secondary)] hover:[&_button]:bg-[var(--bg-hover)]',
  home: 'flex min-h-0 flex-col items-center justify-center bg-[var(--bg-base)] px-8 pb-[90px] pt-10 text-[var(--text-primary)]',
  orb: 'mb-[26px] grid h-[60px] w-[60px] place-items-center rounded-full border border-[var(--border)] bg-[var(--bg-surface)] text-[var(--text-muted)] shadow-[var(--shadow-md)]',
  startLine: 'mb-7 flex flex-wrap items-center justify-center gap-[9px] text-[12px] text-[var(--text-muted)] [&_b]:font-medium [&_b]:text-[var(--text-primary)] [&_span]:font-medium',
  promptBox: 'w-[min(800px,calc(100vw-360px))] min-w-[min(800px,calc(100vw-360px))] rounded-lg border border-[var(--border-light)] bg-[var(--bg-surface)] shadow-[var(--shadow-lg)] max-[900px]:w-[min(680px,calc(100vw-280px))] max-[900px]:min-w-0 [&_footer]:flex [&_footer]:min-h-[42px] [&_footer]:items-center [&_footer]:justify-between [&_footer]:gap-2 [&_footer]:pb-2 [&_footer]:pl-[13px] [&_footer]:pr-[9px] [&_textarea]:box-border [&_textarea]:h-[94px] [&_textarea]:w-full [&_textarea]:resize-none [&_textarea]:border-0 [&_textarea]:bg-transparent [&_textarea]:px-[14px] [&_textarea]:pb-2 [&_textarea]:pt-[14px] [&_textarea]:font-[Inter,var(--font-sans)] [&_textarea]:text-[14px] [&_textarea]:leading-[1.5] [&_textarea]:text-[var(--text-primary)] [&_textarea]:outline-none [&_textarea::placeholder]:text-[var(--text-muted)]',
  promptIconButton: 'grid h-[30px] w-[30px] cursor-pointer place-items-center rounded-[7px] border-0 bg-transparent text-[var(--text-muted)] hover:enabled:bg-[var(--bg-hover)] hover:enabled:text-[var(--text-primary)] disabled:cursor-default disabled:opacity-35',
  promptSubmit: 'grid h-[30px] w-[30px] cursor-pointer place-items-center rounded-[7px] border border-[var(--brand)] bg-[var(--brand)] text-[var(--bg-base)] shadow-[0_0_0_1px_var(--brand-dim),0_8px_18px_rgba(34,197,94,0.24)] hover:enabled:border-[var(--brand-hover)] hover:enabled:bg-[var(--brand-hover)] disabled:cursor-default disabled:opacity-45 [&_svg]:stroke-[2.25]',
  introCard: 'mt-16 grid w-[min(640px,calc(100vw-440px))] grid-cols-[96px_minmax(0,1fr)] gap-[18px] rounded-lg border border-dashed border-[var(--border-light)] bg-[var(--bg-surface)] p-3 text-[var(--text-secondary)] max-[900px]:w-[min(680px,calc(100vw-280px))] max-[900px]:grid-cols-1 max-[900px]:min-w-0 [&>svg]:h-[70px] [&>svg]:w-24 [&>svg]:rounded-md [&>svg]:bg-[var(--brand-dim)] [&>svg]:p-4 [&>svg]:text-[var(--brand)] [&_p]:m-0 [&_p]:text-[12px] [&_p]:leading-[1.5] [&_p]:text-[var(--text-muted)] [&_strong]:mb-2 [&_strong]:mt-1 [&_strong]:block [&_strong]:text-[14px] [&_strong]:text-[var(--text-primary)]',
  workspace: 'grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden bg-[var(--bg-surface)]',
  header: 'flex min-h-[78px] items-center justify-between gap-[18px] border-b border-[var(--border)] bg-[var(--bg-surface)] px-[18px] py-[10px] max-[900px]:flex-col max-[900px]:items-start [&_h1]:mb-[3px] [&_h1]:mt-1 [&_h1]:text-[15px] [&_h1]:font-semibold [&_h1]:text-[var(--text-primary)] [&_p]:m-0 [&_p]:max-w-[820px] [&_p]:text-[11px] [&_p]:leading-[1.45] [&_p]:text-[var(--text-secondary)]',
  projectLine: 'flex items-center gap-[5px] font-mono text-[10px] text-[var(--text-muted)] [&_svg]:w-[9px]',
  titleEdit: 'my-[7px] flex items-center gap-[6px] [&_button]:inline-flex [&_button]:h-[30px] [&_button]:cursor-pointer [&_button]:items-center [&_button]:gap-[5px] [&_button]:rounded-[5px] [&_button]:border [&_button]:border-[var(--border-light)] [&_button]:bg-[var(--bg-surface)] [&_button]:px-[9px] [&_button]:text-[9px] [&_button]:font-semibold [&_button]:text-[var(--text-secondary)] [&_button:disabled]:cursor-default [&_button:disabled]:opacity-40 [&_input]:h-[31px] [&_input]:w-[min(520px,50vw)] [&_input]:rounded-[5px] [&_input]:border [&_input]:border-[#52525b] [&_input]:bg-[#0d0e12] [&_input]:px-[10px] [&_input]:text-[15px] [&_input]:font-bold [&_input]:text-[#f1f5f9] [&_input]:outline-none',
  headerActions: 'flex flex-wrap items-center justify-end gap-[6px] max-[900px]:justify-start',
  cockpit: 'grid min-h-0 grid-cols-[minmax(420px,1fr)_minmax(380px,44%)] overflow-hidden bg-[var(--bg-surface)] max-[900px]:grid-cols-1 max-[900px]:grid-rows-[minmax(0,1fr)_minmax(360px,44vh)]',
  runStream: 'grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)_52px] border-r border-[var(--border)] bg-[var(--bg-base)] max-[900px]:border-b max-[900px]:border-r-0',
  streamPrompt: 'mx-[22px] mb-2 mt-4 rounded-[7px] border border-[var(--border)] bg-[var(--bg-hover)] px-3 py-[9px] text-[13px] text-[var(--text-primary)]',
  streamList: 'min-h-0 overflow-auto px-[22px] pb-[22px]',
  streamEntry: 'grid grid-cols-[16px_minmax(0,1fr)] gap-[9px] my-[6px] [&>div]:min-w-0 [&>div]:rounded-[5px] [&>div]:border [&>div]:border-[var(--border)] [&>div]:bg-[var(--bg-surface)] [&>div]:px-[10px] [&>div]:py-2 [&_details]:mt-[6px] [&_details]:text-[10px] [&_details]:text-[var(--text-secondary)] [&_header]:flex [&_header]:items-center [&_header]:justify-between [&_header]:gap-[10px] [&_pre]:mt-[7px] [&_pre]:max-h-[260px] [&_pre]:overflow-auto [&_pre]:rounded-[5px] [&_pre]:bg-[var(--bg-base)] [&_pre]:p-[9px] [&_pre]:font-mono [&_pre]:text-[10px] [&_pre]:leading-[1.55] [&_pre]:text-[var(--text-secondary)] [&_small]:mt-1 [&_small]:block [&_small]:text-[11px] [&_small]:text-[var(--text-muted)] [&_strong]:overflow-hidden [&_strong]:text-ellipsis [&_strong]:whitespace-nowrap [&_strong]:text-[12px] [&_strong]:font-medium [&_strong]:text-[var(--text-primary)] [&_summary]:cursor-pointer [&_time]:shrink-0 [&_time]:font-mono [&_time]:text-[10px] [&_time]:text-[var(--text-muted)]',
  nextEntry: '[&>div]:border-[var(--border-light)] [&_time]:font-bold [&_time]:text-[var(--text-primary)]',
  liveEntry: '[&>div]:border-[var(--border-light)] [&>div]:bg-[var(--bg-hover)] [&_time]:font-bold [&_time]:text-[var(--text-primary)]',
  timelineDot: 'relative mt-4 h-[9px] w-[9px] rounded-full border border-[var(--text-muted)] bg-[var(--bg-surface)] after:absolute after:left-1 after:top-[10px] after:h-[calc(100%+34px)] after:w-px after:bg-[var(--border)] after:content-[""]',
  timelineDotLast: 'after:hidden',
  timelineDotNext: 'border-[var(--text-primary)]',
  timelineDotLive: 'border-[var(--brand)] bg-[var(--brand)] shadow-[0_0_0_3px_var(--brand-dim)] [animation:quest-live-pulse_1.7s_ease-out_infinite]',
  reviewChip: 'ml-6 mt-3 inline-flex w-max cursor-pointer items-center gap-[6px] rounded-md border border-[var(--border-light)] bg-[var(--bg-surface)] px-[10px] py-[7px] text-[12px] text-[var(--success)] [&_span]:text-[var(--danger)]',
  steerBar: 'grid grid-cols-[118px_minmax(0,1fr)_34px] items-center gap-3 border-t border-[var(--border)] bg-[var(--bg-surface)] px-3 py-2 text-[11px] text-[var(--text-muted)] [&_input]:min-w-0 [&_input]:border-0 [&_input]:bg-transparent [&_input]:text-[12px] [&_input]:text-[var(--text-secondary)] [&_input]:outline-none [&_input::placeholder]:text-[var(--text-muted)] [&_select]:h-7 [&_select]:min-w-0 [&_select]:rounded-[5px] [&_select]:border [&_select]:border-[var(--border-light)] [&_select]:bg-[var(--bg-surface)] [&_select]:px-[6px] [&_select]:text-[11px] [&_select]:text-[var(--text-primary)] [&_select]:outline-none',
  sendButton: 'grid h-[30px] w-[30px] cursor-pointer place-items-center rounded-[5px] border border-[var(--brand)] bg-[var(--brand)] text-[var(--bg-base)] shadow-[0_0_0_1px_var(--brand-dim)] hover:enabled:border-[var(--brand-hover)] hover:enabled:bg-[var(--brand-hover)] disabled:cursor-default disabled:opacity-45 [&_svg]:stroke-[2.25]',
  rightPanel: 'grid min-h-0 min-w-0 grid-rows-[37px_minmax(0,1fr)] bg-[var(--bg-surface)]',
  panelTabs: 'flex min-w-0 border-b border-[var(--border)] bg-[var(--bg-base)]',
  panelTab: 'inline-flex h-[37px] min-w-[42px] cursor-pointer items-center justify-center gap-[6px] border-0 border-r border-[var(--border)] border-b-2 border-b-transparent bg-transparent px-[12px] text-[12px] text-[var(--text-secondary)] transition-[background-color,border-color,color,box-shadow] duration-150 hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] [&_svg]:shrink-0',
  panelTabActive: 'min-w-[118px] border-b-[var(--brand)] bg-[var(--brand-dim)] text-[var(--brand)] shadow-[inset_0_-1px_0_var(--brand),inset_0_1px_0_rgba(255,255,255,0.04)]',
  overview: cn('min-h-0 overflow-auto p-4', panelSection),
  sectionHeading: 'flex flex-wrap items-center justify-between gap-3 py-[15px] [&>div:first-child]:min-w-[160px] [&>div:first-child]:flex [&>div:first-child]:flex-1 [&>div:first-child]:flex-col [&>div:first-child]:gap-1 [&>div:last-child]:flex [&>div:last-child]:shrink-0 [&>div:last-child]:flex-wrap [&>div:last-child]:justify-end [&>div:last-child]:gap-[7px] [&_span]:text-[10px] [&_span]:font-extrabold [&_span]:tracking-[0.12em] [&_span]:text-[#64748b] [&_strong]:text-[11px] [&_strong]:text-[var(--text-secondary)]',
  artifactViewer: 'min-h-0 overflow-auto p-4 [&>div_span]:text-[11px] [&>div_span]:capitalize [&>div_span]:text-[var(--text-muted)] [&_h2]:my-[6px] [&_h2]:text-[16px] [&_h2]:text-[var(--text-primary)] [&_header]:mb-[18px] [&_header]:flex [&_header]:justify-between [&_header]:gap-2 [&_p]:m-0 [&_p]:font-mono [&_p]:text-[11px] [&_p]:text-[var(--text-secondary)] [&_pre]:mt-[7px] [&_pre]:max-h-[260px] [&_pre]:overflow-auto [&_pre]:rounded-[5px] [&_pre]:bg-[var(--bg-base)] [&_pre]:p-[9px] [&_pre]:font-mono [&_pre]:text-[10px] [&_pre]:leading-[1.55] [&_pre]:text-[var(--text-secondary)]',
  knowledge: cn('grid gap-[14px] overflow-auto p-[14px]', panelSection),
  review: cn('min-h-0 overflow-auto p-4', panelSection),
  reviewEmpty: 'flex h-full min-h-[360px] flex-col items-center justify-center gap-2 text-[var(--text-muted)]',
  reviewSummary: 'mb-[18px] grid grid-cols-[72px_minmax(0,1fr)] gap-4 rounded-lg border border-[var(--border-light)] bg-[var(--bg-base)] p-3 [&_div]:flex [&_div]:flex-col [&_div]:gap-[5px] [&_p]:m-0 [&_p]:text-[12px] [&_p]:leading-[1.6] [&_p]:text-[var(--text-secondary)] [&_span]:text-[10px] [&_span]:font-extrabold [&_span]:tracking-[0.12em] [&_span]:text-[var(--text-muted)] [&_strong]:text-[12px] [&_strong]:uppercase [&_strong]:text-[var(--success)]',
  reviewMetrics: 'grid grid-cols-[repeat(auto-fit,minmax(118px,1fr))] gap-2 [&_div]:min-h-[58px] [&_div]:rounded-md [&_div]:border [&_div]:border-[var(--border)] [&_div]:bg-[#131419] [&_div]:p-[10px] [&_span]:block [&_span]:overflow-hidden [&_span]:text-ellipsis [&_span]:whitespace-nowrap [&_span]:text-[10px] [&_span]:uppercase [&_span]:text-[var(--text-muted)] [&_strong]:mt-[7px] [&_strong]:block [&_strong]:font-mono [&_strong]:text-[13px] [&_strong]:text-[#e5e7eb]',
  metricNote: 'mb-0 mt-2 text-[9px] leading-[1.5] text-[var(--text-muted)]',
  reviewActions: 'flex flex-wrap gap-2',
  decisionRow: 'flex flex-wrap gap-2',
  decisionHistory: 'mt-[10px] grid gap-[6px] [&>div]:flex [&>div]:items-center [&>div]:gap-2 [&_button]:h-7 [&_button]:cursor-pointer [&_button]:rounded-[5px] [&_button]:border [&_button]:border-[var(--danger)] [&_button]:bg-[var(--danger-dim)] [&_button]:px-[9px] [&_button]:text-[10px] [&_button]:text-[var(--danger)] [&_small]:flex-1 [&_small]:rounded-[5px] [&_small]:border [&_small]:border-[var(--border-light)] [&_small]:bg-[var(--bg-surface)] [&_small]:px-2 [&_small]:py-[7px] [&_small]:text-[10px] [&_small]:text-[var(--text-muted)]',
  footer: 'flex items-center justify-between border-t border-[var(--border)] bg-[var(--bg-base)] px-[10px] font-mono text-[10px] text-[var(--text-muted)]',
  errorToast: 'fixed right-4 top-[58px] z-[80] grid w-[min(360px,calc(100vw-32px))] cursor-pointer grid-cols-[18px_minmax(0,1fr)_14px] items-center gap-[10px] rounded-lg border border-[#fecaca] bg-[#fff7f7] px-3 py-[11px] text-left text-[#991b1b] shadow-[0_16px_42px_rgba(15,23,42,0.16)] [&_small]:block [&_small]:overflow-hidden [&_small]:text-ellipsis [&_small]:whitespace-nowrap [&_small]:text-[10px] [&_small]:text-[#7f1d1d] [&_span]:min-w-0 [&_strong]:mb-0.5 [&_strong]:block [&_strong]:text-[12px]',
  errorModal: 'fixed inset-0 z-[90] grid place-items-center bg-[rgba(15,23,42,0.42)] p-6 [&>div]:grid [&>div]:max-h-[min(520px,calc(100vh-48px))] [&>div]:w-[min(680px,100%)] [&>div]:grid-rows-[auto_minmax(0,1fr)_auto] [&>div]:rounded-lg [&>div]:border [&>div]:border-[var(--border-light)] [&>div]:bg-[var(--bg-surface)] [&>div]:text-[var(--text-primary)] [&>div]:shadow-[var(--shadow-lg)] [&_footer]:flex [&_footer]:items-center [&_footer]:justify-end [&_footer]:gap-[10px] [&_footer]:border-t [&_footer]:border-[var(--border)] [&_footer]:px-[14px] [&_footer]:py-3 [&_header]:flex [&_header]:items-center [&_header]:justify-between [&_header]:gap-[10px] [&_header]:border-b [&_header]:border-[var(--border)] [&_header]:px-[14px] [&_header]:py-3 [&_header_span]:flex [&_header_span]:items-center [&_header_span]:gap-2 [&_header_span]:text-[13px] [&_header_span]:font-bold [&_header_span]:text-[var(--danger)] [&_pre]:m-0 [&_pre]:overflow-auto [&_pre]:whitespace-pre-wrap [&_pre]:break-words [&_pre]:bg-[var(--bg-base)] [&_pre]:p-[14px] [&_pre]:font-mono [&_pre]:text-[11px] [&_pre]:leading-[1.6] [&_pre]:text-[var(--text-secondary)]',
  modalButton: 'inline-flex h-[30px] cursor-pointer items-center gap-[6px] rounded-[5px] border border-[var(--border-light)] bg-[var(--bg-surface)] px-[10px] text-[11px] font-semibold text-[var(--text-primary)]',
  group: 'select-none px-3 pb-[10px] pt-[6px] [&>header]:flex [&>header]:items-center [&>header]:justify-between [&>header]:px-1 [&>header]:pb-2 [&>header]:pt-[7px] [&>header]:text-[12px] [&>header]:font-medium [&>header]:text-[var(--text-secondary)] [&>header_b]:text-[10px] [&>header_b]:text-[var(--text-muted)] [&>p]:px-[22px] [&>p]:py-2 [&>p]:text-[11px] [&>p]:text-[var(--text-muted)]',
  groupButton: 'mb-0.5 grid w-full select-none cursor-pointer grid-cols-[8px_minmax(0,1fr)] items-start gap-[9px] rounded-[7px] border border-transparent bg-transparent px-[7px] py-2 text-left text-[var(--text-secondary)] hover:border-[var(--border-light)] hover:bg-[var(--bg-hover)] [&_div]:min-w-0 [&_small]:block [&_small]:overflow-hidden [&_small]:select-none [&_small]:text-ellipsis [&_small]:whitespace-nowrap [&_small]:text-[10px] [&_small]:text-[var(--text-muted)] [&_strong]:mb-1 [&_strong]:block [&_strong]:overflow-hidden [&_strong]:select-none [&_strong]:text-ellipsis [&_strong]:whitespace-nowrap [&_strong]:text-[12px] [&_strong]:font-medium [&_strong]:text-[var(--text-primary)]',
  groupButtonActive: 'border-[var(--border-light)] bg-[var(--bg-hover)]',
  knowledgeRow: 'grid gap-2 rounded-md border border-[var(--border-light)] bg-[var(--bg-surface)] p-[10px] [&_footer]:flex [&_footer]:items-center [&_footer]:justify-start [&_footer]:gap-2 [&_footer_button]:inline-flex [&_footer_button]:h-7 [&_footer_button]:cursor-pointer [&_footer_button]:items-center [&_footer_button]:gap-[5px] [&_footer_button]:rounded-[5px] [&_footer_button]:border [&_footer_button]:border-[var(--border-light)] [&_footer_button]:bg-[var(--bg-base)] [&_footer_button]:px-[9px] [&_footer_button]:text-[10px] [&_footer_button]:text-[var(--text-secondary)] [&_header]:flex [&_header]:items-center [&_header]:justify-between [&_header]:gap-2 [&_header_b]:font-mono [&_header_b]:text-[9px] [&_header_b]:text-[var(--text-muted)] [&_header_span]:text-[10px] [&_header_span]:font-bold [&_header_span]:uppercase [&_header_span]:text-[var(--text-secondary)] [&_p]:m-0 [&_p]:text-[12px] [&_p]:leading-[1.45] [&_p]:text-[var(--text-primary)] [&_small]:text-[10px] [&_small]:text-[var(--text-muted)]',
};

const panelTabs: Array<{
  id: Exclude<QuestPanel, 'artifact'>;
  labelKey: string;
  icon: React.ReactNode;
}> = [
  { id: 'overview', labelKey: 'quest_tab_overview', icon: <IconRefresh /> },
  { id: 'intent', labelKey: 'quest_tab_intent', icon: <IconFile /> },
  { id: 'spec', labelKey: 'quest_tab_spec', icon: <IconFile /> },
  { id: 'review', labelKey: 'quest_tab_review', icon: <IconCheck /> },
  { id: 'knowledge', labelKey: 'quest_tab_knowledge', icon: <IconSparkles /> },
];

function statusTextClass(status: QuestStatus | string): string {
  if (status === 'completed' || status === 'approved') return 'text-[#16a34a]';
  if (status === 'running' || status === 'applying' || status === 'validating' || status === 'repairing') return 'text-[#52525b]';
  if (status === 'ready_for_review') return 'text-[#3f3f46]';
  if (status === 'waiting_for_user' || status === 'clarifying' || status === 'pending') return 'text-[#d97706]';
  if (status === 'blocked' || status === 'rejected' || status === 'canceled' || status === 'failed' || status === 'stale' || status === 'missing') return 'text-[#dc2626]';
  if (status === 'archived') return 'text-[#64748b]';
  return 'text-[#64748b]';
}

function statusDotClass(status: QuestStatus | string): string {
  if (status === 'completed') return 'bg-[#22c55e]';
  if (status === 'running' || status === 'applying' || status === 'validating' || status === 'repairing') return 'bg-[#52525b] shadow-[0_0_7px_rgba(82,82,91,0.35)]';
  if (status === 'ready_for_review') return 'bg-[#3f3f46]';
  if (status === 'waiting_for_user' || status === 'clarifying') return 'bg-[#f59e0b]';
  if (status === 'blocked' || status === 'rejected' || status === 'canceled') return 'bg-[#ef4444]';
  if (status === 'archived') return 'bg-[#475569]';
  return 'bg-[#94a3b8]';
}

function progressItemClass(status: 'done' | 'current' | 'pending'): string {
  return cn(
    'grid grid-cols-[18px_minmax(0,1fr)] items-start gap-2 text-[var(--text-muted)] [&>span]:mt-px [&>span]:grid [&>span]:h-[13px] [&>span]:w-[13px] [&>span]:place-items-center [&>span]:rounded-full [&>span]:border [&>span]:border-[var(--text-muted)] [&>span]:bg-[var(--bg-surface)] [&_p]:m-0 [&_p]:text-[12px] [&_p]:leading-[1.35]',
    status === 'done' && '[&>span]:border-[var(--border-light)] [&>span]:bg-[var(--bg-active)] [&>span]:text-[var(--text-primary)] [&_p]:text-[var(--text-muted)]',
    status === 'current' && '[&>span]:border-[var(--text-primary)] [&_p]:text-[var(--text-primary)]',
  );
}

function QuestDropdown({
  value,
  options,
  onChange,
  disabled,
  compact = false,
  widthClass = 'w-full',
  menuWidthClass,
  align = 'left',
}: {
  value: string;
  options: QuestSelectOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  compact?: boolean;
  widthClass?: string;
  menuWidthClass?: string;
  align?: 'left' | 'right';
}) {
  const [open, setOpen] = useState(false);
  const selectedOption = options.find(option => option.value === value) ?? options[0];

  return (
    <div
      className={cn('relative min-w-0', widthClass)}
      onBlur={event => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
          setOpen(false);
        }
      }}
    >
      <button
        type="button"
        className={cn(
          'flex min-w-0 cursor-pointer items-center justify-between gap-2 rounded-[5px] border-0 bg-transparent text-left text-[11px] font-medium text-[var(--text-secondary)] outline-none hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] disabled:cursor-default disabled:opacity-45',
          compact ? 'h-7 px-2' : 'h-8 w-full border border-[var(--border-light)] bg-[var(--bg-surface)] px-2',
        )}
        onClick={() => setOpen(current => !current)}
        disabled={disabled || options.length === 0}
      >
        <span className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap">
          {selectedOption?.label ?? 'No options'}
        </span>
        <IconChevronDown className={cn('shrink-0 text-[var(--text-muted)]', open && 'rotate-180')} size={compact ? 12 : 13} />
      </button>
      {open && (
        <div className={cn(
          'absolute top-[calc(100%+4px)] z-30 flex max-h-[240px] min-w-full flex-col overflow-auto rounded-[7px] border border-[var(--border-light)] bg-[var(--bg-elevated)] p-1 shadow-[var(--shadow-md)]',
          align === 'right' ? 'right-0' : 'left-0',
          menuWidthClass,
        )}>
          {options.map(option => {
            const active = option.value === value;
            return (
              <button
                type="button"
                key={option.value}
                className={cn(
                  'flex min-w-0 cursor-pointer flex-col gap-[2px] rounded-[5px] border-0 bg-transparent px-2 py-[7px] text-left text-[11px] text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]',
                  active && 'bg-[var(--bg-active)] text-[var(--text-primary)]',
                )}
                onMouseDown={event => event.preventDefault()}
                onClick={() => {
                  onChange(option.value);
                  setOpen(false);
                }}
              >
                <span className="block max-w-full overflow-hidden text-ellipsis whitespace-nowrap font-semibold">{option.label}</span>
                {option.description && <small className="overflow-hidden text-ellipsis whitespace-nowrap text-[10px] text-[var(--text-muted)]">{option.description}</small>}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

function PromptRewriteIcon({ active }: { active: boolean }) {
  if (active) {
    return (
      <span
        className="relative block h-[15px] w-[15px] rounded-full border-2 border-[var(--brand-dim)] border-t-[var(--brand)] [animation:quest-loader-spin_820ms_linear_infinite] after:absolute after:left-1/2 after:top-1/2 after:h-[4px] after:w-[4px] after:-translate-x-1/2 after:-translate-y-1/2 after:rounded-full after:bg-[var(--brand)] after:content-[''] after:[animation:quest-loader-pulse_820ms_ease-in-out_infinite]"
        aria-hidden="true"
      />
    );
  }
  return <IconWand size={15} />;
}

function QuestLoader({ size = 14 }: { size?: number }) {
  return (
    <span
      className="relative inline-block shrink-0 rounded-full border-2 border-[rgba(13,14,16,0.28)] border-t-[var(--bg-base)] align-[-2px] [animation:quest-loader-spin_780ms_linear_infinite] after:absolute after:left-1/2 after:top-1/2 after:h-[4px] after:w-[4px] after:-translate-x-1/2 after:-translate-y-1/2 after:rounded-full after:bg-[var(--bg-base)] after:content-[''] after:[animation:quest-loader-pulse_780ms_ease-in-out_infinite]"
      style={{ width: size, height: size }}
      aria-hidden="true"
    />
  );
}

function VoiceInputIcon({ status }: { status: VoiceInputStatus }) {
  if (status === 'connecting') {
    return <QuestLoader size={15} />;
  }
  if (status === 'recording') {
    return (
      <span className="flex h-[15px] w-[15px] items-center justify-center gap-[2px]" aria-hidden="true">
        {[7, 12, 15, 10].map((height, index) => (
          <span
            key={index}
            className="block w-[2px] rounded-full bg-current"
            style={{
              height,
              animation: `quest-voice-wave 720ms ease-in-out ${index * 110}ms infinite alternate`,
            }}
          />
        ))}
      </span>
    );
  }
  return <IconMic size={15} />;
}

function formatTime(timestamp: number): string {
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(timestamp);
}

function formatMetricDuration(value?: number | null): string {
  if (value === null || value === undefined) {
    return 'n/a';
  }
  if (value < 1000) {
    return `${value} ms`;
  }
  return `${(value / 1000).toFixed(1)} s`;
}

function formatMetricScore(value?: number | null): string {
  if (value === null || value === undefined) {
    return 'n/a';
  }
  return `${Math.round(value * 100)}%`;
}

function statusAction(status: QuestStatus): { labelKey: string; next: QuestStatus } | null {
  switch (status) {
    case 'running':
      return { labelKey: 'quest_action_pause', next: 'waiting_for_user' };
    case 'waiting_for_user':
    case 'blocked':
      return { labelKey: 'quest_action_resume', next: 'running' };
    case 'ready_for_review':
      return { labelKey: 'quest_action_accept', next: 'completed' };
    default:
      return null;
  }
}

function defaultPanelForQuest(detail: QuestDetail): QuestPanel {
  return detail.status === 'draft' || detail.status === 'specified' ? 'spec' : 'overview';
}

function progressItems(
  detail: QuestDetail,
  t: (key: string) => string,
): Array<{ title: string; status: 'done' | 'current' | 'pending' }> {
  const taskEvents = detail.events.filter(event => event.kind === 'task_created');
  const titles = taskEvents.length > 0
    ? taskEvents.map(event => event.summary)
    : [t('quest_progress_review_spec'), t('quest_progress_approve_execution'), t('quest_progress_review_evidence')];
  return titles.map((title, index) => {
    if (detail.status === 'completed') return { title, status: 'done' };
    if (detail.status === 'ready_for_review') return { title, status: index < titles.length - 1 ? 'done' : 'current' };
    if (['prepared', 'running', 'validating', 'repairing', 'applying'].includes(detail.status)) {
      return { title, status: index === Math.min(2, titles.length - 1) ? 'current' : index < 2 ? 'done' : 'pending' };
    }
    if (detail.status === 'blocked') return { title, status: index === Math.min(3, titles.length - 1) ? 'current' : index < 3 ? 'done' : 'pending' };
    return { title, status: index === 0 ? 'current' : 'pending' };
  });
}

function hasEventDetails(details: unknown): boolean {
  if (!details || typeof details !== 'object') return false;
  return Object.keys(details as Record<string, unknown>).length > 0;
}

function formatEventDetails(details: unknown): string {
  try {
    return JSON.stringify(details, null, 2);
  } catch {
    return String(details);
  }
}

export default function QuestPage({
  currentProjectPath,
  initialQuestId,
  onOpenEditor,
  onCloseProject,
}: Props) {
  const { t, t_fmt } = useTranslation();
  const voiceInputRef = React.useRef<RealtimeTranscriptionHandle | null>(null);
  const [quests, setQuests] = useState<QuestRecord[]>([]);
  const [knowledge, setKnowledge] = useState<KnowledgeEntry[]>([]);
  const [selected, setSelected] = useState<QuestDetail | null>(null);
  const [panel, setPanel] = useState<QuestPanel>('overview');
  const [artifact, setArtifact] = useState<QuestArtifactSelection | null>(null);
  const [intentDraft, setIntentDraft] = useState('');
  const [specDraft, setSpecDraft] = useState('');
  const [goal, setGoal] = useState('');
  const [questMode, setQuestMode] = useState<QuestMode>('solo');
  const [modelConfig, setModelConfig] = useState<QuestModelConfig>(defaultQuestModelConfig);
  const [workspaceAutoWrite, setWorkspaceAutoWrite] = useState(true);
  const [modelOptions, setModelOptions] = useState<QuestModelOption[]>([]);
  const [voiceInputStatus, setVoiceInputStatus] = useState<VoiceInputStatus>('idle');
  const [canUseOpenAIVoiceInput, setCanUseOpenAIVoiceInput] = useState(false);
  const [rewritingPrompt, setRewritingPrompt] = useState(false);
  const rewriteRequestRef = React.useRef<QuestAiStreamHandle<{ prompt: string }> | null>(null);
  const [renaming, setRenaming] = useState(false);
  const [titleDraft, setTitleDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [executingQuestId, setExecutingQuestId] = useState<string | null>(null);
  const [selectedReviewFiles, setSelectedReviewFiles] = useState<Set<string>>(new Set());
  const [selectedReviewGroups, setSelectedReviewGroups] = useState<Set<string>>(new Set());
  const [questInput, setQuestInput] = useState('');
  const [questInputMode, setQuestInputMode] = useState<QuestInputMode>('steer');
  const [error, setError] = useState<string | null>(null);
  const [errorOpen, setErrorOpen] = useState(false);

  const clearSelectedQuest = useCallback(() => {
    setSelected(null);
    setIntentDraft('');
    setSpecDraft('');
    setSelectedReviewFiles(new Set());
    setSelectedReviewGroups(new Set());
    setTitleDraft('');
    setArtifact(null);
    setRenaming(false);
  }, []);

  const reportError = useCallback((reason: unknown) => {
    setError(String(reason));
    setErrorOpen(false);
  }, []);

  const resetReviewSelection = useCallback((detail: QuestDetail) => {
    setSelectedReviewFiles(new Set(detail.review?.changed_files.map(file => file.path) ?? []));
    setSelectedReviewGroups(new Set(detail.review?.transaction_groups.map(group => group.id) ?? []));
  }, []);

  const stopVoiceInput = useCallback(() => {
    const handle = voiceInputRef.current;
    if (!handle) return;
    window.cancelAnimationFrame(handle.animationFrame);
    handle.dataChannel.close();
    handle.peer.close();
    handle.stream.getTracks().forEach(track => track.stop());
    handle.audioContext.close().catch(() => {});
    voiceInputRef.current = null;
    setVoiceInputStatus('idle');
  }, []);

  useEffect(() => () => stopVoiceInput(), [stopVoiceInput]);

  const syncQuestDrafts = useCallback((detail: QuestDetail) => {
    setSelected(detail);
    setIntentDraft(detail.intent);
    setSpecDraft(detail.spec);
    setQuestMode(detail.mode);
    setModelConfig(detail.model_config ?? defaultQuestModelConfig);
    setWorkspaceAutoWrite(detail.autonomy?.workspace_writes_automatic ?? true);
    setTitleDraft(detail.title);
  }, []);

  const refreshList = useCallback(async (preferredId?: string | null) => {
    const result = await listQuests();
    setQuests(result.quests);
    const id = preferredId === null ? result.quests[0]?.id : preferredId ?? selected?.id ?? result.quests[0]?.id;
    if (id) {
      const detail = await getQuest(id);
      syncQuestDrafts(detail);
      setPanel(defaultPanelForQuest(detail));
      resetReviewSelection(detail);
    } else {
      setSelected(null);
      setIntentDraft('');
      setSpecDraft('');
      setQuestMode('solo');
      setModelConfig(defaultQuestModelConfig);
      setWorkspaceAutoWrite(true);
      setSelectedReviewFiles(new Set());
      setSelectedReviewGroups(new Set());
    }
  }, [resetReviewSelection, selected?.id, syncQuestDrafts]);

  const refreshKnowledge = useCallback(async () => {
    const result = await listKnowledge();
    setKnowledge(result.entries);
  }, []);

  useEffect(() => {
    refreshList(initialQuestId).catch(reportError);
    refreshKnowledge().catch(reportError);
  }, []); // Load the cross-project registry once on entry.

  useEffect(() => {
    let canceled = false;
    async function loadQuestModels() {
      const settings = await rpc<{ provider: string; model: string; has_api_key?: boolean }>('app/get_copilot_settings').catch(() => null);
      const provider = settings?.provider && settings.provider !== 'stub' ? settings.provider : null;
      if (!canceled) {
        setCanUseOpenAIVoiceInput(provider === 'openai' && settings?.has_api_key === true);
      }
      let models: QuestModelOption[] = [];
      if (provider) {
        models = await rpc<QuestModelOption[]>('app/detect_models', { provider }).catch(() => []);
      }
      if (models.length === 0) {
        const registry = await rpc<{ providers: Array<{ provider: string; models: QuestModelOption[] }> }>('app/get_model_registry')
          .catch(() => ({ providers: [] }));
        models = registry.providers.flatMap(meta => meta.models.map(model => ({ ...model, provider: model.provider || meta.provider })));
      }
      if (!canceled) {
        setModelOptions(models);
        const preferred = settings?.model || models[0]?.id || '';
        setModelConfig(prev => ({
          ...prev,
          provider: models.find(model => model.id === (prev.model || preferred))?.provider ?? provider ?? prev.provider,
          model: prev.model || preferred,
          max_tokens: models.find(model => model.id === (prev.model || preferred))?.default_max_tokens ?? prev.max_tokens,
        }));
      }
    }
    loadQuestModels().catch(reportError);
    return () => {
      canceled = true;
    };
  }, [reportError]);

  const visibleQuests = useMemo(() => {
    const active = quests.filter(quest => ACTIVE_STATUSES.includes(quest.status));
    const history = quests.filter(quest => !ACTIVE_STATUSES.includes(quest.status));
    return { active, history };
  }, [quests]);

  const modelSelectOptions = useMemo<QuestSelectOption[]>(() => {
    if (modelOptions.length === 0) {
      return [{ value: '', label: t('quest_no_models_available') }];
    }
    return modelOptions.map(option => ({
      value: option.id,
      label: option.display_name || option.id,
      description: option.provider,
    }));
  }, [modelOptions, t]);

  const thinkingOptions = useMemo<QuestSelectOption[]>(() => [
    { value: 'off', label: t('quest_thinking_off') },
    { value: 'low', label: t('quest_thinking_low') },
    { value: 'medium', label: t('quest_thinking_medium') },
    { value: 'high', label: t('quest_thinking_high') },
  ], [t]);

  const selectQuest = useCallback(async (quest: QuestRecord) => {
    setError(null);
    try {
      const detail = await getQuest(quest.id);
      syncQuestDrafts(detail);
      setRenaming(false);
      setArtifact(null);
      setPanel(defaultPanelForQuest(detail));
      resetReviewSelection(detail);
    } catch (reason) {
      reportError(reason);
    }
  }, [reportError, resetReviewSelection, syncQuestDrafts]);

  const selectQuestById = useCallback(async (questId: string): Promise<QuestDetail | null> => {
    setError(null);
    try {
      const detail = await getQuest(questId);
      syncQuestDrafts(detail);
      setRenaming(false);
      setArtifact(null);
      setPanel(defaultPanelForQuest(detail));
      resetReviewSelection(detail);
      return detail;
    } catch (reason) {
      reportError(reason);
      return null;
    }
  }, [reportError, resetReviewSelection, syncQuestDrafts]);

  const create = useCallback(async () => {
    if (!goal.trim() || rewritingPrompt) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await createQuest('', goal.trim(), {
        mode: questMode,
        model_config: {
          ...modelConfig,
          provider: modelOptions.find(model => model.id === modelConfig.model)?.provider ?? modelConfig.provider,
        },
      });
      setGoal('');
      syncQuestDrafts(detail);
      setArtifact(null);
      setPanel('spec');
      resetReviewSelection(detail);
      await refreshList(detail.id);
      await refreshKnowledge();
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [goal, modelConfig, modelOptions, questMode, refreshList, rewritingPrompt, syncQuestDrafts]);

  const rewritePrompt = useCallback(async () => {
    if (rewritingPrompt) {
      rewriteRequestRef.current?.cancel();
      return;
    }
    if (!goal.trim()) return;
    const originalPrompt = goal;
    let streamedPrompt = '';
    setError(null);
    setRewritingPrompt(true);
    try {
      const selectedModel = modelOptions.find(model => model.id === modelConfig.model);
      const request = rewriteQuestPrompt(
        goal.trim(),
        {
          ...modelConfig,
          provider: selectedModel?.provider ?? modelConfig.provider,
        },
        (delta, kind) => {
          if (kind !== 'text' || !delta) return;
          streamedPrompt += delta;
          setGoal(streamedPrompt);
        },
      );
      rewriteRequestRef.current = request;
      const result = await request.promise;
      setGoal(result.prompt);
    } catch (reason) {
      setGoal(originalPrompt);
      if (!(reason instanceof Error && reason.message === 'cancelled')) {
        reportError(reason);
      }
    } finally {
      rewriteRequestRef.current = null;
      setRewritingPrompt(false);
    }
  }, [goal, modelConfig, modelOptions, reportError, rewritingPrompt]);

  useEffect(() => () => {
    rewriteRequestRef.current?.cancel();
  }, []);

  const startOpenAIVoiceInput = useCallback(async () => {
    if (voiceInputStatus !== 'idle') {
      stopVoiceInput();
      return;
    }
    if (!canUseOpenAIVoiceInput || !currentProjectPath) return;
    setError(null);
    setVoiceInputStatus('connecting');
    try {
      await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const session = await createOpenAIRealtimeTranscriptionSession();
      const clientSecret = session.session.client_secret?.value;
      if (!clientSecret) {
        throw new Error('OpenAI Realtime transcription session did not return a client secret.');
      }

      const peer = new RTCPeerConnection();
      const dataChannel = peer.createDataChannel('oai-events');
      stream.getTracks().forEach(track => peer.addTrack(track, stream));
      const audioContext = new AudioContext();
      const analyser = audioContext.createAnalyser();
      analyser.fftSize = 1024;
      const source = audioContext.createMediaStreamSource(stream);
      source.connect(analyser);

      dataChannel.onmessage = event => {
        let payload: Record<string, unknown>;
        try {
          payload = JSON.parse(event.data);
        } catch {
          return;
        }
        const type = typeof payload.type === 'string' ? payload.type : '';
        const transcript =
          typeof payload.transcript === 'string'
            ? payload.transcript
            : typeof payload.delta === 'string'
              ? payload.delta
              : typeof payload.text === 'string'
                ? payload.text
                : '';
        if (!transcript) return;
        if (type.includes('delta')) {
          setGoal(current => current.endsWith(transcript) ? current : `${current}${transcript}`);
        } else if (type.includes('completed') || type.includes('done') || type.includes('final')) {
          setGoal(current => {
            const separator = current.trim().length > 0 && !current.endsWith(' ') && !current.endsWith('\n') ? ' ' : '';
            return `${current}${separator}${transcript}`.trimStart();
          });
        }
      };

      const offer = await peer.createOffer();
      await peer.setLocalDescription(offer);
      const endpoint = session.realtime_url;
      const response = await fetch(endpoint, {
        method: 'POST',
        headers: {
          Authorization: `Bearer ${clientSecret}`,
          'Content-Type': 'application/sdp',
        },
        body: offer.sdp,
      });
      if (!response.ok) {
        throw new Error(`OpenAI Realtime connection failed: ${response.status}`);
      }
      const answer = await response.text();
      await peer.setRemoteDescription({ type: 'answer', sdp: answer });
      const samples = new Uint8Array(analyser.fftSize);
      let silenceStartedAt = 0;
      const monitorAudio = () => {
        analyser.getByteTimeDomainData(samples);
        let total = 0;
        for (const sample of samples) {
          const centered = sample - 128;
          total += centered * centered;
        }
        const rms = Math.sqrt(total / samples.length) / 128;
        const now = performance.now();
        if (rms > 0.025) {
          silenceStartedAt = now;
        } else if (silenceStartedAt > 0 && now - silenceStartedAt > 3000) {
          stopVoiceInput();
          return;
        } else if (silenceStartedAt === 0) {
          silenceStartedAt = now;
        }
        const current = voiceInputRef.current;
        if (current) {
          current.animationFrame = window.requestAnimationFrame(monitorAudio);
        }
      };
      voiceInputRef.current = {
        peer,
        stream,
        dataChannel,
        audioContext,
        analyser,
        animationFrame: window.requestAnimationFrame(monitorAudio),
      };
      setVoiceInputStatus('recording');
    } catch (reason) {
      stopVoiceInput();
      reportError(reason);
    }
  }, [canUseOpenAIVoiceInput, currentProjectPath, reportError, stopVoiceInput, voiceInputStatus]);

  const saveSpec = useCallback(async () => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await updateQuestSpec(selected.id, specDraft);
      setSelected(detail);
      setTitleDraft(detail.title);
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, selected, specDraft]);

  const saveIntent = useCallback(async () => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await updateQuestIntent(selected.id, intentDraft);
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [intentDraft, refreshList, selected]);

  const saveExecutionConfig = useCallback(async () => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await updateQuestExecutionConfig(
        selected.id,
        questMode,
        {
          ...modelConfig,
          provider: modelOptions.find(model => model.id === modelConfig.model)?.provider ?? modelConfig.provider,
        },
        {
          ...selected.autonomy,
          workspace_writes_automatic: workspaceAutoWrite,
          active_project_apply_requires_approval: true,
        },
      );
      syncQuestDrafts(detail);
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [modelConfig, modelOptions, questMode, refreshList, reportError, selected, syncQuestDrafts, workspaceAutoWrite]);

  const execute = useCallback(async () => {
    if (!selected) return;
    setBusy(true);
    setExecutingQuestId(selected.id);
    setError(null);
    try {
      if (intentDraft !== selected.intent) {
        await updateQuestIntent(selected.id, intentDraft);
      }
      if (specDraft !== selected.spec) {
        await updateQuestSpec(selected.id, specDraft);
      }
      setSelected(prev => prev && prev.id === selected.id ? { ...prev, status: 'running' } : prev);
      setPanel('overview');
      const detail = await executeQuest(selected.id);
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      resetReviewSelection(detail);
      setArtifact(null);
      setPanel('overview');
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setExecutingQuestId(null);
      setBusy(false);
    }
  }, [intentDraft, refreshList, resetReviewSelection, selected, specDraft]);

  const transition = useCallback(async (status: QuestStatus) => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await transitionQuest(selected.id, status);
      setSelected(detail);
      setTitleDraft(detail.title);
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, selected]);

  const rename = useCallback(async () => {
    if (!selected || !titleDraft.trim() || titleDraft.trim() === selected.title) {
      setRenaming(false);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const detail = await renameQuest(selected.id, titleDraft.trim());
      setSelected(detail);
      setTitleDraft(detail.title);
      setRenaming(false);
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, selected, titleDraft]);

  const branchSelectedQuest = useCallback(async () => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await branchQuest(selected.id, `${selected.title} branch`);
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      setArtifact(null);
      resetReviewSelection(detail);
      setPanel(defaultPanelForQuest(detail));
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, reportError, resetReviewSelection, selected]);

  const deleteQuestById = useCallback(async (id: string) => {
    setBusy(true);
    setError(null);
    try {
      await deleteQuest(id);
      if (selected?.id === id) {
        clearSelectedQuest();
      }
      await refreshList(null);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [clearSelectedQuest, refreshList, reportError, selected?.id]);

  const remove = useCallback(async () => {
    if (!selected || selected.status !== 'archived') return;
    await deleteQuestById(selected.id);
  }, [deleteQuestById, selected]);

  const runQuestMenuAction = useCallback(async (quest: QuestRecord, menuAction: QuestMenuAction) => {
    if (menuAction === 'open') {
      await selectQuestById(quest.id);
      return;
    }
    if (menuAction === 'rename') {
      const detail = await selectQuestById(quest.id);
      if (detail) {
        setRenaming(true);
      }
      return;
    }
    if (menuAction === 'open_editor') {
      await onOpenEditor(quest.project.path, {
        questId: quest.id,
        questTitle: quest.title,
        kind: 'intent',
        label: 'Quest intent',
        path: quest.intent_path,
      });
      return;
    }
    if (menuAction === 'delete') {
      await deleteQuestById(quest.id);
      return;
    }

    setBusy(true);
    setError(null);
    try {
      const detail = menuAction === 'branch'
        ? await branchQuest(quest.id, `${quest.title} branch`)
        : menuAction === 'export'
          ? await exportQuest(quest.id)
          : menuAction === 'cancel'
            ? await cancelQuest(quest.id, 'Canceled from Quest list')
            : menuAction === 'archive'
              ? await transitionQuest(quest.id, 'archived')
              : menuAction === 'reopen'
                ? await reopenQuest(quest.id, 'Reopened from Quest list')
                : null;
      if (!detail) return;
      syncQuestDrafts(detail);
      resetReviewSelection(detail);
      setArtifact(null);
      setRenaming(false);
      setPanel(menuAction === 'reopen' ? 'spec' : defaultPanelForQuest(detail));
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [deleteQuestById, onOpenEditor, refreshList, reportError, resetReviewSelection, selectQuestById, syncQuestDrafts]);

  const action = selected ? statusAction(selected.status) : null;
  const artifactFor = useCallback((
    kind: QuestEditorArtifact['kind'],
    label: string,
    path?: string,
  ): QuestEditorArtifact | undefined => selected ? {
    questId: selected.id,
    questTitle: selected.title,
    kind,
    label,
    path,
  } : undefined, [selected]);

  const openArtifact = useCallback((nextArtifact: QuestArtifactSelection, nextPanel: QuestPanel = 'artifact') => {
    setArtifact(nextArtifact);
    setPanel(nextPanel);
  }, []);

  const applySelectedQuest = useCallback(async (files?: string[], transactionGroupIds?: string[]) => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const detail = transactionGroupIds
        ? await applyQuestTransactionGroups(selected.id, transactionGroupIds)
        : await applyQuest(selected.id, files);
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      resetReviewSelection(detail);
      setPanel('overview');
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, reportError, resetReviewSelection, selected]);

  const discardSelectedQuest = useCallback(async (files?: string[], transactionGroupIds?: string[]) => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const detail = transactionGroupIds
        ? await discardQuestTransactionGroups(selected.id, transactionGroupIds)
        : await discardQuest(selected.id, files);
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      resetReviewSelection(detail);
      setPanel('overview');
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, reportError, resetReviewSelection, selected]);

  const rollbackSelectedQuest = useCallback(async (rollbackId: string) => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await rollbackQuest(selected.id, rollbackId);
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      setPanel('review');
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, reportError, selected]);

  const exportSelectedQuest = useCallback(async () => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await exportQuest(selected.id);
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, reportError, selected]);

  const rejectSelectedQuest = useCallback(async () => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await rejectQuest(selected.id, 'Rejected reviewed Quest result');
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      setPanel('overview');
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, reportError, selected]);

  const cancelSelectedQuest = useCallback(async () => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await cancelQuest(selected.id, 'Canceled from Quest workspace');
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      setPanel('overview');
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, reportError, selected]);

  const reopenSelectedQuest = useCallback(async () => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await reopenQuest(selected.id, 'Reopened from Quest workspace');
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      setPanel('spec');
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, reportError, selected]);

  const reviseSelectedQuest = useCallback(async () => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await requestQuestRevision(selected.id, 'Requested Quest revision');
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      setPanel('spec');
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, reportError, selected]);

  const updateKnowledgeEntry = useCallback(async (
    action: 'approve' | 'reject' | 'remove',
    id: string,
  ) => {
    setBusy(true);
    setError(null);
    try {
      const result = action === 'approve'
        ? await approveKnowledge(id)
        : action === 'reject'
          ? await rejectKnowledge(id)
          : await removeKnowledge(id);
      setKnowledge(result.entries);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [reportError]);

  const revalidateKnowledgeEntries = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await revalidateKnowledge();
      setKnowledge(result.entries);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [reportError]);

  const toggleQuestKnowledge = useCallback(async (entry: KnowledgeEntry) => {
    if (!selected || entry.status !== 'approved') return;
    setBusy(true);
    setError(null);
    try {
      const currentlyAttached = selected.attached_knowledge_ids.includes(entry.id);
      const knowledgeIds = currentlyAttached
        ? selected.attached_knowledge_ids.filter(id => id !== entry.id)
        : [...selected.attached_knowledge_ids, entry.id];
      const detail = await updateQuestKnowledgeContext(selected.id, knowledgeIds);
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, reportError, selected]);

  const submitQuestInput = useCallback(async () => {
    if (!selected || !questInput.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await addQuestNote(selected.id, questInputMode, questInput.trim());
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      setQuestInput('');
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [questInput, questInputMode, refreshList, reportError, selected]);

  const requestQuickFix = useCallback(async (issue: string) => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await requestQuestQuickFix(selected.id, issue);
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      setPanel('overview');
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, reportError, selected]);

  const continueSelectedQuest = useCallback(async (reason?: string) => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await continueQuest(selected.id, reason ?? 'Continue Quest from current evidence');
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      setPanel('overview');
      await refreshList(detail.id);
    } catch (errorReason) {
      reportError(errorReason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, reportError, selected]);

  const runReviewAction = useCallback(async (reviewAction: QuestReviewAction) => {
    if (!selected) return;
    const target = reviewAction.target ?? reviewAction.label;
    if (reviewAction.kind === 'quick_fix') {
      await requestQuickFix(target);
      return;
    }
    if (reviewAction.kind === 'revise') {
      await reviseSelectedQuest();
      return;
    }
    if (reviewAction.kind === 'retry') {
      await execute();
      return;
    }
    if (reviewAction.kind === 'continue') {
      await continueSelectedQuest(target);
      return;
    }
    if (reviewAction.kind === 'archive') {
      await transition('archived');
      return;
    }
    if (reviewAction.kind === 'branch') {
      await branchSelectedQuest();
      return;
    }
    if (reviewAction.kind === 'apply_selected') {
      if (selected.review?.transaction_groups.length) {
        await applySelectedQuest(undefined, Array.from(selectedReviewGroups));
      } else {
        await applySelectedQuest(Array.from(selectedReviewFiles));
      }
      return;
    }
    if (reviewAction.kind === 'discard_selected') {
      if (selected.review?.transaction_groups.length) {
        await discardSelectedQuest(undefined, Array.from(selectedReviewGroups));
      } else {
        await discardSelectedQuest(Array.from(selectedReviewFiles));
      }
      return;
    }
    if (reviewAction.kind === 'open_review_finding') {
      openArtifact({ kind: 'review_finding', label: target });
    }
  }, [
    applySelectedQuest,
    branchSelectedQuest,
    continueSelectedQuest,
    discardSelectedQuest,
    execute,
    openArtifact,
    requestQuickFix,
    reviseSelectedQuest,
    selected,
    selectedReviewFiles,
    selectedReviewGroups,
    transition,
  ]);

  return (
    <div className={questClasses.shell}>
      <header className={questClasses.globalHeader}>
        <div className={questClasses.brand}>
          <span>Aster</span>
          <strong>{t('quest_title')}</strong>
        </div>
        <nav>
          <button className={cn(questClasses.topNavButton, questClasses.topNavButtonActive)}>{t('quest_title')}</button>
        </nav>
        <div className={questClasses.globalActions}>
          <button className={buttonBase} onClick={onCloseProject} title={t('quest_close_project')}><IconX /></button>
        </div>
      </header>

      <div className={questClasses.layout}>
        <aside className={questClasses.sidebar}>
          <div className={questClasses.sidebarHeading}>
            <button className={questClasses.newButton} onClick={() => setSelected(null)} disabled={!currentProjectPath}>
              <IconPlus /> {t('quest_new')} <kbd>Ctrl N</kbd>
            </button>
          </div>

          <QuestGroup label={t('quest_group_active')} quests={visibleQuests.active} selectedId={selected?.id} onSelect={selectQuest} onMenuAction={runQuestMenuAction} />
          <QuestGroup label={t('quest_group_history')} quests={visibleQuests.history} selectedId={selected?.id} onSelect={selectQuest} onMenuAction={runQuestMenuAction} />
          <div className={questClasses.sidebarFooter}>
            <button onClick={() => setPanel('knowledge')}>{t('quest_knowledge')} <b>{knowledge.filter(entry => entry.status === 'pending').length}</b></button>
            <button disabled>{t('quest_marketplace')}</button>
          </div>
        </aside>

        {!selected ? (
          <main className={questClasses.home}>
            <div className={questClasses.orb}><IconSparkles size={28} /></div>
            <h1 className="m-0 mb-3 text-[clamp(28px,3vw,40px)] font-[650] leading-[1.1] text-[var(--text-primary)]">{t('quest_home_title')}</h1>
            <div className={questClasses.startLine}>
              <span>{t('quest_start_in')}</span>
              <b>{currentProjectPath ? 'Aster' : t('quest_no_project')}</b>
              <span>{t('quest_local')}</span>
              <span>main</span>
            </div>
            <div className={questClasses.promptBox}>
              <textarea
                value={goal}
                onChange={event => setGoal(event.target.value)}
                onKeyDown={event => {
                  if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
                    event.preventDefault();
                    if (!busy && !rewritingPrompt && goal.trim() && currentProjectPath) {
                      create();
                    }
                  }
                }}
                placeholder={t('quest_goal_placeholder')}
                disabled={!currentProjectPath}
              />
              <footer>
                <div className="flex min-w-0 flex-1 items-center gap-1 text-[12px] text-[var(--text-secondary)]">
                  <QuestDropdown
                    value={questMode}
                    options={[
                      { value: 'solo', label: t('quest_mode_solo') },
                      { value: 'extra', label: t('quest_mode_extra') },
                    ]}
                    onChange={value => setQuestMode(value as QuestMode)}
                    disabled={busy}
                    compact
                    widthClass="w-[92px]"
                    menuWidthClass="w-[120px]"
                  />
                  <label className="inline-flex items-center gap-[6px] rounded-[5px] px-1 py-[5px] text-[11px] font-medium text-[var(--text-secondary)]">
                    <input
                      type="checkbox"
                      checked={workspaceAutoWrite}
                      onChange={event => setWorkspaceAutoWrite(event.target.checked)}
                      disabled={busy}
                    />
                    {t('quest_auto')}
                  </label>
                  <QuestDropdown
                    value={modelConfig.model}
                    options={modelSelectOptions}
                    onChange={value => {
                      const model = modelOptions.find(option => option.id === value);
                      setModelConfig(prev => ({
                        ...prev,
                        model: value,
                        provider: model?.provider ?? prev.provider,
                        max_tokens: model?.default_max_tokens ?? prev.max_tokens,
                      }));
                    }}
                    disabled={busy}
                    compact
                    widthClass="w-[150px]"
                    menuWidthClass="w-[220px]"
                  />
                </div>
                <div className="flex items-center gap-1">
                  <button
                    className={questClasses.promptIconButton}
                    disabled={busy || (!rewritingPrompt && !goal.trim())}
                    onClick={rewritePrompt}
                    title={rewritingPrompt ? t('quest_stop_rewriting') : t('quest_prompt_rewrite')}
                    type="button"
                  >
                    <PromptRewriteIcon active={rewritingPrompt} />
                  </button>
                  <button
                    className={cn(
                      questClasses.promptIconButton,
                      voiceInputStatus === 'recording' && 'bg-[var(--danger-dim)] text-[var(--danger)]',
                    )}
                    onClick={startOpenAIVoiceInput}
                    disabled={!canUseOpenAIVoiceInput || voiceInputStatus === 'connecting' || !currentProjectPath}
                    title={
                      canUseOpenAIVoiceInput
                        ? voiceInputStatus === 'recording'
                          ? t('quest_stop_voice_input')
                          : t('quest_voice_input')
                        : t('quest_voice_input_connect')
                    }
                    type="button"
                  >
                    <VoiceInputIcon status={voiceInputStatus} />
                  </button>
                <button className={questClasses.promptSubmit} onClick={create} disabled={busy || rewritingPrompt || !goal.trim() || !currentProjectPath} title={t('quest_create')}>
                  {busy ? <QuestLoader /> : <IconSend />}
                </button>
                </div>
              </footer>
            </div>
            <div className={questClasses.introCard}>
              <IconSparkles />
              <div>
                <strong>{t('quest_intro_title')}</strong>
                <p>{t('quest_intro_desc')}</p>
              </div>
            </div>
          </main>
        ) : (
          <main className={questClasses.workspace}>
            <header className={questClasses.header}>
              <div>
                <div className={questClasses.projectLine}>
                  <span>{selected.project.name}</span>
                  <IconChevronRight />
                  <span>{selected.id}</span>
                  {selected.branch_of && (
                    <>
                      <IconChevronRight />
                      <span>{t_fmt('quest_branched_from', { id: selected.branch_of })}</span>
                    </>
                  )}
                </div>
                {renaming ? (
                  <div className={questClasses.titleEdit}>
                    <input
                      value={titleDraft}
                      onChange={event => setTitleDraft(event.target.value)}
                      onKeyDown={event => {
                        if (event.key === 'Enter') rename();
                        if (event.key === 'Escape') {
                          setTitleDraft(selected.title);
                          setRenaming(false);
                        }
                      }}
                      autoFocus
                    />
                    <button onClick={rename} disabled={busy || !titleDraft.trim()}><IconCheck /> {t('btn_save')}</button>
                    <button onClick={() => { setTitleDraft(selected.title); setRenaming(false); }}><IconX /></button>
                  </div>
                ) : (
                  <h1>{selected.title}</h1>
                )}
                <p>{selected.goal}</p>
              </div>
              <div className={questClasses.headerActions}>
                <span className={cn('rounded-full border border-current px-2 py-[5px] text-[10px] font-extrabold uppercase', statusTextClass(selected.status))}>{selected.status}</span>
                <button className={buttonBase} onClick={() => onOpenEditor(
                  selected.project.path,
                  artifactFor('intent', t('quest_artifact_intent'), selected.intent_path),
                )}><IconCode /> {t('quest_open_editor')}</button>
                {action && (
                  <button
                    className={buttonBase}
                    onClick={() => action.labelKey === 'quest_action_resume'
                      ? continueSelectedQuest('Resume Quest from current evidence')
                      : transition(action.next)}
                    disabled={busy}
                  >
                    {t(action.labelKey)}
                  </button>
                )}
                {selected.status === 'ready_for_review' && (
                  <button className={buttonBase} onClick={rejectSelectedQuest} disabled={busy}>{t('quest_reject')}</button>
                )}
              </div>
            </header>

            <section className={questClasses.cockpit}>
              <div className={questClasses.runStream}>
                <div className={questClasses.streamPrompt}>{selected.goal}</div>
                <div className={questClasses.streamList}>
                  <article className={questClasses.streamEntry}>
                    <span className={questClasses.timelineDot} />
                    <div>
                      <header><strong>{t('quest_goal_accepted')}</strong><time>{formatTime(selected.created_at_ms)}</time></header>
                      <small>{t('quest_user_prompt')}</small>
                    </div>
                  </article>
                  {selected.events.map((event, index) => (
                    <article key={event.id} className={questClasses.streamEntry}>
                      <span className={cn(questClasses.timelineDot, index === selected.events.length - 1 && 'after:hidden')} />
                      <div>
                        <header><strong>{event.summary}</strong><time>{formatTime(event.timestamp_ms)}</time></header>
                        <small>{event.kind.replaceAll('_', ' ')}</small>
                        {hasEventDetails(event.details) && (
                          <details>
                            <summary>{t('quest_evidence')}</summary>
                            <pre>{formatEventDetails(event.details)}</pre>
                          </details>
                        )}
                      </div>
                    </article>
                  ))}
                  <article className={cn(questClasses.streamEntry, questClasses.nextEntry)}>
                    <span className={cn(questClasses.timelineDot, questClasses.timelineDotNext)} />
                    <div>
                      <header><strong>{selected.next_action.label}</strong><time>{t('quest_time_next')}</time></header>
                      <small>{selected.next_action.reason}</small>
                    </div>
                  </article>
                  {['draft', 'specified'].includes(selected.status) && (
                    <article className={cn(questClasses.streamEntry, questClasses.nextEntry)}>
                      <span className={cn(questClasses.timelineDot, questClasses.timelineDotNext)} />
                      <div>
                        <header><strong>{t('quest_spec_ready')}</strong><time>{t('quest_time_next')}</time></header>
                        <small>{t('quest_spec_ready_desc')}</small>
                      </div>
                    </article>
                  )}
                  {executingQuestId === selected.id && (
                    <article className={cn(questClasses.streamEntry, questClasses.liveEntry)}>
                      <span className={cn(questClasses.timelineDot, questClasses.timelineDotLive, questClasses.timelineDotLast)} />
                      <div>
                        <header><strong>{t('quest_execution_running')}</strong><time>{t('quest_time_live')}</time></header>
                        <small>{t('quest_execution_running_desc')}</small>
                      </div>
                    </article>
                  )}
                  {selected.review && (
                    <button className={questClasses.reviewChip} onClick={() => setPanel('review')}>
                      <IconCheck /> {t('quest_tab_review')} +{selected.review.changed_files.reduce((sum, file) => sum + file.additions, 0)}
                      <span>-{selected.review.changed_files.reduce((sum, file) => sum + file.deletions, 0)}</span>
                    </button>
                  )}
                </div>
                <div className={questClasses.steerBar}>
                  <select
                    value={questInputMode}
                    onChange={event => setQuestInputMode(event.target.value as QuestInputMode)}
                    disabled={busy}
                  >
                    <option value="steer">{t('quest_input_steer')}</option>
                    <option value="clarify">{t('quest_input_clarify')}</option>
                    <option value="manual_intervention">{t('quest_input_manual')}</option>
                    <option value="pause">{t('quest_input_pause')}</option>
                  </select>
                  <input
                    value={questInput}
                    onChange={event => setQuestInput(event.target.value)}
                    onKeyDown={event => {
                      if (event.key === 'Enter') submitQuestInput();
                    }}
                    placeholder={t('quest_input_placeholder')}
                    disabled={busy || !selected}
                  />
                  <button className={questClasses.sendButton} onClick={submitQuestInput} disabled={busy || !questInput.trim()}>
                    <IconSend />
                  </button>
                </div>
              </div>

              <aside className={questClasses.rightPanel}>
                <div className={questClasses.panelTabs}>
                  {panelTabs.map(tab => {
                    const active = panel === tab.id;
                    return (
                      <button
                        key={tab.id}
                        className={cn(questClasses.panelTab, active && questClasses.panelTabActive)}
                        onClick={() => setPanel(tab.id)}
                        title={t(tab.labelKey)}
                        aria-label={t(tab.labelKey)}
                        aria-current={active ? 'page' : undefined}
                      >
                        {tab.icon}
                        {active && <span>{t(tab.labelKey)}</span>}
                      </button>
                    );
                  })}
                </div>

                {panel === 'overview' && (
                  <div className={questClasses.overview}>
                    <section>
                      <h2>{t('quest_progress')}</h2>
                      <ol className="m-0 grid list-none gap-[10px] p-0">
                        {progressItems(selected, t).map((item, index) => (
                          <li key={`${item.title}-${index}`} className={progressItemClass(item.status)}>
                            <span>{item.status === 'done' ? <IconCheck /> : null}</span>
                            <p>{item.title}</p>
                          </li>
                        ))}
                      </ol>
	                    </section>

	                    <section>
	                      <h2>{t('quest_execution')} <b>{selected.mode}</b></h2>
	                      <div className="grid grid-cols-[repeat(auto-fit,minmax(160px,1fr))] gap-2">
	                        <div className={modeSwitchClass}>
	                          {(['solo', 'extra'] as QuestMode[]).map(mode => (
	                            <button
	                              key={mode}
	                              className={modeSwitchButtonClass(questMode === mode)}
	                              onClick={() => setQuestMode(mode)}
	                              disabled={busy || executionLockedStatuses.includes(selected.status)}
	                            >
	                              {mode === 'solo' ? t('quest_mode_solo') : t('quest_mode_extra')}
	                            </button>
	                          ))}
	                        </div>
	                        <QuestDropdown
	                          value={modelConfig.model}
	                          options={modelSelectOptions}
	                          onChange={value => {
	                            const model = modelOptions.find(option => option.id === value);
	                            setModelConfig(prev => ({
	                              ...prev,
	                              model: value,
	                              provider: model?.provider ?? prev.provider,
	                              max_tokens: model?.default_max_tokens ?? prev.max_tokens,
	                            }));
	                          }}
	                          disabled={busy || executionLockedStatuses.includes(selected.status)}
	                          menuWidthClass="w-[260px]"
	                        />
	                        <QuestDropdown
	                          value={modelConfig.thinking_effort}
	                          options={thinkingOptions}
	                          onChange={value => setModelConfig(prev => ({ ...prev, thinking_effort: value }))}
	                          disabled={busy || executionLockedStatuses.includes(selected.status)}
	                        />
                        <input
                          className={executionSelectClass}
	                          type="number"
	                          min={1}
	                          value={modelConfig.max_tokens}
	                          onChange={event => setModelConfig(prev => ({ ...prev, max_tokens: Number(event.target.value) || 4096 }))}
	                          disabled={busy || executionLockedStatuses.includes(selected.status)}
	                        />
	                      </div>
	                      <div className="mt-2 flex flex-wrap items-center gap-2 text-[11px] text-[var(--text-muted)]">
	                        <label className={executionToggleClass}>
	                          <input
	                            type="checkbox"
	                            checked={workspaceAutoWrite}
	                            onChange={event => setWorkspaceAutoWrite(event.target.checked)}
	                            disabled={busy || executionLockedStatuses.includes(selected.status)}
	                          />
	                          {t('quest_workspace_auto_write')}
	                        </label>
	                        <span className="inline-flex h-8 items-center rounded-[5px] border border-[var(--border)] bg-[var(--bg-base)] px-[10px] text-[11px] text-[var(--text-muted)]">{t('quest_active_apply_approval')}</span>
	                        <button
	                          className={sectionHeadingButton}
	                          onClick={saveExecutionConfig}
	                          disabled={busy || (questMode === selected.mode && workspaceAutoWrite === selected.autonomy.workspace_writes_automatic && JSON.stringify(modelConfig) === JSON.stringify(selected.model_config))}
	                        >
                          <IconEdit /> {t('quest_save_execution')}
                        </button>
                      </div>
                    </section>

                    <section>
                      <h2>{t('quest_artifacts')}</h2>
                      <button className={artifactRowClass} onClick={() => { setArtifact(null); setPanel('intent'); }}>
                        <IconFile /><span><strong>{t('quest_artifact_intent')}</strong><small>{selected.intent_path}</small></span>
                      </button>
                      <button className={artifactRowClass} onClick={() => { setArtifact(null); setPanel('spec'); }}>
                        <IconFile /><span><strong>{t('quest_artifact_spec')}</strong><small>{selected.spec_path ?? t('quest_not_generated')}</small></span>
                      </button>
                      <button className={artifactRowClass} onClick={() => openArtifact({ kind: 'trace', label: t('quest_timeline_trace'), path: selected.trace_path })}>
                        <IconFile /><span><strong>{t('quest_timeline_trace')}</strong><small>{selected.trace_path}</small></span>
                      </button>
                      {selected.checkpoints.map(checkpoint => (
                        <button
                          className={artifactRowClass}
                          key={checkpoint.id}
                          onClick={() => openArtifact({ kind: 'checkpoint', label: checkpoint.label, path: checkpoint.artifact_path ?? checkpoint.id })}
                        >
                          <IconRefresh />
                          <span>
                            <strong>{checkpoint.label}</strong>
                            <small>{checkpoint.workspace_id ?? t('quest_workspace_pending')} · {formatTime(checkpoint.timestamp_ms)}</small>
                          </span>
                        </button>
                      ))}
                      {selected.review?.exploration_attempts.map(attempt => (
                        <button
                          className={artifactRowClass}
                          key={attempt.id}
                          onClick={() => openArtifact({ kind: 'exploration', label: attempt.label, path: attempt.artifact_path })}
                        >
                          <IconSparkles /><span><strong>{attempt.label}</strong><small>{attempt.outcome}{attempt.selected ? ` · ${t('quest_selected')}` : ''}</small></span>
                        </button>
                      ))}
                      <button className={artifactRowClass} onClick={() => setPanel('review')} disabled={!selected.review}>
                        <IconCheck /><span><strong>{t('quest_review_bundle')}</strong><small>{selected.review ? t_fmt('quest_changed_files_count', { count: String(selected.review.changed_files.length) }) : t('quest_not_ready')}</small></span>
                      </button>
                    </section>

                    <section>
                      <h2>{t('quest_changed_files')} <b>{selected.review?.changed_files.length ?? 0}</b></h2>
                      {selected.review?.changed_files.map(file => (
                        <button
                          className={cn(fileRowClass, 'cursor-pointer')}
                          key={file.path}
                          onClick={() => openArtifact({ kind: 'changed_file', label: file.path, path: file.path })}
                        >
                          <IconFile /><span><strong>{file.path}</strong><small>{file.status}</small></span>
                          <b>+{file.additions} <i>-{file.deletions}</i></b>
                        </button>
                      )) ?? <p className={mutedText}>{t('quest_no_file_changes')}</p>}
                    </section>

                    <section>
                      <h2>{t('quest_validation')}</h2>
                      {selected.review?.validations.map(validation => (
                        <button
                          className={validationRowClass}
                          key={validation.name}
                          onClick={() => openArtifact({ kind: 'validation', label: validation.name })}
                        >
                          <IconCheck />
                          <span>
                            <strong>{validation.name}</strong>
                            <small>
                              {validation.summary}
                              {validation.command && ` · ${validation.policy_approved ? t('quest_policy_approved') : t('quest_unapproved')}: ${validation.command}`}
                              {validation.log && ` · ${t('quest_log_attached')}`}
                            </small>
                          </span>
                          <b>{validation.status}</b>
                        </button>
                      )) ?? <p className={mutedText}>{t('quest_no_validation')}</p>}
                    </section>

                    <section>
                      <h2>{t('quest_references')}</h2>
                      {selected.attached_knowledge.length === 0 ? (
                        <p className={mutedText}>{t('quest_no_knowledge_attached')}</p>
                      ) : selected.attached_knowledge.map(entry => (
                        <button className={artifactRowClass} key={entry.id} onClick={() => setPanel('knowledge')}>
                          <IconSparkles /><span><strong>{entry.category}</strong><small>{entry.content}</small></span>
                        </button>
                      ))}
                    </section>
                  </div>
                )}

                {panel === 'intent' && (
                  <div className={documentPanelClass}>
                    <div className={questClasses.sectionHeading}>
                      <div><span>{t('quest_durable_intent')}</span><strong>{t('quest_durable_intent_desc')}</strong></div>
                      <div>
                        <button className={sectionHeadingButton} onClick={saveIntent} disabled={busy || intentDraft === selected.intent}><IconEdit /> {t('btn_save')}</button>
                      </div>
                    </div>
                    <textarea
                      value={intentDraft}
                      onChange={event => setIntentDraft(event.target.value)}
                      disabled={!['draft', 'clarifying', 'specified', 'planning', 'waiting_for_user', 'blocked'].includes(selected.status)}
                    />
                  </div>
                )}

                {panel === 'spec' && (
                  <div className={documentPanelClass}>
                    <div className={questClasses.sectionHeading}>
                      <div><span>{t('quest_ai_tool_spec')}</span><strong>{t('quest_ai_tool_spec_desc')}</strong></div>
                      <div>
                        <button className={sectionHeadingButton} onClick={saveSpec} disabled={busy || specDraft === selected.spec}><IconEdit /> {t('btn_save')}</button>
                        <button className={cn(sectionHeadingButton, primaryButton)} onClick={execute} disabled={busy || selected.status === 'archived'}>
                          {busy ? <QuestLoader /> : <IconPlay />} {t('quest_approve')}
                        </button>
                      </div>
                    </div>
                    <textarea
                      value={specDraft}
                      onChange={event => setSpecDraft(event.target.value)}
                      disabled={!['draft', 'clarifying', 'specified', 'planning', 'waiting_for_user', 'blocked'].includes(selected.status)}
                    />
                  </div>
                )}

                {panel === 'artifact' && artifact && (
                  <div className={questClasses.artifactViewer}>
                    <header>
                      <button className={sectionHeadingButton} onClick={() => setPanel('overview')}><IconChevronRight /> {t('quest_tab_overview')}</button>
                      <button className={sectionHeadingButton} onClick={() => onOpenEditor(selected.project.path, artifactFor(artifact.kind, artifact.label, artifact.path))}><IconCode /> {t('quest_open_editor')}</button>
                    </header>
                    <div>
                      <span>{artifact.kind.replace('_', ' ')}</span>
                      <h2>{artifact.label}</h2>
                      {artifact.path && <p>{artifact.path}</p>}
                    </div>
                    <pre>{artifact.kind === 'changed_file'
                      ? selected.review?.changed_files.find(file => file.path === artifact.path)?.diff ?? t('quest_no_diff')
                      : JSON.stringify(
                        artifact.kind === 'trace'
                          ? selected.events
                          : artifact.kind === 'exploration'
                            ? selected.review?.exploration_attempts.find(attempt => attempt.artifact_path === artifact.path || attempt.label === artifact.label)
                            : artifact.kind === 'checkpoint'
                              ? selected.checkpoints.find(checkpoint => checkpoint.artifact_path === artifact.path || checkpoint.label === artifact.label || checkpoint.id === artifact.path)
                              : artifact.kind === 'validation'
                                ? selected.review?.validations.find(validation => validation.name === artifact.label)
                                : artifact.kind === 'review_finding'
                                  ? selected.review?.findings.find(finding => finding.artifact_path === artifact.path || finding.title === artifact.label || finding.summary === artifact.label)
                                    ?? selected.review?.unresolved_issues.find(issue => issue === artifact.label)
                                  : selected.review?.unresolved_issues.find(issue => issue === artifact.label),
                        null,
                        2,
                      )}</pre>
                  </div>
                )}

                {panel === 'knowledge' && (
                  <div className={questClasses.knowledge}>
                    <section>
                      <h2>
                        {t('quest_pending_knowledge')} <b>{knowledge.filter(entry => entry.status === 'pending').length}</b>
                        <button className={sectionHeadingButton} onClick={revalidateKnowledgeEntries} disabled={busy}>{t('quest_revalidate')}</button>
                      </h2>
                      {knowledge.filter(entry => entry.status === 'pending').length === 0 ? (
                        <p className={mutedText}>{t('quest_no_pending_knowledge')}</p>
                      ) : knowledge.filter(entry => entry.status === 'pending').map(entry => (
                        <KnowledgeRow
                          key={entry.id}
                          entry={entry}
                          busy={busy}
                          attached={selected?.attached_knowledge_ids.includes(entry.id) ?? false}
                          onToggleAttach={() => toggleQuestKnowledge(entry)}
                          onApprove={() => updateKnowledgeEntry('approve', entry.id)}
                          onReject={() => updateKnowledgeEntry('reject', entry.id)}
                          onRemove={() => updateKnowledgeEntry('remove', entry.id)}
                        />
                      ))}
                    </section>
                    <section>
                      <h2>{t('quest_approved_knowledge')} <b>{knowledge.filter(entry => entry.status === 'approved').length}</b></h2>
                      {knowledge.filter(entry => entry.status === 'approved').length === 0 ? (
                        <p className={mutedText}>{t('quest_no_approved_knowledge')}</p>
                      ) : knowledge.filter(entry => entry.status === 'approved').map(entry => (
                        <KnowledgeRow
                          key={entry.id}
                          entry={entry}
                          busy={busy}
                          attached={selected?.attached_knowledge_ids.includes(entry.id) ?? false}
                          onToggleAttach={() => toggleQuestKnowledge(entry)}
                          onApprove={() => updateKnowledgeEntry('approve', entry.id)}
                          onReject={() => updateKnowledgeEntry('reject', entry.id)}
                          onRemove={() => updateKnowledgeEntry('remove', entry.id)}
                        />
                      ))}
                    </section>
                  </div>
                )}

                {panel === 'review' && (
                  <div className={questClasses.review}>
                    {!selected.review ? (
                      <div className={questClasses.reviewEmpty}>
                        <IconCheck size={24} />
                        <strong>{t('quest_no_review_bundle')}</strong>
                        <span>{t('quest_approve_spec_first')}</span>
                      </div>
                    ) : (
                      <>
                        <div className={questClasses.reviewSummary}>
                          <div><span>{t('quest_risk')}</span><strong>{selected.review.risk}</strong></div>
                          <p>{selected.review.summary}</p>
                        </div>
                        <section>
                          <h2>{t('quest_capability_metrics')}</h2>
                          <div className={questClasses.reviewMetrics}>
                            <div>
                              <span>{t('quest_metric_first_action')}</span>
                              <strong>{formatMetricDuration(selected.review.metrics?.intent_to_first_action_ms)}</strong>
                            </div>
                            <div>
                              <span>{t('quest_metric_tool_latency')}</span>
                              <strong>{formatMetricDuration(selected.review.metrics?.tool_call_latency_ms)}</strong>
                            </div>
                            <div>
                              <span>{t('quest_metric_validators')}</span>
                              <strong>{formatMetricDuration(selected.review.metrics?.validator_turnaround_ms)}</strong>
                            </div>
                            <div>
                              <span>{t('quest_metric_context_relevance')}</span>
                              <strong>{formatMetricScore(selected.review.metrics?.context_relevance_score)}</strong>
                            </div>
                            <div>
                              <span>{t('quest_metric_recovery')}</span>
                              <strong>{formatMetricScore(selected.review.metrics?.failed_action_recovery_rate)}</strong>
                            </div>
                            <div>
                              <span>{t('quest_metric_evidence_quality')}</span>
                              <strong>{formatMetricScore(selected.review.metrics?.review_evidence_quality_score)}</strong>
                            </div>
                            <div>
                              <span>{t('quest_metric_attempts')}</span>
                              <strong>{selected.review.metrics?.isolated_attempt_count ?? 0}</strong>
                            </div>
                            <div>
                              <span>{t('quest_metric_validation_failures')}</span>
                              <strong>{selected.review.metrics?.validation_failure_count ?? 0}/{selected.review.metrics?.validation_count ?? 0}</strong>
                            </div>
                          </div>
                          {(selected.review.metrics?.notes ?? []).length > 0 && (
                            <p className={questClasses.metricNote}>{selected.review.metrics?.notes?.join(' ')}</p>
                          )}
                        </section>
                        <section>
                          <h2>{t('quest_unresolved_issues')}</h2>
                          {(selected.review.findings ?? []).length > 0 ? (
                            (selected.review.findings ?? []).map(finding => (
                              <div className={issueClass} key={finding.id}>
                                <button
                                  className={issueOpenClass}
                                  onClick={() => openArtifact({
                                    kind: 'review_finding',
                                    label: finding.title,
                                    path: finding.artifact_path ?? undefined,
                                  })}
                                >
                                  <IconAlertCircle /> {finding.title} <span>{finding.severity}</span>
                                </button>
                                <p>{finding.summary}</p>
                                <button onClick={() => requestQuickFix(finding.summary)} disabled={busy}>{t('quest_quick_fix')}</button>
                              </div>
                            ))
                          ) : selected.review.unresolved_issues.length === 0
                            ? <div className={cn(issueClass, clearIssueClass)}><IconCheck /> {t('quest_no_unresolved_issues')}</div>
                            : selected.review.unresolved_issues.map(issue => (
                              <div className={issueClass} key={issue}>
                                <button
                                  className={issueOpenClass}
                                  onClick={() => openArtifact({ kind: 'review_finding', label: issue })}
                                >
                                  <IconAlertCircle /> {issue}
                                </button>
                                <button onClick={() => requestQuickFix(issue)} disabled={busy}>{t('quest_quick_fix')}</button>
                              </div>
                            ))}
                        </section>
                        <section>
                          <h2>{t('quest_next_actions')}</h2>
                          {(selected.review.next_actions ?? []).length === 0 ? (
                            <p className={mutedText}>{t('quest_no_next_action')}</p>
                          ) : (
                            <div className={questClasses.reviewActions}>
                              {(selected.review.next_actions ?? []).map(action => {
                                const needsSelection = action.kind === 'apply_selected' || action.kind === 'discard_selected';
                                const selectedActionDisabled = needsSelection
                                  && (selected.review?.transaction_groups.length
                                    ? selectedReviewGroups.size === 0
                                    : selectedReviewFiles.size === 0);
                                return (
                                  <button
                                    className={reviewActionButtonClass}
                                    key={action.id}
                                    onClick={() => runReviewAction(action)}
                                    disabled={busy || selectedActionDisabled}
                                  >
                                    {action.label}
                                  </button>
                                );
                              })}
                            </div>
                          )}
                        </section>
                        <section>
                          <h2>{t('quest_exploration_attempts')}</h2>
                          {selected.review.exploration_attempts.length === 0 ? (
                            <p className={mutedText}>{t('quest_no_attempts')}</p>
                          ) : selected.review.exploration_attempts.map(attempt => (
                            <button
                              className={cn(artifactRowClass, 'min-h-12 grid-cols-[18px_minmax(0,1fr)_auto] py-[9px] [&_small]:whitespace-normal [&>b]:text-[10px] [&>b]:uppercase [&>b]:text-[#52525b]')}
                              key={attempt.id}
                              onClick={() => openArtifact({ kind: 'exploration', label: attempt.label, path: attempt.artifact_path })}
                            >
                              <IconSparkles />
                              <span>
                                <strong>{attempt.label}</strong>
                                <small>{attempt.summary}</small>
                              </span>
                              <b>{attempt.selected ? t('quest_selected') : attempt.outcome}</b>
                            </button>
                          ))}
                        </section>
                        <section>
                          <h2>{t('quest_transaction_groups')}</h2>
                          {selected.review.transaction_groups.length > 0 ? (
                            selected.review.transaction_groups.map(group => {
                              const totals = group.files.reduce((acc, path) => {
                                const file = selected.review?.changed_files.find(item => item.path === path);
                                return {
                                  additions: acc.additions + (file?.additions ?? 0),
                                  deletions: acc.deletions + (file?.deletions ?? 0),
                                };
                              }, { additions: 0, deletions: 0 });
                              return (
                                <label className={transactionRowClass} key={group.id}>
                                  <input
                                    type="checkbox"
                                    checked={selectedReviewGroups.has(group.id)}
                                    onChange={event => {
                                      setSelectedReviewGroups(previous => {
                                        const next = new Set(previous);
                                        if (event.target.checked) {
                                          next.add(group.id);
                                        } else {
                                          next.delete(group.id);
                                        }
                                        return next;
                                      });
                                    }}
                                    disabled={busy || selected.status !== 'ready_for_review'}
                                  />
                                  <IconFile />
                                  <span>
                                    <strong>{group.label}</strong>
                                    <small>{group.summary} · {t_fmt('quest_files_count', { count: String(group.files.length) })} · {group.risk || t('quest_risk_pending')}</small>
                                  </span>
                                  <b>+{totals.additions} <i>-{totals.deletions}</i></b>
                                </label>
                              );
                            })
                          ) : selected.review.changed_files.length === 0 ? (
                            <p className={mutedText}>{t('quest_no_applicable_files')}</p>
                          ) : selected.review.changed_files.map(file => (
                            <label className={selectableFileRowClass} key={file.path}>
                              <input
                                type="checkbox"
                                checked={selectedReviewFiles.has(file.path)}
                                onChange={event => {
                                  setSelectedReviewFiles(previous => {
                                    const next = new Set(previous);
                                    if (event.target.checked) {
                                      next.add(file.path);
                                    } else {
                                      next.delete(file.path);
                                    }
                                    return next;
                                  });
                                }}
                                disabled={busy || selected.status !== 'ready_for_review'}
                              />
                              <IconFile /><span><strong>{file.path}</strong><small>{file.status}</small></span>
                              <b>+{file.additions} <i>-{file.deletions}</i></b>
                            </label>
                          ))}
                        </section>
                        <section>
                          <h2>{t('quest_final_decision')}</h2>
                          <div className={questClasses.decisionRow}>
                            <button
                              className={cn(decisionButtonClass, primaryButton)}
                              onClick={() => selected.review?.transaction_groups.length
                                ? applySelectedQuest(undefined, Array.from(selectedReviewGroups))
                                : applySelectedQuest(Array.from(selectedReviewFiles))
                              }
                              disabled={busy || selected.status !== 'ready_for_review' || (selected.review.transaction_groups.length ? selectedReviewGroups.size === 0 : selectedReviewFiles.size === 0)}
                            >
                              <IconCheck /> {t('quest_apply_selected')}
                            </button>
                            <button
                              className={decisionButtonClass}
                              onClick={() => applySelectedQuest()}
                              disabled={busy || selected.status !== 'ready_for_review' || selected.review.changed_files.length === 0}
                            >
                              {t('quest_apply_all')}
                            </button>
                            <button
                              className={decisionButtonClass}
                              onClick={() => selected.review?.transaction_groups.length
                                ? discardSelectedQuest(undefined, Array.from(selectedReviewGroups))
                                : discardSelectedQuest(Array.from(selectedReviewFiles))
                              }
                              disabled={busy || selected.status !== 'ready_for_review' || (selected.review.transaction_groups.length ? selectedReviewGroups.size === 0 : selectedReviewFiles.size === 0)}
                            >
                              <IconX /> {t('quest_discard_selected')}
                            </button>
                            <button className={decisionButtonClass} onClick={rejectSelectedQuest} disabled={busy || selected.status !== 'ready_for_review'}><IconX /> {t('quest_reject_result')}</button>
                            <button className={decisionButtonClass} onClick={reviseSelectedQuest} disabled={busy || !['ready_for_review', 'blocked', 'waiting_for_user'].includes(selected.status)}><IconRefresh /> {t('quest_request_revision')}</button>
                          </div>
                          {selected.decisions.length > 0 && (
                            <div className={questClasses.decisionHistory}>
                              {selected.decisions.map(decision => (
                                <div key={`${decision.kind}-${decision.timestamp_ms}`}>
                                  <small>{decision.kind.replace('_', ' ')} · {decision.summary}</small>
                                  {decision.rollback_id && decision.kind !== 'rollback' && (
                                    <button onClick={() => rollbackSelectedQuest(decision.rollback_id!)} disabled={busy}>
                                      {t('quest_roll_back')}
                                    </button>
                                  )}
                                </div>
                              ))}
                            </div>
                          )}
                        </section>
                      </>
                    )}
                  </div>
                )}
              </aside>
            </section>
          </main>
        )}
      </div>
      <footer className={questClasses.footer}>
        <span>{t('quest_registry')}</span>
        <span>{currentProjectPath ? t_fmt('quest_current_project', { path: currentProjectPath }) : t('quest_no_project_open')}</span>
      </footer>
      {error && (
        <button className={questClasses.errorToast} onClick={() => setErrorOpen(true)} title={t('quest_view_error_details')}>
          <IconAlertCircle />
          <span>
            <strong>{t('quest_failed')}</strong>
            <small>{error}</small>
          </span>
          <IconChevronRight />
        </button>
      )}
      {errorOpen && error && (
        <div className={questClasses.errorModal} role="dialog" aria-modal="true" aria-label={t('quest_error_details')}>
          <div>
            <header>
              <span><IconAlertCircle /> {t('quest_error')}</span>
              <button className={questClasses.modalButton} onClick={() => setErrorOpen(false)} title={t('btn_close')}><IconX /></button>
            </header>
            <pre>{error}</pre>
            <footer>
              <button className={questClasses.modalButton} onClick={() => setError(null)}>{t('quest_dismiss')}</button>
              <button className={cn(questClasses.modalButton, 'border-[#111827] bg-[#111827] text-white')} onClick={() => setErrorOpen(false)}>{t('btn_close')}</button>
            </footer>
          </div>
        </div>
      )}
    </div>
  );
}

function QuestGroup({
  label,
  quests,
  selectedId,
  onSelect,
  onMenuAction,
}: {
  label: string;
  quests: QuestRecord[];
  selectedId?: string;
  onSelect: (quest: QuestRecord) => void;
  onMenuAction?: (quest: QuestRecord, action: QuestMenuAction) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    quest: QuestRecord;
    deleteConfirm: boolean;
  } | null>(null);

  useEffect(() => {
    if (!contextMenu) return;
    const close = () => setContextMenu(null);
    window.addEventListener('click', close);
    window.addEventListener('keydown', close);
    return () => {
      window.removeEventListener('click', close);
      window.removeEventListener('keydown', close);
    };
  }, [contextMenu]);

  const menuIconSize = 14;
  const questContextMenuItemClass = `${contextMenuItemClass} min-h-8 gap-2 px-3.5 py-2 text-[13px]`;
  const questContextMenuDangerItemClass = `${questContextMenuItemClass} text-[var(--danger)] hover:bg-[var(--danger-dim)]`;

  const menuItems = useMemo(() => {
    if (!contextMenu) return [];
    const quest = contextMenu.quest;
    const items: Array<{ action: QuestMenuAction; label: string; icon: React.ReactNode; danger?: boolean; confirm?: boolean }> = [
      { action: 'open', label: t('quest_menu_open'), icon: <IconChevronRight size={menuIconSize} /> },
      { action: 'open_editor', label: t('quest_open_editor'), icon: <IconCode size={menuIconSize} /> },
      { action: 'rename', label: t('action_rename'), icon: <IconEdit size={menuIconSize} /> },
      { action: 'branch', label: t('quest_menu_branch'), icon: <IconSparkles size={menuIconSize} /> },
      { action: 'export', label: t('quest_menu_export'), icon: <IconFile size={menuIconSize} /> },
    ];
    if (!['archived', 'canceled', 'completed', 'running'].includes(quest.status)) {
      items.push({ action: 'cancel', label: t('dialog_cancel'), icon: <IconX size={menuIconSize} />, danger: true });
    }
    if (!['archived', 'canceled', 'running'].includes(quest.status)) {
      items.push({ action: 'archive', label: t('quest_menu_archive'), icon: <IconTrash size={menuIconSize} />, danger: true });
    }
    if (['archived', 'canceled', 'completed'].includes(quest.status)) {
      items.push({ action: 'reopen', label: t('quest_menu_reopen'), icon: <IconRefresh size={menuIconSize} /> });
    }
    if (quest.status === 'archived') {
      items.push({ action: 'delete', label: contextMenu.deleteConfirm ? t('action_confirm_delete') : t('action_delete'), icon: <IconTrash size={menuIconSize} />, danger: true, confirm: true });
    }
    return items;
  }, [contextMenu, t]);

  const handleMenuAction = useCallback(async (action: QuestMenuAction, confirm?: boolean) => {
    if (!contextMenu || !onMenuAction) return;
    if (confirm && !contextMenu.deleteConfirm) {
      setContextMenu(current => current ? { ...current, deleteConfirm: true } : current);
      return;
    }
    const quest = contextMenu.quest;
    setContextMenu(null);
    await onMenuAction(quest, action);
  }, [contextMenu, onMenuAction]);

  return (
    <section className={questClasses.group}>
      <header><span>{label}</span><b>{quests.length}</b></header>
      {quests.map(quest => (
        <button
          key={quest.id}
          className={cn(questClasses.groupButton, selectedId === quest.id && questClasses.groupButtonActive)}
          onClick={() => onSelect(quest)}
          onContextMenu={event => {
            if (!onMenuAction) return;
            event.preventDefault();
            setContextMenu({
              x: event.clientX,
              y: event.clientY,
              quest,
              deleteConfirm: false,
            });
          }}
        >
          <span className={cn('mt-[3px] h-[6px] w-[6px] rounded-full', statusDotClass(quest.status))} />
          <div>
            <strong>{quest.title}</strong>
            <small>{quest.project.name} · {formatTime(quest.updated_at_ms)}</small>
          </div>
        </button>
      ))}
      {quests.length === 0 && <p>{t('common_none')}</p>}
      {contextMenu && (
        <div
          className={`${contextMenuClass} fixed z-[1000] w-[190px] py-1.5`}
          style={{ left: contextMenu.x, top: contextMenu.y }}
          onClick={event => event.stopPropagation()}
        >
          {menuItems.map((item, index) => (
            <React.Fragment key={item.action}>
              {index === 5 && <div className={contextMenuSeparatorClass} />}
              {index === menuItems.length - 1 && item.action === 'delete' && <div className={contextMenuSeparatorClass} />}
              <button
                className={item.danger ? questContextMenuDangerItemClass : questContextMenuItemClass}
                onClick={() => handleMenuAction(item.action, item.confirm)}
              >
                {item.icon} {item.label}
              </button>
            </React.Fragment>
          ))}
        </div>
      )}
    </section>
  );
}

function KnowledgeRow({
  entry,
  busy,
  attached,
  onToggleAttach,
  onApprove,
  onReject,
  onRemove,
}: {
  entry: KnowledgeEntry;
  busy: boolean;
  attached: boolean;
  onToggleAttach: () => void;
  onApprove: () => void;
  onReject: () => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation();
  return (
    <article className={questClasses.knowledgeRow}>
      <header>
        <span>{entry.category}</span>
        <b className={statusTextClass(entry.status)}>{entry.status}</b>
      </header>
      <p>{entry.content}</p>
      <small>{entry.source}</small>
      <small className={statusTextClass(entry.reference_status)}>
        {entry.reference_status}: {entry.reference_summary}
      </small>
      <footer>
        {entry.status === 'approved' && (
          <button onClick={onToggleAttach} disabled={busy}>
            {attached ? <IconX /> : <IconPlus />} {attached ? t('quest_detach') : t('quest_attach')}
          </button>
        )}
        {entry.status !== 'approved' && <button onClick={onApprove} disabled={busy}><IconCheck /> {t('quest_approve')}</button>}
        {entry.status === 'pending' && <button onClick={onReject} disabled={busy}><IconX /> {t('quest_reject')}</button>}
        <button onClick={onRemove} disabled={busy}><IconTrash /> {t('inspector_remove_component')}</button>
      </footer>
    </article>
  );
}
