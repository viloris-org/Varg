import React, { useCallback, useEffect, useMemo, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
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
  executeQuest,
  approveKnowledge,
  applyQuestTransactionGroups,
  applyQuest,
  exportQuest,
  getQuest,
  listQuestProjectFiles,
  listKnowledge,
  listQuests,
  readQuestArtifact,
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
  updateQuestKnowledgeContext,
  updateQuestSpec,
  type KnowledgeEntry,
  type QuestDetail,
  type QuestAiStreamHandle,
  type QuestAiStreamKind,
  type QuestEvent,
  type QuestMode,
  type QuestModelConfig,
  type QuestProjectFile,
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
  IconMessageSquare,
  IconMic,
  IconMonitor,
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
type QuestQueueGroup = 'needs_action' | 'running' | 'recent' | 'archived';
type VoiceInputStatus = 'idle' | 'connecting' | 'recording';
type QuestMenuAction = 'open' | 'rename' | 'open_editor' | 'branch' | 'export' | 'cancel' | 'archive' | 'reopen' | 'delete';
type SpecMode = 'edit' | 'preview';
type LiveQuestActivity = {
  id: string;
  kind: QuestAiStreamKind | 'status';
  label: string;
  detail?: string;
  startedAt: number;
};
type SpecSelection = {
  text: string;
  top: number;
  left: number;
  mode: 'button' | 'input';
};
type QuestInputSuggestionMode = 'file' | 'command';
type QuestInputSuggestion = {
  id: string;
  label: string;
  detail: string;
  value: string;
  mode: QuestInputSuggestionMode;
  run?: () => void | Promise<void>;
};
type QuestInputFileToken = {
  path: string;
  kind: string;
};
type QuestQuestionOption = {
  id: string;
  label: string;
  description?: string | null;
};
type QuestQuestion = {
  id: string;
  prompt: string;
  options: QuestQuestionOption[];
  allow_multiple: boolean;
  allow_custom: boolean;
};
type QuestQuestionCard = {
  eventId: string;
  title: string;
  questions: QuestQuestion[];
};
type QuestTimelineBlockKind =
  | 'prompt'
  | 'thought'
  | 'plan'
  | 'workspace'
  | 'tool_group'
  | 'execution'
  | 'validation'
  | 'result'
  | 'decision'
  | 'debug';
type QuestTimelineBlock = {
  id: string;
  kind: QuestTimelineBlockKind;
  title: string;
  summary: string;
  events: QuestEvent[];
  defaultCollapsed: boolean;
  importance: 'primary' | 'secondary' | 'debug';
};

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
interface CopilotSettingsFull {
  provider: string;
  model: string;
  api_endpoint: string | null;
  api_key: string | null;
  has_api_key?: boolean;
  max_tokens: number;
  allowed_commands?: string[];
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

function extraQuestArtifacts(selected: QuestDetail): QuestArtifactSelection[] {
  const hidden = new Set([
    selected.intent_path,
    selected.spec_path ?? 'spec.md',
    selected.trace_path,
    ...(selected.checkpoints.map(checkpoint => checkpoint.artifact_path).filter(Boolean) as string[]),
    ...(selected.review?.exploration_attempts.map(attempt => attempt.artifact_path) ?? []),
    ...(selected.review?.findings.map(finding => finding.artifact_path).filter(Boolean) as string[]),
  ]);
  return selected.artifact_links
    .filter(artifact => artifact.path && !hidden.has(artifact.path))
    .map(artifact => ({
      kind: artifact.kind as QuestEditorArtifact['kind'],
      label: artifact.label,
      path: artifact.path,
    }));
}

function cn(...classes: Array<string | false | null | undefined>): string {
  return classes.filter(Boolean).join(' ');
}

const artifactPaneStorageKey = 'aster.quest.artifactPanePercent';
const artifactPaneMinPercent = 28;
const artifactPaneMaxPercent = 55;

function clampArtifactPanePercent(value: number): number {
  return Math.min(artifactPaneMaxPercent, Math.max(artifactPaneMinPercent, value));
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
const feedDecisionCardClass = 'ml-6 mt-3 grid w-[min(720px,calc(100%-36px))] overflow-hidden rounded-[6px] border border-[var(--border-light)] bg-[var(--bg-surface)] text-[var(--text-primary)] shadow-[var(--shadow-sm)] [&_header]:flex [&_header]:min-w-0 [&_header]:items-center [&_header]:justify-between [&_header]:gap-2 [&_header]:border-b [&_header]:border-[var(--border)] [&_header]:px-[10px] [&_header]:py-[7px] [&_header_span]:flex [&_header_span]:min-w-0 [&_header_span]:items-center [&_header_span]:gap-2 [&_header_span]:text-[11px] [&_header_span]:text-[var(--text-secondary)] [&_header_strong]:min-w-0 [&_header_strong]:overflow-hidden [&_header_strong]:text-ellipsis [&_header_strong]:whitespace-nowrap [&_header_strong]:font-semibold [&_header_strong]:text-[var(--text-primary)] [&_section]:grid [&_section]:gap-2 [&_section]:px-[10px] [&_section]:py-[9px] [&_p]:m-0 [&_p]:text-[12px] [&_p]:leading-[1.45] [&_footer]:flex [&_footer]:items-center [&_footer]:justify-between [&_footer]:gap-2 [&_footer]:px-[10px] [&_footer]:pb-[10px] [&_footer_button]:inline-flex [&_footer_button]:h-7 [&_footer_button]:cursor-pointer [&_footer_button]:items-center [&_footer_button]:gap-[6px] [&_footer_button]:rounded-[5px] [&_footer_button]:border [&_footer_button]:px-[9px] [&_footer_button]:text-[10px] [&_footer_button]:font-semibold [&_footer_button:disabled]:cursor-default [&_footer_button:disabled]:opacity-45';
const askAiSelectionButtonClass = 'absolute z-20 inline-flex min-h-8 cursor-pointer items-center gap-[7px] rounded-[var(--radius-md)] border border-[var(--brand)] bg-[var(--brand)] px-3 text-[11px] font-semibold text-[var(--bg-base)] shadow-[0_0_0_1px_var(--brand-dim),0_8px_18px_rgba(34,197,94,0.24)] hover:border-[var(--brand-hover)] hover:bg-[var(--brand-hover)] disabled:cursor-not-allowed disabled:opacity-60 [&_svg]:stroke-[2.25]';
const askAiSelectionPromptClass = 'absolute z-20 grid w-[min(360px,calc(100%-24px))] gap-2 rounded-[var(--radius-md)] border border-[var(--brand)] bg-[var(--bg-elevated)] p-2 shadow-[0_0_0_1px_var(--brand-dim),0_14px_34px_rgba(0,0,0,0.38)]';
const askAiSelectionPromptInputClass = 'min-h-[68px] resize-none rounded-[5px] border border-[var(--border-light)] bg-[var(--bg-base)] px-2.5 py-2 text-[12px] leading-[1.45] text-[var(--text-primary)] outline-none placeholder:text-[var(--text-muted)] focus:border-[var(--brand)]';
const askAiSelectionPromptActionsClass = 'flex items-center justify-end gap-1.5 [&_button]:inline-flex [&_button]:h-7 [&_button]:cursor-pointer [&_button]:items-center [&_button]:gap-1.5 [&_button]:rounded-[5px] [&_button]:border [&_button]:px-2.5 [&_button]:text-[10px] [&_button]:font-semibold [&_button:disabled]:cursor-default [&_button:disabled]:opacity-45';
const questInputSuggestClass = 'absolute bottom-[44px] left-3 z-30 grid max-h-[240px] w-[min(520px,calc(100%-24px))] overflow-auto rounded-[7px] border border-[var(--border-light)] bg-[var(--bg-elevated)] py-1 shadow-[var(--shadow-lg)]';
const questInputSuggestItemClass = (active: boolean) => cn(
  'grid cursor-pointer grid-cols-[18px_minmax(0,1fr)_auto] items-center gap-2 border-0 bg-transparent px-3 py-2 text-left text-[11px] text-[var(--text-secondary)]',
  active && 'bg-[var(--bg-hover)] text-[var(--text-primary)]',
  '[&_small]:overflow-hidden [&_small]:text-ellipsis [&_small]:whitespace-nowrap [&_small]:text-[10px] [&_small]:text-[var(--text-muted)] [&_strong]:overflow-hidden [&_strong]:text-ellipsis [&_strong]:whitespace-nowrap [&_strong]:text-[12px] [&_strong]:font-medium [&_kbd]:font-mono [&_kbd]:text-[10px] [&_kbd]:text-[var(--text-muted)]',
);
const questInputTokenBoxClass = 'grid min-w-0 gap-1.5';
const questInputTextRowClass = 'flex min-w-0 items-end gap-1.5';
const questInputFileTokenClass = 'inline-grid max-w-[220px] grid-cols-[14px_minmax(0,1fr)_16px] items-center gap-1.5 rounded-[6px] border border-[var(--border-light)] bg-[var(--bg-hover)] px-2 py-[5px] text-[11px] text-[var(--text-secondary)] [&_button]:grid [&_button]:h-4 [&_button]:w-4 [&_button]:cursor-pointer [&_button]:place-items-center [&_button]:rounded [&_button]:border-0 [&_button]:bg-transparent [&_button]:p-0 [&_button]:text-[var(--text-muted)] hover:[&_button]:text-[var(--text-primary)] [&_span]:overflow-hidden [&_span]:text-ellipsis [&_span]:whitespace-nowrap';
const questInputTextareaClass = 'max-h-[150px] min-h-[28px] min-w-0 flex-1 resize-none border-0 bg-transparent py-[5px] text-[12px] leading-[1.45] text-[var(--text-secondary)] outline-none placeholder:text-[var(--text-muted)]';
const questionCardClass = 'absolute bottom-[calc(100%+8px)] left-2 right-2 z-20 grid max-h-[320px] overflow-hidden rounded-[7px] border border-[var(--border-light)] bg-[var(--bg-surface)] text-[12px] text-[var(--text-primary)] shadow-[var(--shadow-lg)] [&_header]:flex [&_header]:items-center [&_header]:justify-between [&_header]:gap-2 [&_header]:border-b [&_header]:border-[var(--border)] [&_header]:px-3 [&_header]:py-2 [&_header_span]:inline-flex [&_header_span]:items-center [&_header_span]:gap-2 [&_header_span]:font-medium [&_section]:grid [&_section]:max-h-[230px] [&_section]:gap-3 [&_section]:overflow-auto [&_section]:px-3 [&_section]:py-3 [&_footer]:flex [&_footer]:items-center [&_footer]:justify-between [&_footer]:gap-2 [&_footer]:border-t [&_footer]:border-[var(--border)] [&_footer]:px-3 [&_footer]:py-2';
const questionOptionClass = (active: boolean) => cn(
  'grid w-full cursor-pointer grid-cols-[22px_minmax(0,1fr)] gap-2 rounded-[5px] border border-transparent bg-transparent px-2 py-[6px] text-left font-[inherit] text-[12px] text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] [&_b]:grid [&_b]:h-5 [&_b]:w-5 [&_b]:place-items-center [&_b]:rounded [&_b]:border [&_b]:border-[var(--border-light)] [&_b]:font-mono [&_b]:text-[10px] [&_b]:font-medium [&_strong]:block [&_strong]:font-medium [&_strong]:text-[var(--text-primary)] [&_small]:mt-0.5 [&_small]:block [&_small]:text-[11px] [&_small]:leading-[1.35] [&_small]:text-[var(--text-muted)]',
  active && 'border-[var(--border-light)] bg-[var(--bg-hover)] [&_b]:border-[var(--text-primary)] [&_b]:text-[var(--text-primary)]',
);
const questionAnswerSummaryClass = 'mx-auto mb-4 mt-3 grid w-[min(720px,calc(100%-44px))] gap-3 rounded-[7px] border border-[var(--border)] bg-[var(--bg-surface)] px-3 py-3 text-[12px] text-[var(--text-primary)] [&_header]:flex [&_header]:items-center [&_header]:gap-2 [&_header]:font-medium [&_section]:grid [&_section]:gap-3 [&_b]:block [&_b]:font-medium [&_p]:m-0 [&_p]:mt-1 [&_p]:text-[var(--text-muted)]';
const executionToggleClass = 'inline-flex h-8 items-center gap-[7px] rounded-[5px] border border-[var(--border-light)] bg-[var(--bg-surface)] px-[10px] text-[11px] font-medium text-[var(--text-secondary)] [&_input]:m-0 [&_input]:h-[13px] [&_input]:w-[13px]';
const markdownPreviewClass = [
  'min-h-0 flex-1 overflow-auto rounded-lg border border-[var(--border-light)] bg-[var(--bg-base)] px-[26px] py-[22px] text-[12px] leading-[1.7] text-[var(--text-primary)]',
  '[&_h1]:mb-[0.65em] [&_h1]:mt-0 [&_h1]:text-[22px] [&_h1]:font-semibold [&_h1]:leading-[1.25]',
  '[&_h2]:mb-[0.55em] [&_h2]:mt-[1.15em] [&_h2]:text-[16px] [&_h2]:font-semibold',
  '[&_h3]:mb-[0.45em] [&_h3]:mt-[1em] [&_h3]:text-[13px] [&_h3]:font-semibold',
  '[&_p]:my-[0.55em] [&_ul]:my-[0.55em] [&_ol]:my-[0.55em] [&_ul]:pl-[1.45em] [&_ol]:pl-[1.45em] [&_li]:my-[0.2em]',
  '[&_code:not(pre_code)]:rounded [&_code:not(pre_code)]:bg-[var(--bg-hover)] [&_code:not(pre_code)]:px-[5px] [&_code:not(pre_code)]:py-px [&_code:not(pre_code)]:font-mono [&_code:not(pre_code)]:text-[0.92em]',
  '[&_pre]:overflow-auto [&_pre]:rounded-md [&_pre]:bg-[var(--bg-hover)] [&_pre]:p-3 [&_pre]:text-[11px]',
  '[&_blockquote]:border-l-2 [&_blockquote]:border-[var(--border-light)] [&_blockquote]:pl-3 [&_blockquote]:text-[var(--text-secondary)]',
  '[&_table]:my-3 [&_table]:block [&_table]:max-w-full [&_table]:overflow-x-auto [&_table]:border-collapse',
  '[&_th]:border [&_th]:border-[var(--border)] [&_th]:bg-[var(--bg-hover)] [&_th]:px-2 [&_th]:py-1',
  '[&_td]:border [&_td]:border-[var(--border)] [&_td]:px-2 [&_td]:py-1',
].join(' ');

const questClasses = {
  shell: 'grid h-screen w-screen grid-rows-[48px_minmax(0,1fr)_24px] overflow-hidden bg-[var(--bg-base)] font-[Inter,var(--font-sans)] text-[var(--text-primary)]',
  globalHeader: 'grid grid-cols-[minmax(0,1fr)_auto] items-center border-b border-[var(--border)] bg-[var(--bg-overlay)] text-[var(--text-primary)]',
  brand: 'flex min-w-0 items-center gap-2 px-4 [&_span]:min-w-0 [&_span]:overflow-hidden [&_span]:text-ellipsis [&_span]:whitespace-nowrap [&_span]:text-[13px] [&_span]:font-semibold [&_span]:text-[var(--text-primary)] [&_strong]:shrink-0 [&_strong]:text-[11px] [&_strong]:font-medium [&_strong]:text-[var(--text-muted)] [&_svg]:shrink-0 [&_svg]:text-[var(--text-muted)]',
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
  liveActivityList: 'mt-4 w-[min(800px,calc(100vw-360px))] min-w-[min(800px,calc(100vw-360px))] border-l border-[var(--border)] py-1 pl-4 max-[900px]:w-[min(680px,calc(100vw-280px))] max-[900px]:min-w-0',
  liveActivityEntry: 'relative flex min-h-[22px] items-center gap-2 text-[11px] leading-none text-[var(--text-muted)] before:absolute before:-left-[19px] before:top-1/2 before:h-[7px] before:w-[7px] before:-translate-y-1/2 before:rounded-full before:border before:border-[var(--text-muted)] before:bg-[var(--bg-base)] [&_b]:font-medium [&_b]:text-[var(--text-secondary)] [&_span]:min-w-0 [&_span]:overflow-hidden [&_span]:text-ellipsis [&_span]:whitespace-nowrap [&_svg]:h-[13px] [&_svg]:w-[13px] [&_svg]:shrink-0',
  liveActivityEntryActive: 'before:border-[rgba(255,255,255,0.78)] [&_b]:bg-[linear-gradient(90deg,var(--text-secondary),#fff,var(--text-secondary))] [&_b]:bg-[length:220%_100%] [&_b]:bg-clip-text [&_b]:text-transparent [&_b]:[animation:quest-live-text-sheen_1.8s_linear_infinite] [&_span]:bg-[linear-gradient(90deg,var(--text-muted),#f8fafc,var(--text-muted))] [&_span]:bg-[length:220%_100%] [&_span]:bg-clip-text [&_span]:text-transparent [&_span]:[animation:quest-live-text-sheen_1.8s_linear_infinite]',
  workspace: 'grid min-h-0 min-w-0 overflow-hidden bg-[var(--bg-surface)]',
  cockpit: 'grid min-h-0 overflow-hidden bg-[var(--bg-surface)] max-[900px]:grid-cols-1 max-[900px]:grid-rows-[minmax(0,1fr)_minmax(320px,42vh)]',
  runStream: 'grid min-h-0 min-w-0 grid-rows-[minmax(0,1fr)_auto] bg-[var(--bg-base)] max-[900px]:border-b max-[900px]:border-r-0',
  paneResizeHandle: 'w-[6px] cursor-col-resize border-x border-[var(--border)] bg-[var(--bg-base)] transition-colors hover:bg-[var(--bg-hover)] active:bg-[var(--bg-hover)] focus:outline-none focus:ring-1 focus:ring-inset focus:ring-[var(--brand)] max-[900px]:hidden',
  paneResizeHandleActive: 'bg-[var(--bg-hover)]',
  streamPrompt: 'mx-auto mb-5 mt-4 w-[min(720px,calc(100%-44px))] rounded-[7px] border border-[var(--border)] bg-[var(--bg-hover)] px-3 py-[9px] text-[12px] leading-[1.45] text-[var(--text-primary)]',
  streamList: 'min-h-0 overflow-auto px-[22px] pb-[22px]',
  streamEntry: 'py-[3px] [&>div]:min-w-0 [&_button]:flex [&_button]:min-h-8 [&_button]:w-full [&_button]:cursor-default [&_button]:items-center [&_button]:gap-2 [&_button]:rounded-[5px] [&_button]:border-0 [&_button]:bg-transparent [&_button]:px-2 [&_button]:py-[6px] [&_button]:text-left [&_button]:font-[inherit] [&_button]:outline-none [&_button:enabled]:cursor-pointer [&_button:enabled:hover]:bg-[var(--bg-hover)] [&_pre]:mt-1 [&_pre]:max-h-[260px] [&_pre]:overflow-auto [&_pre]:rounded-[5px] [&_pre]:bg-[var(--bg-surface)] [&_pre]:p-[9px] [&_pre]:font-mono [&_pre]:text-[10px] [&_pre]:leading-[1.55] [&_pre]:text-[var(--text-secondary)] [&_small]:min-w-0 [&_small]:shrink-0 [&_small]:text-[11px] [&_small]:text-[var(--text-muted)] [&_strong]:min-w-0 [&_strong]:overflow-hidden [&_strong]:text-ellipsis [&_strong]:whitespace-nowrap [&_strong]:text-[12px] [&_strong]:font-medium [&_strong]:text-[var(--text-primary)]',
  streamBlock: 'rounded-[6px] px-1 py-1',
  streamBlockDebug: 'opacity-75',
  streamChildren: 'ml-4 mt-1 grid gap-1 border-l border-[var(--border)] pl-3',
  streamChild: 'grid gap-1 rounded-[5px] px-2 py-[6px] text-[11px] text-[var(--text-secondary)] [&_button]:min-h-0 [&_button]:px-0 [&_button]:py-0 [&_button:hover]:bg-transparent [&_small]:text-[10px] [&_strong]:text-[11px]',
  streamArrow: 'ml-auto h-[13px] w-[13px] shrink-0 text-[var(--text-muted)] opacity-0 transition-[opacity,transform] duration-150 group-hover:opacity-100',
  streamEvidence: 'ml-2 mr-1 pb-2',
  nextEntry: '[&_strong]:text-[var(--brand)]',
  liveEntry: 'rounded-[6px] [&_strong]:bg-[linear-gradient(90deg,var(--text-primary),#fff,var(--text-primary))] [&_strong]:bg-[length:220%_100%] [&_strong]:bg-clip-text [&_strong]:text-transparent [&_strong]:[animation:quest-live-text-sheen_1.8s_linear_infinite] [&_small]:bg-[linear-gradient(90deg,var(--text-muted),#f8fafc,var(--text-muted))] [&_small]:bg-[length:220%_100%] [&_small]:bg-clip-text [&_small]:text-transparent [&_small]:[animation:quest-live-text-sheen_1.8s_linear_infinite]',
  timelineDot: 'relative mt-4 h-[9px] w-[9px] rounded-full border border-[var(--text-muted)] bg-[var(--bg-surface)] after:absolute after:left-1 after:top-[10px] after:h-[calc(100%+34px)] after:w-px after:bg-[var(--border)] after:content-[""]',
  timelineDotLast: 'after:hidden',
  timelineDotNext: 'border-[var(--text-primary)]',
  timelineDotLive: 'border-[var(--brand)] bg-[var(--brand)] shadow-[0_0_0_3px_var(--brand-dim)] [animation:quest-live-pulse_1.7s_ease-out_infinite]',
  reviewChip: 'ml-6 mt-3 inline-flex w-max cursor-pointer items-center gap-[6px] rounded-md border border-[var(--border-light)] bg-[var(--bg-surface)] px-[10px] py-[7px] text-[12px] text-[var(--success)] [&_span]:text-[var(--danger)]',
  steerBar: 'relative mx-auto mb-4 grid w-[min(720px,calc(100%-44px))] grid-cols-[minmax(0,1fr)_34px] items-end gap-3 rounded-[7px] border border-[var(--border-light)] bg-[var(--bg-surface)] px-3 py-2 text-[11px] text-[var(--text-muted)] shadow-[var(--shadow-sm)]',
  composerToolbar: 'flex min-w-0 items-center gap-1 pt-px text-[10px] text-[var(--text-muted)]',
  sendButton: 'grid h-[30px] w-[30px] cursor-pointer place-items-center rounded-[5px] border border-[var(--brand)] bg-[var(--brand)] text-[var(--bg-base)] shadow-[0_0_0_1px_var(--brand-dim)] hover:enabled:border-[var(--brand-hover)] hover:enabled:bg-[var(--brand-hover)] disabled:cursor-default disabled:opacity-45 [&_svg]:stroke-[2.25]',
  rightPanel: 'grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)] bg-[var(--bg-surface)]',
  artifactHeader: 'grid gap-2 border-b border-[var(--border)] bg-[var(--bg-base)] px-3 py-3 [&_h2]:m-0 [&_h2]:text-[12px] [&_h2]:font-semibold [&_h2]:text-[var(--text-primary)] [&_p]:m-0 [&_p]:text-[10px] [&_p]:leading-[1.35] [&_p]:text-[var(--text-muted)]',
  panelTabs: 'flex min-w-0 gap-1 overflow-x-auto',
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
  groupButton: 'mb-0.5 grid w-full select-none cursor-pointer grid-cols-[8px_minmax(0,1fr)] items-start gap-[9px] rounded-[7px] border border-transparent bg-transparent px-[7px] py-2 text-left text-[var(--text-secondary)] hover:border-[var(--border-light)] hover:bg-[var(--bg-hover)] [&_div]:min-w-0 [&_em]:not-italic [&_em]:rounded-full [&_em]:bg-[var(--warning-dim)] [&_em]:px-1.5 [&_em]:py-[2px] [&_em]:text-[9px] [&_em]:font-medium [&_em]:text-[var(--warning)] [&_small]:block [&_small]:overflow-hidden [&_small]:select-none [&_small]:text-ellipsis [&_small]:whitespace-nowrap [&_small]:text-[10px] [&_small]:text-[var(--text-muted)] [&_strong]:mb-1 [&_strong]:block [&_strong]:overflow-hidden [&_strong]:select-none [&_strong]:text-ellipsis [&_strong]:whitespace-nowrap [&_strong]:text-[12px] [&_strong]:font-medium [&_strong]:text-[var(--text-primary)]',
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

const queueGroupOrder: QuestQueueGroup[] = ['needs_action', 'running', 'recent', 'archived'];

const queueGroupLabels: Record<QuestQueueGroup, string> = {
  needs_action: 'Needs action',
  running: 'Running',
  recent: 'Recent',
  archived: 'Archived',
};

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
  placement = 'bottom',
}: {
  value: string;
  options: QuestSelectOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  compact?: boolean;
  widthClass?: string;
  menuWidthClass?: string;
  align?: 'left' | 'right';
  placement?: 'top' | 'bottom';
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
          'flex min-w-0 cursor-pointer items-center justify-between rounded-[5px] border-0 bg-transparent text-left text-[11px] font-medium text-[var(--text-secondary)] outline-none hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] disabled:cursor-default disabled:opacity-45',
          compact ? 'h-[18px] gap-0.5 px-0.5 text-[9px]' : 'h-8 w-full gap-2 border border-[var(--border-light)] bg-[var(--bg-surface)] px-2',
        )}
        onClick={() => setOpen(current => !current)}
        disabled={disabled || options.length === 0}
      >
        <span className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap">
          {selectedOption?.label ?? 'No options'}
        </span>
        <IconChevronDown
          className={cn(
            'shrink-0 text-[var(--text-muted)]',
            open && placement === 'bottom' && 'rotate-180',
            !open && placement === 'top' && 'rotate-180',
          )}
          size={compact ? 8 : 13}
        />
      </button>
      {open && (
        <div className={cn(
          'absolute z-30 flex max-h-[240px] min-w-full flex-col overflow-auto rounded-[7px] border border-[var(--border-light)] bg-[var(--bg-elevated)] p-1 shadow-[var(--shadow-md)]',
          placement === 'top' ? 'bottom-[calc(100%+4px)]' : 'top-[calc(100%+4px)]',
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

function QuestModelThinkingDropdown({
  modelValue,
  thinkingValue,
  modelOptions,
  thinkingOptions,
  onModelChange,
  onThinkingChange,
  disabled,
  widthClass = 'w-full',
  menuWidthClass = 'w-[240px]',
  placement = 'bottom',
}: {
  modelValue: string;
  thinkingValue: string;
  modelOptions: QuestSelectOption[];
  thinkingOptions: QuestSelectOption[];
  onModelChange: (value: string) => void;
  onThinkingChange: (value: string) => void;
  disabled?: boolean;
  widthClass?: string;
  menuWidthClass?: string;
  placement?: 'top' | 'bottom';
}) {
  const [open, setOpen] = useState(false);
  const selectedModel = modelOptions.find(option => option.value === modelValue) ?? modelOptions[0];
  const selectedThinking = thinkingOptions.find(option => option.value === thinkingValue) ?? thinkingOptions[0];
  const compactThinkingLabel = (value: string, label: string): string => {
    if (value === 'off') return 'Off';
    if (value === 'low') return 'Low';
    if (value === 'medium') return 'Med';
    if (value === 'high') return 'High';
    return label;
  };

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
        className="flex h-[18px] min-w-0 cursor-pointer items-center justify-between gap-0.5 rounded-[5px] border-0 bg-transparent px-0.5 text-left text-[9px] font-medium text-[var(--text-secondary)] outline-none hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] disabled:cursor-default disabled:opacity-45"
        onClick={() => setOpen(current => !current)}
        disabled={disabled || modelOptions.length === 0}
        title={`${selectedModel?.label ?? 'No model'} / ${selectedThinking?.label ?? ''}`}
      >
        <span className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap">
          {selectedModel?.label ?? 'No model'}
        </span>
        <IconChevronDown
          className={cn(
            'shrink-0 text-[var(--text-muted)]',
            open && placement === 'bottom' && 'rotate-180',
            !open && placement === 'top' && 'rotate-180',
          )}
          size={8}
        />
      </button>
      {open && (
        <div className={cn(
          'absolute left-0 z-30 grid max-h-[280px] min-w-full overflow-hidden rounded-[7px] border border-[var(--border-light)] bg-[var(--bg-elevated)] shadow-[var(--shadow-md)]',
          placement === 'top' ? 'bottom-[calc(100%+4px)]' : 'top-[calc(100%+4px)]',
          menuWidthClass,
        )}>
          <div className="grid border-b border-[var(--border)] p-1">
            <div className="px-2 pb-1 pt-1 text-[9px] font-semibold uppercase text-[var(--text-muted)]">Thinking</div>
            <div className="grid grid-cols-4 gap-0.5 rounded-[5px] bg-[var(--bg-base)] p-0.5">
              {thinkingOptions.map(option => {
                const active = option.value === thinkingValue;
                return (
                  <button
                    type="button"
                    key={option.value}
                    className={cn(
                      'h-[24px] cursor-pointer rounded-[4px] border border-transparent bg-transparent px-1 text-[10px] font-semibold text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]',
                      active && '!border-[var(--brand)] !bg-[var(--brand)] !text-white hover:!border-[var(--brand-hover)] hover:!bg-[var(--brand-hover)] hover:!text-white',
                    )}
                    onMouseDown={event => event.preventDefault()}
                    onClick={() => onThinkingChange(option.value)}
                    aria-pressed={active}
                    title={option.label}
                  >
                    <span className="block overflow-hidden text-ellipsis whitespace-nowrap">
                      {compactThinkingLabel(option.value, option.label)}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>
          <div className="grid max-h-[210px] overflow-auto p-1">
            <div className="px-2 pb-1 pt-1 text-[9px] font-semibold uppercase text-[var(--text-muted)]">Model</div>
            {modelOptions.map(option => {
              const active = option.value === modelValue;
              return (
                <button
                  type="button"
                  key={option.value}
                  className={cn(
                    'grid min-w-0 cursor-pointer grid-cols-[minmax(0,1fr)_14px] items-center gap-2 rounded-[5px] border-0 bg-transparent px-2 py-[7px] text-left text-[11px] text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]',
                    active && 'bg-[var(--bg-active)] text-[var(--text-primary)]',
                  )}
                  onMouseDown={event => event.preventDefault()}
                  onClick={() => {
                    onModelChange(option.value);
                    setOpen(false);
                  }}
                >
                  <span className="min-w-0">
                    <span className="block overflow-hidden text-ellipsis whitespace-nowrap font-semibold">{option.label}</span>
                    {option.description && <small className="block overflow-hidden text-ellipsis whitespace-nowrap text-[10px] text-[var(--text-muted)]">{option.description}</small>}
                  </span>
                  {active && <IconCheck size={12} className="text-[var(--text-primary)]" />}
                </button>
              );
            })}
          </div>
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

function formatElapsed(startedAt: number, now: number): string {
  return `${Math.max(0, Math.round((now - startedAt) / 1000))}s`;
}

function compactStreamText(text: string): string {
  return text.replace(/\s+/g, ' ').trim().slice(0, 140);
}

function activeQuestInputTrigger(value: string): { mode: QuestInputSuggestionMode; query: string; start: number } | null {
  const match = value.match(/(^|\s)([@/])([^\s@/]*)$/);
  if (!match) return null;
  return {
    mode: match[2] === '@' ? 'file' : 'command',
    query: match[3] ?? '',
    start: value.length - (match[2].length + (match[3]?.length ?? 0)),
  };
}

function toolCallLabel(delta: string): string | null {
  try {
    const parsed = JSON.parse(delta) as { name?: string };
    return parsed.name ? `Tool call ${parsed.name}` : null;
  } catch {
    return null;
  }
}

function defaultPanelForQuest(detail: QuestDetail): QuestPanel {
  if (detail.status === 'draft' || detail.status === 'specified') return 'spec';
  if (detail.status === 'ready_for_review' && detail.review) return 'review';
  return 'overview';
}

function queueGroupForQuest(status: QuestStatus): QuestQueueGroup {
  if (['clarifying', 'waiting_for_user', 'blocked', 'ready_for_review', 'draft', 'specified'].includes(status)) {
    return 'needs_action';
  }
  if (['planning', 'prepared', 'running', 'validating', 'repairing', 'applying'].includes(status)) {
    return 'running';
  }
  if (['archived', 'canceled'].includes(status)) {
    return 'archived';
  }
  return 'recent';
}

function statusBadgeLabel(status: QuestStatus): string {
  if (status === 'ready_for_review') return 'Review';
  if (status === 'waiting_for_user' || status === 'clarifying') return 'Action Required';
  if (status === 'running' || status === 'validating' || status === 'repairing' || status === 'applying') return 'Running';
  if (status === 'blocked') return 'Blocked';
  if (status === 'completed') return 'Done';
  return status.replaceAll('_', ' ');
}

function progressItems(
  detail: QuestDetail,
  t: (key: string) => string,
): Array<{ title: string; status: 'done' | 'current' | 'pending' }> {
  const titles = [t('quest_progress_review_spec'), t('quest_progress_approve_execution'), t('quest_progress_review_evidence')];
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

function isInternalTaskEvent(kind: string): boolean {
  return kind === 'task_created' || kind === 'tasks_updated';
}

function isStatusEvent(kind: string): boolean {
  return kind === 'status_changed';
}

function isWorkspaceEvent(kind: string): boolean {
  return kind === 'checkpoint'
    || kind === 'alternative'
    || kind === 'knowledge_context_updated'
    || kind === 'execution_config_updated'
    || kind === 'branched'
    || kind === 'branch_created';
}

function isToolEvent(kind: string): boolean {
  return kind === 'file_read'
    || kind === 'file_edit'
    || kind === 'command'
    || kind === 'command_policy'
    || kind === 'tool_call';
}

function statusFromValidationEvent(event: QuestEvent): string {
  if (!event.details || typeof event.details !== 'object') return '';
  const status = (event.details as Record<string, unknown>).status;
  return typeof status === 'string' ? status : '';
}

function validationSummary(events: QuestEvent[]): string {
  const passed = events.filter(event => statusFromValidationEvent(event) === 'passed').length;
  const failed = events.filter(event => ['failed', 'error'].includes(statusFromValidationEvent(event))).length;
  const skipped = events.filter(event => statusFromValidationEvent(event) === 'skipped').length;
  const total = events.length;
  if (failed > 0) return `${failed}/${total} checks failed`;
  if (passed > 0 || skipped > 0) return `${passed}/${total} checks passed${skipped > 0 ? `, ${skipped} skipped` : ''}`;
  return total === 1 ? events[0].summary : `${total} validation checks recorded`;
}

function parseQuestionCardEvent(event: QuestEvent): QuestQuestionCard | null {
  if (event.kind !== 'question_card' || !event.details || typeof event.details !== 'object') return null;
  const details = event.details as Record<string, unknown>;
  const questionsValue = Array.isArray(details.questions) ? details.questions : [];
  const questions = questionsValue.map((item, index): QuestQuestion | null => {
    if (!item || typeof item !== 'object') return null;
    const value = item as Record<string, unknown>;
    const prompt = typeof value.prompt === 'string' ? value.prompt.trim() : '';
    if (!prompt) return null;
    const optionsValue = Array.isArray(value.options) ? value.options : [];
    const options = optionsValue.map((option, optionIndex): QuestQuestionOption | null => {
      if (!option || typeof option !== 'object') return null;
      const optionValue = option as Record<string, unknown>;
      const label = typeof optionValue.label === 'string' ? optionValue.label.trim() : '';
      if (!label) return null;
      return {
        id: typeof optionValue.id === 'string' && optionValue.id.trim()
          ? optionValue.id.trim()
          : String.fromCharCode(65 + Math.min(optionIndex, 25)),
        label,
        description: typeof optionValue.description === 'string' && optionValue.description.trim()
          ? optionValue.description.trim()
          : null,
      };
    }).filter((option): option is QuestQuestionOption => Boolean(option));
    return {
      id: typeof value.id === 'string' && value.id.trim() ? value.id.trim() : `q${index + 1}`,
      prompt,
      options,
      allow_multiple: typeof value.allow_multiple === 'boolean' ? value.allow_multiple : false,
      allow_custom: typeof value.allow_custom === 'boolean' ? value.allow_custom : options.length === 0,
    };
  }).filter((question): question is QuestQuestion => Boolean(question));
  if (questions.length === 0) return null;
  return {
    eventId: event.id,
    title: typeof details.title === 'string' && details.title.trim() ? details.title.trim() : event.summary || 'Questions',
    questions,
  };
}

function answerMessageFromQuestionCard(
  card: QuestQuestionCard,
  answers: Record<string, string[]>,
  customAnswers: Record<string, string>,
): string {
  return [
    `Answers for ${card.title}:`,
    ...card.questions.map(question => {
      const selectedLabels = (answers[question.id] ?? [])
        .map(optionId => question.options.find(option => option.id === optionId)?.label ?? optionId);
      const custom = customAnswers[question.id]?.trim();
      const values = [...selectedLabels, ...(custom ? [custom] : [])];
      return `- ${question.prompt}\n  ${values.length > 0 ? values.join('; ') : 'No answer'}`;
    }),
  ].join('\n');
}

function latestQuestionCard(events: QuestEvent[]): QuestQuestionCard | null {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const card = parseQuestionCardEvent(events[index]);
    if (card) return card;
  }
  return null;
}

function latestClarificationAnswer(events: QuestEvent[]): string | null {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index];
    if (event.kind !== 'clarification_answer') continue;
    if (event.details && typeof event.details === 'object') {
      const message = (event.details as Record<string, unknown>).message;
      if (typeof message === 'string' && message.trim()) return message.trim();
    }
    return event.summary;
  }
  return null;
}

function blockKindLabel(kind: QuestTimelineBlockKind): string {
  return kind.replaceAll('_', ' ');
}

function eventKindLabel(kind: string): string {
  return kind.replaceAll('_', ' ');
}

function providerEndpointConfigurable(provider: string): boolean {
  return provider === 'custom' || provider === 'ollama';
}

function providerRequiresEndpoint(provider: string): boolean {
  return provider === 'custom';
}

function makeTimelineBlock(
  kind: QuestTimelineBlockKind,
  title: string,
  summary: string,
  events: QuestEvent[],
  options: Partial<Pick<QuestTimelineBlock, 'defaultCollapsed' | 'importance'>> = {},
): QuestTimelineBlock {
  return {
    id: `${kind}-${events.map(event => event.id).join('-') || title}`,
    kind,
    title,
    summary,
    events,
    defaultCollapsed: options.defaultCollapsed ?? events.length > 1,
    importance: options.importance ?? 'primary',
  };
}

function buildQuestTimeline(events: QuestEvent[]): QuestTimelineBlock[] {
  const blocks: QuestTimelineBlock[] = [];
  const workspaceEvents: QuestEvent[] = [];
  const toolEvents: QuestEvent[] = [];
  const validationEvents: QuestEvent[] = [];
  const debugEvents: QuestEvent[] = [];

  const flushWorkspace = () => {
    if (workspaceEvents.length === 0) return;
    blocks.push(makeTimelineBlock(
      'workspace',
      'Workspace prepared',
      workspaceEvents.length === 1 ? workspaceEvents[0].summary : `${workspaceEvents.length} workspace events`,
      workspaceEvents.splice(0),
      { defaultCollapsed: true, importance: 'secondary' },
    ));
  };
  const flushTools = () => {
    if (toolEvents.length === 0) return;
    blocks.push(makeTimelineBlock(
      'tool_group',
      'Tool activity',
      toolEvents.length === 1 ? toolEvents[0].summary : `${toolEvents.length} operations`,
      toolEvents.splice(0),
      { defaultCollapsed: true, importance: 'secondary' },
    ));
  };
  const flushValidation = () => {
    if (validationEvents.length === 0) return;
    blocks.push(makeTimelineBlock(
      'validation',
      validationEvents.some(event => ['failed', 'error'].includes(statusFromValidationEvent(event))) ? 'Validation failed' : 'Validation passed',
      validationSummary(validationEvents),
      validationEvents.splice(0),
      { defaultCollapsed: true, importance: 'primary' },
    ));
  };
  const flushGrouped = () => {
    flushWorkspace();
    flushTools();
    flushValidation();
  };

  for (const event of events) {
    if (isInternalTaskEvent(event.kind)) continue;
    if (isStatusEvent(event.kind)) {
      debugEvents.push(event);
      continue;
    }
    if (isWorkspaceEvent(event.kind)) {
      workspaceEvents.push(event);
      continue;
    }
    if (isToolEvent(event.kind)) {
      toolEvents.push(event);
      continue;
    }
    if (event.kind === 'validation') {
      validationEvents.push(event);
      continue;
    }
    if (event.kind === 'question_card') {
      flushGrouped();
      continue;
    }

    flushGrouped();
    if (event.kind === 'created' || event.kind === 'spec_updated' || event.kind === 'intent_updated') {
      blocks.push(makeTimelineBlock('plan', event.summary, eventKindLabel(event.kind), [event], { defaultCollapsed: true }));
    } else if (event.kind === 'plan') {
      blocks.push(makeTimelineBlock('plan', event.summary, eventKindLabel(event.kind), [event], { defaultCollapsed: true }));
    } else if (event.kind === 'review_ready') {
      blocks.push(makeTimelineBlock('result', event.summary, eventKindLabel(event.kind), [event], { defaultCollapsed: false }));
    } else if (event.kind.includes('decision') || event.kind.startsWith('user_') || event.kind.includes('requested')) {
      blocks.push(makeTimelineBlock('decision', event.summary, eventKindLabel(event.kind), [event], { defaultCollapsed: false }));
    } else if (event.kind === 'thought' || event.kind === 'thinking') {
      blocks.push(makeTimelineBlock('thought', 'Thought', event.summary, [event], { defaultCollapsed: true, importance: 'secondary' }));
    } else {
      blocks.push(makeTimelineBlock('execution', event.summary, eventKindLabel(event.kind), [event], { defaultCollapsed: true }));
    }
  }

  flushGrouped();
  if (debugEvents.length > 0) {
    blocks.push(makeTimelineBlock(
      'debug',
      'Debug trace',
      `${debugEvents.length} internal events`,
      debugEvents,
      { defaultCollapsed: true, importance: 'debug' },
    ));
  }
  return blocks;
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

function summarizeSpecForDecision(spec: string, fallback: string): string {
  const line = spec
    .split('\n')
    .map(item => item.replace(/^#+\s*/, '').replace(/^[-*]\s*/, '').trim())
    .find(item => item.length > 0);
  if (!line) return fallback;
  return line.length > 220 ? `${line.slice(0, 217)}...` : line;
}

export default function QuestPage({
  currentProjectPath,
  initialQuestId,
  onOpenEditor,
  onCloseProject,
}: Props) {
  const { t, t_fmt } = useTranslation();
  const voiceInputRef = React.useRef<RealtimeTranscriptionHandle | null>(null);
  const questInputRef = React.useRef<HTMLTextAreaElement | null>(null);
  const specAskInputRef = React.useRef<HTMLTextAreaElement | null>(null);
  const cockpitRef = React.useRef<HTMLElement | null>(null);
  const [quests, setQuests] = useState<QuestRecord[]>([]);
  const [knowledge, setKnowledge] = useState<KnowledgeEntry[]>([]);
  const [selected, setSelected] = useState<QuestDetail | null>(null);
  const [panel, setPanel] = useState<QuestPanel>('overview');
  const [artifact, setArtifact] = useState<QuestArtifactSelection | null>(null);
  const [intentDraft, setIntentDraft] = useState('');
  const [specDraft, setSpecDraft] = useState('');
  const [specMode, setSpecMode] = useState<SpecMode>('preview');
  const [specSelection, setSpecSelection] = useState<SpecSelection | null>(null);
  const [specSelectionQuestion, setSpecSelectionQuestion] = useState('');
  const [goal, setGoal] = useState('');
  const [creatingQuestGoal, setCreatingQuestGoal] = useState<string | null>(null);
  const [questMode, setQuestMode] = useState<QuestMode>('solo');
  const [modelConfig, setModelConfig] = useState<QuestModelConfig>(defaultQuestModelConfig);
  const [modelOptions, setModelOptions] = useState<QuestModelOption[]>([]);
  const [voiceInputStatus, setVoiceInputStatus] = useState<VoiceInputStatus>('idle');
  const [canUseOpenAIVoiceInput, setCanUseOpenAIVoiceInput] = useState(false);
  const [rewritingPrompt, setRewritingPrompt] = useState(false);
  const rewriteRequestRef = React.useRef<QuestAiStreamHandle<{ prompt: string }> | null>(null);
  const [renaming, setRenaming] = useState(false);
  const [titleDraft, setTitleDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [executingQuestId, setExecutingQuestId] = useState<string | null>(null);
  const [liveQuestActivities, setLiveQuestActivities] = useState<LiveQuestActivity[]>([]);
  const [liveNow, setLiveNow] = useState(() => Date.now());
  const [selectedReviewFiles, setSelectedReviewFiles] = useState<Set<string>>(new Set());
  const [selectedReviewGroups, setSelectedReviewGroups] = useState<Set<string>>(new Set());
  const [eventExpansionOverrides, setEventExpansionOverrides] = useState<Map<string, boolean>>(new Map());
  const [questInput, setQuestInput] = useState('');
  const [questInputFileTokens, setQuestInputFileTokens] = useState<QuestInputFileToken[]>([]);
  const [questProjectFiles, setQuestProjectFiles] = useState<QuestProjectFile[]>([]);
  const [questInputSuggestionMode, setQuestInputSuggestionMode] = useState<QuestInputSuggestionMode | null>(null);
  const [questInputSuggestionIndex, setQuestInputSuggestionIndex] = useState(0);
  const [questionAnswers, setQuestionAnswers] = useState<Record<string, string[]>>({});
  const [questionCustomAnswers, setQuestionCustomAnswers] = useState<Record<string, string>>({});
  const [questionCardCollapsed, setQuestionCardCollapsed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [errorOpen, setErrorOpen] = useState(false);
  const [artifactPanePercent, setArtifactPanePercent] = useState(() => {
    const stored = window.localStorage.getItem(artifactPaneStorageKey);
    const parsed = stored ? Number(stored) : 36;
    return Number.isFinite(parsed) ? clampArtifactPanePercent(parsed) : 36;
  });
  const [isResizingArtifactPane, setIsResizingArtifactPane] = useState(false);
  const [isNarrowQuestLayout, setIsNarrowQuestLayout] = useState(false);
  const [artifactContent, setArtifactContent] = useState<string | null>(null);
  const [artifactContentKey, setArtifactContentKey] = useState<string | null>(null);

  const clearSelectedQuest = useCallback(() => {
    setSelected(null);
    setIntentDraft('');
    setSpecDraft('');
    setSpecMode('preview');
    setSpecSelection(null);
    setSpecSelectionQuestion('');
    setSelectedReviewFiles(new Set());
    setSelectedReviewGroups(new Set());
    setEventExpansionOverrides(new Map());
    setQuestInput('');
    setQuestInputFileTokens([]);
    setQuestionAnswers({});
    setQuestionCustomAnswers({});
    setQuestionCardCollapsed(false);
    setTitleDraft('');
    setArtifact(null);
    setArtifactContent(null);
    setArtifactContentKey(null);
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

  useEffect(() => {
    const path = artifact?.path;
    if (!selected || !path || artifact.kind === 'changed_file' || artifact.kind === 'trace') {
      setArtifactContent(null);
      setArtifactContentKey(null);
      return;
    }
    const key = `${selected.id}:${path}`;
    let cancelled = false;
    setArtifactContentKey(key);
    setArtifactContent(null);
    readQuestArtifact(selected.id, path)
      .then(result => {
        if (!cancelled) {
          setArtifactContent(result.content);
        }
      })
      .catch(reason => {
        if (!cancelled) {
          setArtifactContent(String(reason));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [artifact?.kind, artifact?.path, selected?.id]);

  const syncQuestDrafts = useCallback((detail: QuestDetail) => {
    const nextQuestionCardId = latestQuestionCard(detail.events)?.eventId ?? null;
    setSelected(detail);
    setIntentDraft(detail.intent);
    setSpecDraft(detail.spec);
    setSpecMode('preview');
    setSpecSelection(null);
    setSpecSelectionQuestion('');
    setQuestMode(detail.mode);
    setModelConfig(detail.model_config ?? defaultQuestModelConfig);
    setTitleDraft(detail.title);
    setQuestionAnswers(current => {
      if (nextQuestionCardId && current.__card_id === nextQuestionCardId) return current;
      return nextQuestionCardId ? { __card_id: [nextQuestionCardId] } : {};
    });
    setQuestionCustomAnswers({});
    setQuestionCardCollapsed(false);
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
      setSpecMode('preview');
      setSpecSelection(null);
      setSpecSelectionQuestion('');
      setQuestMode('solo');
      setModelConfig(defaultQuestModelConfig);
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
    const media = window.matchMedia('(max-width: 900px)');
    const update = () => setIsNarrowQuestLayout(media.matches);
    update();
    media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, []);

  useEffect(() => {
    if (!currentProjectPath) return;
    listQuestProjectFiles()
      .then(files => setQuestProjectFiles(files))
      .catch(() => setQuestProjectFiles([]));
  }, [currentProjectPath]);

  useEffect(() => {
    if (!busy && !executingQuestId && liveQuestActivities.length === 0) return;
    const timer = window.setInterval(() => setLiveNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [busy, executingQuestId, liveQuestActivities.length]);

  useEffect(() => {
    if (!executingQuestId) return;
    const timer = window.setInterval(() => {
      getQuest(executingQuestId)
        .then(detail => {
          setSelected(current => current?.id === detail.id ? detail : current);
          setIntentDraft(detail.intent);
          setSpecDraft(detail.spec);
          setTitleDraft(detail.title);
          resetReviewSelection(detail);
        })
        .catch(() => {});
    }, 1500);
    return () => window.clearInterval(timer);
  }, [executingQuestId, resetReviewSelection]);

  useEffect(() => {
    let canceled = false;
    async function loadQuestModels() {
      const settings = await rpc<CopilotSettingsFull>('app/get_copilot_settings').catch(() => null);
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
        setModelConfig(prev => {
          const selectedModel = models.find(model => model.id === (prev.model || preferred));
          return {
            ...prev,
            provider: selectedModel?.provider ?? provider ?? prev.provider,
            model: prev.model || preferred,
            api_endpoint: prev.api_endpoint ?? settings?.api_endpoint ?? null,
            max_tokens: selectedModel?.default_max_tokens ?? settings?.max_tokens ?? prev.max_tokens,
          };
        });
      }
    }
    loadQuestModels().catch(reportError);
    return () => {
      canceled = true;
    };
  }, [reportError]);

  const visibleQuests = useMemo(() => {
    return quests.reduce<Record<QuestQueueGroup, QuestRecord[]>>((groups, quest) => {
      groups[queueGroupForQuest(quest.status)].push(quest);
      return groups;
    }, {
      needs_action: [],
      running: [],
      recent: [],
      archived: [],
    });
  }, [quests]);

  const effectiveProjectPath = useMemo(
    () => currentProjectPath ?? selected?.project.path ?? quests[0]?.project.path ?? null,
    [currentProjectPath, quests, selected?.project.path],
  );

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
  const syncQuestModelPreference = useCallback(async (next: QuestModelConfig) => {
    if (!next.model.trim()) return;
    const settings = await rpc<CopilotSettingsFull>('app/get_copilot_settings').catch(() => null);
    if (!settings) return;
    const endpoint = next.api_endpoint ?? settings.api_endpoint ?? null;
    const canSyncProvider = !providerRequiresEndpoint(next.provider) || Boolean(endpoint);
    const { api_key: _apiKey, ...payload } = settings;
    await rpc('app/update_copilot_settings', {
      ...payload,
      provider: canSyncProvider ? next.provider : settings.provider,
      model: next.model,
      api_endpoint: providerEndpointConfigurable(canSyncProvider ? next.provider : settings.provider)
        ? endpoint
        : settings.api_endpoint,
      max_tokens: next.max_tokens,
    });
  }, []);
  const chooseQuestModel = useCallback((value: string) => {
    const model = modelOptions.find(option => option.id === value);
    const next = {
      ...modelConfig,
      model: value,
      provider: model?.provider ?? modelConfig.provider,
      api_endpoint: providerEndpointConfigurable(model?.provider ?? modelConfig.provider)
        ? modelConfig.api_endpoint
        : null,
      max_tokens: model?.default_max_tokens ?? modelConfig.max_tokens,
    };
    setModelConfig(next);
    syncQuestModelPreference(next).catch(reportError);
  }, [modelConfig, modelOptions, reportError, syncQuestModelPreference]);
  const resolveQuestModelConfig = useCallback(async (): Promise<QuestModelConfig> => {
    const selectedModel = modelOptions.find(model => model.id === modelConfig.model);
    const provider = selectedModel?.provider ?? modelConfig.provider;
    const settings = await rpc<CopilotSettingsFull>('app/get_copilot_settings').catch(() => null);
    const endpoint = providerEndpointConfigurable(provider)
      ? modelConfig.api_endpoint ?? settings?.api_endpoint ?? null
      : null;
    return {
      ...modelConfig,
      provider,
      api_endpoint: endpoint,
    };
  }, [modelConfig, modelOptions]);
  const timelineBlocks = useMemo(
    () => buildQuestTimeline(selected?.events ?? []),
    [selected?.events],
  );
  const activeQuestionCard = useMemo(
    () => latestQuestionCard(selected?.events ?? []),
    [selected?.events],
  );
  const latestQuestionAnswer = useMemo(
    () => latestClarificationAnswer(selected?.events ?? []),
    [selected?.events],
  );
  const activeGoal = selected?.goal ?? creatingQuestGoal ?? '';
  const activeStatus = selected?.status ?? (creatingQuestGoal ? 'draft' : null);
  const activeSpec = selected ? (specDraft || selected.spec) : '';
  const activeSpecPath = selected?.spec_path ?? (creatingQuestGoal ? 'spec.md' : null);

  const startArtifactPaneResize = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    const cockpit = cockpitRef.current;
    if (!cockpit) return;
    event.preventDefault();
    const pointerId = event.pointerId;
    const originalCursor = document.body.style.cursor;
    const originalUserSelect = document.body.style.userSelect;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
    setIsResizingArtifactPane(true);
    event.currentTarget.setPointerCapture(pointerId);
    const updateWidth = (clientX: number) => {
      const rect = cockpit.getBoundingClientRect();
      if (rect.width <= 0) return;
      const rightWidth = rect.right - clientX;
      const percent = clampArtifactPanePercent((rightWidth / rect.width) * 100);
      setArtifactPanePercent(percent);
      window.localStorage.setItem(artifactPaneStorageKey, percent.toFixed(2));
    };
    updateWidth(event.clientX);
    const handleMove = (moveEvent: PointerEvent) => updateWidth(moveEvent.clientX);
    const cleanup = () => {
      document.body.style.cursor = originalCursor;
      document.body.style.userSelect = originalUserSelect;
      setIsResizingArtifactPane(false);
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', cleanup);
      window.removeEventListener('pointercancel', cleanup);
      try {
        event.currentTarget.releasePointerCapture(pointerId);
      } catch {
        // The pointer may already be released by the browser.
      }
    };
    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', cleanup);
    window.addEventListener('pointercancel', cleanup);
  }, []);

  const nudgeArtifactPane = useCallback((delta: number) => {
    setArtifactPanePercent(current => {
      const next = clampArtifactPanePercent(current + delta);
      window.localStorage.setItem(artifactPaneStorageKey, next.toFixed(2));
      return next;
    });
  }, []);

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

  const beginNewQuest = useCallback(async () => {
    if (!effectiveProjectPath) return;
    setError(null);
    try {
      if (!currentProjectPath || currentProjectPath !== effectiveProjectPath) {
        await rpc('hub/open_project', { path: effectiveProjectPath });
      }
      setSelected(null);
    } catch (reason) {
      reportError(reason);
    }
  }, [currentProjectPath, effectiveProjectPath, reportError]);

  const create = useCallback(async () => {
    if (!goal.trim() || rewritingPrompt) return;
    const submittedGoal = goal.trim();
    setBusy(true);
    setError(null);
    setCreatingQuestGoal(submittedGoal);
    setPanel('spec');
    setArtifact(null);
    const startedAt = Date.now();
    setLiveNow(startedAt);
    setLiveQuestActivities([{
      id: 'quest-create-start',
      kind: 'status',
      label: 'Planning Quest',
      detail: submittedGoal,
      startedAt,
    }]);
    try {
      if (effectiveProjectPath && (!currentProjectPath || currentProjectPath !== effectiveProjectPath)) {
        await rpc('hub/open_project', { path: effectiveProjectPath });
      }
      const resolvedModelConfig = await resolveQuestModelConfig();
      const detail = await createQuest('', submittedGoal, {
        mode: questMode,
        model_config: resolvedModelConfig,
      }, (delta, kind) => {
        const now = Date.now();
        setLiveNow(now);
        setLiveQuestActivities(prev => {
          if (kind === 'tool_call') {
            const label = toolCallLabel(delta);
            if (!label) return prev;
            return [...prev, {
              id: `tool-${now}-${prev.length}`,
              kind,
              label,
              startedAt: now,
            }].slice(-8);
          }
          const detail = compactStreamText(delta);
          if (!detail) return prev;
          const last = prev[prev.length - 1];
          if (last?.kind === kind) {
            return [
              ...prev.slice(0, -1),
              { ...last, detail: compactStreamText(`${last.detail ?? ''} ${detail}`) },
            ];
          }
          return [...prev, {
            id: `${kind}-${now}-${prev.length}`,
            kind,
            label: kind === 'thinking' ? 'Thought' : 'Drafting',
            detail,
            startedAt: now,
          }].slice(-8);
        });
      });
      setGoal('');
      setCreatingQuestGoal(null);
      syncQuestDrafts(detail);
      setArtifact(null);
      setPanel('spec');
      resetReviewSelection(detail);
      await refreshList(detail.id);
      await refreshKnowledge();
    } catch (reason) {
      setCreatingQuestGoal(null);
      setGoal(submittedGoal);
      reportError(reason);
    } finally {
      setLiveQuestActivities([]);
      setBusy(false);
    }
  }, [currentProjectPath, effectiveProjectPath, goal, questMode, refreshList, refreshKnowledge, reportError, resetReviewSelection, resolveQuestModelConfig, rewritingPrompt, syncQuestDrafts]);

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
      const resolvedModelConfig = await resolveQuestModelConfig();
      const request = rewriteQuestPrompt(
        goal.trim(),
        resolvedModelConfig,
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
  }, [goal, reportError, resolveQuestModelConfig, rewritingPrompt]);

  useEffect(() => () => {
    rewriteRequestRef.current?.cancel();
  }, []);

  const startOpenAIVoiceInput = useCallback(async () => {
    if (voiceInputStatus !== 'idle') {
      stopVoiceInput();
      return;
    }
    if (!canUseOpenAIVoiceInput || !effectiveProjectPath) return;
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
  }, [canUseOpenAIVoiceInput, effectiveProjectPath, reportError, stopVoiceInput, voiceInputStatus]);

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

  const execute = useCallback(async () => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    setExecutingQuestId(selected.id);
    setLiveQuestActivities([{
      id: 'quest-execute-start',
      kind: 'status',
      label: t('quest_execution_running'),
      detail: t('quest_execution_running_desc'),
      startedAt: Date.now(),
    }]);
    try {
      const detail = await executeQuest(selected.id);
      syncQuestDrafts(detail);
      setPanel(detail.review ? 'review' : panel);
      resetReviewSelection(detail);
      await refreshList(detail.id);
      await refreshKnowledge();
    } catch (reason) {
      reportError(reason);
      await refreshList(selected.id);
    } finally {
      setExecutingQuestId(null);
      setLiveQuestActivities([]);
      setBusy(false);
    }
  }, [panel, refreshList, refreshKnowledge, reportError, resetReviewSelection, selected, syncQuestDrafts, t]);

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

  const askAiAboutSpecSelection = useCallback(() => {
    if (!specSelection?.text.trim()) return;
    setSpecSelection(current => current ? { ...current, mode: 'input' } : current);
    setSpecSelectionQuestion('');
    requestAnimationFrame(() => {
      specAskInputRef.current?.focus();
    });
  }, [specSelection]);

  const submitSpecSelectionQuestion = useCallback(async () => {
    if (!selected || !specSelection?.text.trim() || !specSelectionQuestion.trim()) return;
    const message = [
      specSelectionQuestion.trim(),
      '',
      'Selected spec context:',
      specSelection.text.trim(),
    ].join('\n');
    setBusy(true);
    setError(null);
    try {
      const detail = await addQuestNote(selected.id, 'steer', message);
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      setSpecSelection(null);
      setSpecSelectionQuestion('');
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, reportError, selected, specSelection, specSelectionQuestion]);

  const captureSpecTextareaSelection = useCallback((target: HTMLTextAreaElement) => {
    const text = target.value.slice(target.selectionStart, target.selectionEnd).trim();
    if (!text) {
      setSpecSelection(null);
      return;
    }
    setSpecSelection({
      text,
      top: 66,
      left: 18,
      mode: 'button',
    });
    setSpecSelectionQuestion('');
  }, []);

  const captureSpecPreviewSelection = useCallback((container: HTMLDivElement) => {
    const selection = window.getSelection();
    const text = selection?.toString().trim() ?? '';
    if (!selection || selection.rangeCount === 0 || !text) {
      setSpecSelection(null);
      return;
    }
    const range = selection.getRangeAt(0);
    if (!container.contains(range.commonAncestorContainer)) {
      setSpecSelection(null);
      return;
    }
    const rect = range.getBoundingClientRect();
    const containerRect = container.getBoundingClientRect();
    setSpecSelection({
      text,
      top: Math.max(12, rect.top - containerRect.top - 42),
      left: Math.min(Math.max(12, rect.left - containerRect.left), Math.max(12, containerRect.width - 112)),
      mode: 'button',
    });
    setSpecSelectionQuestion('');
  }, []);

  const renderLiveActivities = useCallback((compact = false) => {
    if (liveQuestActivities.length === 0) return null;
    return (
      <div className={compact ? 'mt-2 border-l border-[var(--border)] py-1 pl-4' : questClasses.liveActivityList}>
        {liveQuestActivities.map((activity, index) => (
          <div
            key={activity.id}
            className={cn(
              questClasses.liveActivityEntry,
              index === liveQuestActivities.length - 1 && questClasses.liveActivityEntryActive,
            )}
          >
            {activity.kind === 'tool_call' ? <IconCode /> : activity.kind === 'file' ? <IconFile /> : null}
            <b>{activity.label}</b>
            <span>{activity.detail}</span>
            <time>{formatElapsed(activity.startedAt, liveNow)}</time>
          </div>
        ))}
      </div>
    );
  }, [liveNow, liveQuestActivities]);

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
    const message = [
      ...questInputFileTokens.map(token => `@${token.path}`),
      questInput.trim(),
    ].filter(Boolean).join(' ');
    if (!selected || !message) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await addQuestNote(selected.id, 'steer', message);
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      setQuestInput('');
      setQuestInputFileTokens([]);
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [questInput, questInputFileTokens, refreshList, reportError, selected]);

  const toggleQuestionOption = useCallback((question: QuestQuestion, optionId: string) => {
    setQuestionAnswers(current => {
      const currentValues = current[question.id] ?? [];
      const selected = currentValues.includes(optionId);
      return {
        ...current,
        [question.id]: question.allow_multiple
          ? selected
            ? currentValues.filter(value => value !== optionId)
            : [...currentValues, optionId]
          : selected
            ? []
            : [optionId],
      };
    });
  }, []);

  const submitQuestionCard = useCallback(async () => {
    if (!selected || !activeQuestionCard) return;
    const hasAnswer = activeQuestionCard.questions.some(question => {
      return (questionAnswers[question.id] ?? []).length > 0
        || Boolean(questionCustomAnswers[question.id]?.trim());
    });
    if (!hasAnswer) return;
    setBusy(true);
    setError(null);
    try {
      const message = answerMessageFromQuestionCard(
        activeQuestionCard,
        questionAnswers,
        questionCustomAnswers,
      );
      const detail = await addQuestNote(selected.id, 'clarify', message);
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      setQuestionAnswers({});
      setQuestionCustomAnswers({});
      setQuestionCardCollapsed(false);
      await refreshList(detail.id);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [activeQuestionCard, questionAnswers, questionCustomAnswers, refreshList, reportError, selected]);

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

  const questInputCommands = useMemo<QuestInputSuggestion[]>(() => {
    if (!selected) return [];
    return [
      {
        id: 'execute',
        label: 'Run Quest',
        detail: 'Execute the current spec',
        value: '/run',
        mode: 'command',
        run: execute,
      },
      {
        id: 'continue',
        label: 'Continue',
        detail: 'Resume from current evidence',
        value: '/continue',
        mode: 'command',
        run: () => continueSelectedQuest(),
      },
      {
        id: 'review',
        label: 'Open review',
        detail: 'Show changed files and validation evidence',
        value: '/review',
        mode: 'command',
        run: () => setPanel('review'),
      },
      {
        id: 'spec',
        label: 'Open spec',
        detail: 'Show the Quest specification',
        value: '/spec',
        mode: 'command',
        run: () => setPanel('spec'),
      },
      {
        id: 'cancel',
        label: 'Cancel Quest',
        detail: 'Mark the Quest as canceled',
        value: '/cancel',
        mode: 'command',
        run: cancelSelectedQuest,
      },
      {
        id: 'reopen',
        label: 'Reopen Quest',
        detail: 'Return an archived or canceled Quest to editing',
        value: '/reopen',
        mode: 'command',
        run: reopenSelectedQuest,
      },
    ];
  }, [cancelSelectedQuest, continueSelectedQuest, execute, reopenSelectedQuest, selected]);

  const questInputTrigger = useMemo(() => activeQuestInputTrigger(questInput), [questInput]);
  const questInputSuggestions = useMemo<QuestInputSuggestion[]>(() => {
    if (!questInputTrigger) return [];
    const query = questInputTrigger.query.toLowerCase();
    if (questInputTrigger.mode === 'file') {
      return questProjectFiles
        .filter(file => file.path.toLowerCase().includes(query))
        .slice(0, 8)
        .map(file => ({
          id: `file:${file.path}`,
          label: file.path.split('/').pop() ?? file.path,
          detail: file.path,
          value: `@${file.path}`,
          mode: 'file',
        }));
    }
    return questInputCommands
      .filter(command => command.value.slice(1).includes(query) || command.label.toLowerCase().includes(query))
      .slice(0, 8);
  }, [questInputCommands, questInputTrigger, questProjectFiles]);

  useEffect(() => {
    setQuestInputSuggestionIndex(0);
    setQuestInputSuggestionMode(questInputTrigger?.mode ?? null);
  }, [questInputTrigger?.mode, questInputTrigger?.query]);

  useEffect(() => {
    const input = questInputRef.current;
    if (!input) return;
    input.style.height = '30px';
    input.style.height = `${Math.min(150, Math.max(30, input.scrollHeight))}px`;
  }, [questInput]);

  const chooseQuestInputSuggestion = useCallback(async (suggestion: QuestInputSuggestion) => {
    if (!questInputTrigger) return;
    if (suggestion.mode === 'command' && suggestion.run) {
      setQuestInput('');
      setQuestInputSuggestionMode(null);
      await suggestion.run();
      return;
    }
    const prefix = questInput.slice(0, questInputTrigger.start);
    const path = suggestion.value.slice(1);
    setQuestInput(prefix.trimEnd());
    setQuestInputFileTokens(tokens => tokens.some(token => token.path === path)
      ? tokens
      : [...tokens, { path, kind: suggestion.detail === path ? 'Asset' : suggestion.detail }]);
    setQuestInputSuggestionMode(null);
  }, [questInput, questInputTrigger]);

  return (
    <div className={questClasses.shell}>
      <header className={questClasses.globalHeader}>
        <div className={questClasses.brand}>
          <span title={selected?.title ?? 'Varg'}>{selected?.title ?? 'Varg'}</span>
          <strong>{selected?.project.name ?? t('quest_title')}</strong>
          {selected && <IconMonitor size={13} />}
        </div>
        <div className={questClasses.globalActions}>
          {selected ? (
            <button
              className={buttonBase}
              onClick={() => onOpenEditor(
                selected.project.path,
                artifactFor('intent', t('quest_artifact_intent'), selected.intent_path),
              )}
            >
              <IconCode /> {t('quest_open_editor')}
            </button>
          ) : (
            <button className={buttonBase} onClick={onCloseProject} title={t('quest_close_project')}><IconX /></button>
          )}
        </div>
      </header>

      <div className={questClasses.layout}>
        <aside className={questClasses.sidebar}>
          <div className={questClasses.sidebarHeading}>
            <button className={questClasses.newButton} onClick={beginNewQuest} disabled={!effectiveProjectPath}>
              <IconPlus /> {t('quest_new')} <kbd>Ctrl N</kbd>
            </button>
          </div>

          {queueGroupOrder.map(group => (
            <QuestGroup
              key={group}
              label={queueGroupLabels[group]}
              quests={visibleQuests[group]}
              selectedId={selected?.id}
              onSelect={selectQuest}
              onMenuAction={runQuestMenuAction}
            />
          ))}
          <div className={questClasses.sidebarFooter}>
            <button onClick={() => setPanel('knowledge')}>{t('quest_knowledge')} <b>{knowledge.filter(entry => entry.status === 'pending').length}</b></button>
            <button disabled>{t('quest_marketplace')}</button>
          </div>
        </aside>

        {!selected && !creatingQuestGoal ? (
          <main className={questClasses.home}>
            <div className={questClasses.orb}><IconSparkles size={28} /></div>
            <h1 className="m-0 mb-3 text-[clamp(28px,3vw,40px)] font-[650] leading-[1.1] text-[var(--text-primary)]">{t('quest_home_title')}</h1>
            <div className={questClasses.startLine}>
              <span>{t('quest_start_in')}</span>
              <b>{effectiveProjectPath ? 'Varg' : t('quest_no_project')}</b>
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
                    if (!busy && !rewritingPrompt && goal.trim() && effectiveProjectPath) {
                      create();
                    }
                  }
                }}
                placeholder={t('quest_goal_placeholder')}
                disabled={!effectiveProjectPath}
              />
              <footer>
                <div className="flex min-w-0 flex-1 items-center gap-0 text-[12px] text-[var(--text-secondary)]">
                  <QuestDropdown
                    value={questMode}
                    options={[
                      { value: 'solo', label: t('quest_mode_solo') },
                      { value: 'extra', label: t('quest_mode_extra') },
                    ]}
                    onChange={value => setQuestMode(value as QuestMode)}
                    disabled={busy}
                    compact
                    widthClass="w-auto shrink-0"
                    menuWidthClass="w-[120px]"
                  />
                  <QuestModelThinkingDropdown
                    modelValue={modelConfig.model}
                    thinkingValue={modelConfig.thinking_effort}
                    modelOptions={modelSelectOptions}
                    thinkingOptions={thinkingOptions}
                    onModelChange={chooseQuestModel}
                    onThinkingChange={value => setModelConfig(prev => ({ ...prev, thinking_effort: value }))}
                    disabled={busy}
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
                    disabled={!canUseOpenAIVoiceInput || voiceInputStatus === 'connecting' || !effectiveProjectPath}
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
                <button className={questClasses.promptSubmit} onClick={create} disabled={busy || rewritingPrompt || !goal.trim() || !effectiveProjectPath} title={t('quest_create')}>
                  {busy ? <QuestLoader /> : <IconSend />}
                </button>
                </div>
              </footer>
            </div>
            {renderLiveActivities()}
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
            <section
              ref={cockpitRef}
              className={questClasses.cockpit}
              style={isNarrowQuestLayout ? undefined : {
                gridTemplateColumns: `minmax(360px,1fr) 6px minmax(280px,${artifactPanePercent}%)`,
              }}
            >
              <div className={questClasses.runStream}>
                <div className={questClasses.streamList}>
                  <div className={questClasses.streamPrompt}>{activeGoal}</div>
                  {latestQuestionAnswer && activeQuestionCard && (
                    <div className={questionAnswerSummaryClass}>
                      <header><IconMessageSquare /> Questions Answers</header>
                      <section>
                        {activeQuestionCard.questions.map(question => {
                          const marker = `- ${question.prompt}\n  `;
                          const answer = latestQuestionAnswer.includes(marker)
                            ? latestQuestionAnswer.split(marker)[1]?.split('\n- ')[0]?.trim()
                            : null;
                          return (
                            <div key={question.id}>
                              <b>{question.prompt}</b>
                              <p>{answer || 'Answered'}</p>
                            </div>
                          );
                        })}
                      </section>
                    </div>
                  )}
                  {timelineBlocks.map(block => {
                    const hasDetails = block.events.length > 0;
                    const expanded = eventExpansionOverrides.get(block.id) ?? !block.defaultCollapsed;
                    return (
                      <article
                        key={block.id}
                        className={cn(
                          questClasses.streamEntry,
                          questClasses.streamBlock,
                          block.importance === 'debug' && questClasses.streamBlockDebug,
                        )}
                      >
                        <div>
                          <button
                            className="group"
                            type="button"
                            disabled={!hasDetails}
                            onClick={() => {
                              if (!hasDetails) return;
                              setEventExpansionOverrides(overrides => {
                                const next = new Map(overrides);
                                next.set(block.id, !expanded);
                                return next;
                              });
                            }}
                          >
                            <strong>{block.title}</strong>
                            <small>{block.summary || blockKindLabel(block.kind)}</small>
                            {hasDetails && (
                              <IconChevronRight className={cn(questClasses.streamArrow, expanded && 'rotate-90 opacity-100')} />
                            )}
                          </button>
                          {hasDetails && expanded && (
                            <div className={questClasses.streamChildren}>
                              {block.events.map(event => {
                                const eventHasDetails = hasEventDetails(event.details);
                                return (
                                  <div key={event.id} className={questClasses.streamChild}>
                                    <button type="button" disabled>
                                      <strong>{event.summary}</strong>
                                      <small>{eventKindLabel(event.kind)}</small>
                                    </button>
                                    {eventHasDetails && (
                                      <pre className={questClasses.streamEvidence}>{formatEventDetails(event.details)}</pre>
                                    )}
                                  </div>
                                );
                              })}
                            </div>
                          )}
                        </div>
                      </article>
                    );
                  })}
                  <article className={cn(questClasses.streamEntry, questClasses.nextEntry)}>
                    <div>
                      <button type="button" disabled>
                        <strong>{selected?.next_action.label ?? 'Generating Quest spec'}</strong>
                        <small>{selected?.next_action.reason ?? 'Varg is preparing the editable task artifact.'}</small>
                      </button>
                    </div>
                  </article>
                  {activeStatus && ['draft', 'specified'].includes(activeStatus) && (
                    <div className={feedDecisionCardClass}>
                      <header>
                        <span>
                          <IconCode />
                          <b>Spec</b>
                          <strong>{activeSpecPath?.split('/').pop() ?? 'Spec.md'}</strong>
                        </span>
                      </header>
                      <section>
                        <p>{selected
                          ? summarizeSpecForDecision(activeSpec, selected.next_action.reason || t('quest_spec_ready_desc'))
                          : 'Generating an editable Quest spec from your prompt.'}</p>
                      </section>
                      <footer>
                        <button
                          type="button"
                          className="border-transparent bg-transparent text-[var(--text-muted)] hover:text-[var(--text-primary)]"
                          onClick={() => { setArtifact(null); setPanel('spec'); }}
                        >
                          View detail
                        </button>
                        <button
                          type="button"
                          className="border-[var(--text-primary)] bg-[var(--text-primary)] text-[var(--bg-base)]"
                          onClick={execute}
                          disabled={busy || !selected || selected.status === 'archived'}
                          title="Build"
                        >
                          {busy ? <QuestLoader /> : <IconPlay />} Build
                        </button>
                      </footer>
                    </div>
                  )}
                  {(creatingQuestGoal || (selected && executingQuestId === selected.id)) && (
                    <article className={cn(questClasses.streamEntry, questClasses.liveEntry)}>
                      <div>
                        <button type="button" disabled>
                          <strong>{creatingQuestGoal ? 'Generating Quest spec' : t('quest_execution_running')}</strong>
                          <small>{creatingQuestGoal ? 'waiting for model output and task artifact' : t('quest_execution_running_desc')}</small>
                        </button>
                        {renderLiveActivities(true)}
                      </div>
                    </article>
                  )}
                  {selected?.review && (
                    <button className={questClasses.reviewChip} onClick={() => { setArtifact(null); setPanel('review'); }}>
                      <IconCheck /> {t('quest_tab_review')} +{selected.review.changed_files.reduce((sum, file) => sum + file.additions, 0)}
                      <span>-{selected.review.changed_files.reduce((sum, file) => sum + file.deletions, 0)}</span>
                    </button>
                  )}
                </div>
                <div className={questClasses.steerBar}>
                  {activeQuestionCard && !latestQuestionAnswer && selected && ['clarifying', 'waiting_for_user'].includes(selected.status) && (
                    <div className={questionCardClass}>
                      <header>
                        <span><IconMessageSquare /> {activeQuestionCard.title}</span>
                        <button
                          type="button"
                          className="grid h-6 w-6 cursor-pointer place-items-center rounded border-0 bg-transparent text-[var(--text-muted)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
                          onClick={() => setQuestionCardCollapsed(value => !value)}
                          title={questionCardCollapsed ? 'Expand questions' : 'Collapse questions'}
                        >
                          <IconChevronDown className={questionCardCollapsed ? '' : 'rotate-180'} />
                        </button>
                      </header>
                      {!questionCardCollapsed && (
                        <>
                          <section>
                            {activeQuestionCard.questions.map((question, questionIndex) => (
                              <div key={question.id} className="grid gap-2">
                                <b>{questionIndex + 1}. {question.prompt}</b>
                                <div className="grid gap-1">
                                  {question.options.map(option => {
                                    const selectedOption = (questionAnswers[question.id] ?? []).includes(option.id);
                                    return (
                                      <button
                                        key={option.id}
                                        type="button"
                                        className={questionOptionClass(selectedOption)}
                                        onClick={() => toggleQuestionOption(question, option.id)}
                                        disabled={busy}
                                      >
                                        <b>{option.id}</b>
                                        <span>
                                          <strong>{option.label}</strong>
                                          {option.description && <small>{option.description}</small>}
                                        </span>
                                      </button>
                                    );
                                  })}
                                </div>
                                {question.allow_custom && (
                                  <textarea
                                    rows={2}
                                    className="min-h-[52px] resize-none rounded-[5px] border border-[var(--border-light)] bg-[var(--bg-base)] px-2.5 py-2 text-[12px] leading-[1.4] text-[var(--text-primary)] outline-none placeholder:text-[var(--text-muted)]"
                                    value={questionCustomAnswers[question.id] ?? ''}
                                    onChange={event => setQuestionCustomAnswers(current => ({ ...current, [question.id]: event.target.value }))}
                                    placeholder="Custom answer"
                                    disabled={busy}
                                  />
                                )}
                              </div>
                            ))}
                          </section>
                          <footer>
                            <button
                              type="button"
                              className="inline-flex h-7 cursor-pointer items-center gap-1.5 rounded-[5px] border border-transparent bg-transparent px-2 text-[11px] text-[var(--text-muted)] hover:text-[var(--text-primary)]"
                              onClick={() => setQuestionCardCollapsed(true)}
                            >
                              Cancel
                            </button>
                            <button
                              type="button"
                              className="inline-flex h-7 cursor-pointer items-center gap-1.5 rounded-[5px] border border-[var(--text-primary)] bg-[var(--text-primary)] px-2.5 text-[11px] font-semibold text-[var(--bg-base)] disabled:cursor-default disabled:opacity-45"
                              onClick={submitQuestionCard}
                              disabled={busy || !activeQuestionCard.questions.some(question => (questionAnswers[question.id] ?? []).length > 0 || Boolean(questionCustomAnswers[question.id]?.trim()))}
                            >
                              Continue
                            </button>
                          </footer>
                        </>
                      )}
                    </div>
                  )}
                  {questInputSuggestionMode && questInputSuggestions.length > 0 && (
                    <div className={questInputSuggestClass}>
                      {questInputSuggestions.map((suggestion, index) => (
                        <button
                          key={suggestion.id}
                          type="button"
                          className={questInputSuggestItemClass(index === questInputSuggestionIndex)}
                          onMouseDown={event => {
                            event.preventDefault();
                            chooseQuestInputSuggestion(suggestion).catch(reportError);
                          }}
                        >
                          {suggestion.mode === 'file' ? <IconFile /> : <IconCode />}
                          <span>
                            <strong>{suggestion.label}</strong>
                            <small>{suggestion.detail}</small>
                          </span>
                          <kbd>{suggestion.value}</kbd>
                        </button>
                      ))}
                    </div>
                  )}
                  <div className={questInputTokenBoxClass}>
                    <div className={questInputTextRowClass}>
                      {questInputFileTokens.map(token => (
                        <span className={questInputFileTokenClass} key={token.path}>
                          <IconFile />
                          <span title={token.path}>{token.path}</span>
                          <button
                            type="button"
                            onClick={() => setQuestInputFileTokens(tokens => tokens.filter(item => item.path !== token.path))}
                            title="Remove file"
                            aria-label={`Remove ${token.path}`}
                            disabled={busy}
                          >
                            <IconX />
                          </button>
                        </span>
                      ))}
                      <textarea
                        ref={questInputRef}
                        rows={1}
                        className={questInputTextareaClass}
                        value={questInput}
                        onChange={event => setQuestInput(event.target.value)}
                        onKeyDown={event => {
                          if (questInputSuggestions.length > 0 && ['ArrowDown', 'ArrowUp', 'Tab', 'Enter', 'Escape'].includes(event.key)) {
                            if (event.key === 'Escape') {
                              event.preventDefault();
                              setQuestInputSuggestionMode(null);
                              return;
                            }
                            if (event.key === 'ArrowDown') {
                              event.preventDefault();
                              setQuestInputSuggestionIndex(index => Math.min(index + 1, questInputSuggestions.length - 1));
                              return;
                            }
                            if (event.key === 'ArrowUp') {
                              event.preventDefault();
                              setQuestInputSuggestionIndex(index => Math.max(index - 1, 0));
                              return;
                            }
                            event.preventDefault();
                            chooseQuestInputSuggestion(questInputSuggestions[questInputSuggestionIndex]).catch(reportError);
                            return;
                          }
                          if (event.key === 'Backspace' && !questInput) {
                            setQuestInputFileTokens(tokens => tokens.slice(0, -1));
                            return;
                          }
                          if (event.key === 'Enter' && !event.shiftKey) {
                            event.preventDefault();
                            submitQuestInput();
                          }
                        }}
                        placeholder={questInputFileTokens.length === 0
                          ? creatingQuestGoal
                            ? 'Quest is being created'
                            : selected?.status === 'ready_for_review'
                            ? 'Ask for revision or explain what to change  @ file  / command'
                            : selected?.status === 'blocked'
                              ? 'Provide missing info  @ file  / command'
                              : selected && ['running', 'validating', 'repairing', 'applying'].includes(selected.status)
                                ? 'Add instruction or context  @ file  / command'
                                : `${t('quest_input_placeholder')}  @ file  / command`
                          : 'Message'}
                        disabled={busy || !selected}
                      />
                    </div>
                    <div className={questClasses.composerToolbar}>
                      <QuestDropdown
                        value={questMode}
                        options={[
                          { value: 'solo', label: t('quest_mode_solo') },
                          { value: 'extra', label: t('quest_mode_extra') },
                        ]}
                        onChange={value => setQuestMode(value as QuestMode)}
                        disabled={busy || !selected || executionLockedStatuses.includes(selected.status)}
                        compact
                        widthClass="w-10"
                        menuWidthClass="w-[116px]"
                        placement="top"
                      />
                      <QuestModelThinkingDropdown
                        modelValue={modelConfig.model}
                        thinkingValue={modelConfig.thinking_effort}
                        modelOptions={modelSelectOptions}
                        thinkingOptions={thinkingOptions}
                        onModelChange={chooseQuestModel}
                        onThinkingChange={value => setModelConfig(prev => ({ ...prev, thinking_effort: value }))}
                        disabled={busy || !selected || executionLockedStatuses.includes(selected.status)}
                        widthClass="w-[68px]"
                        menuWidthClass="w-[240px]"
                        placement="top"
                      />
                    </div>
                  </div>
                  <button className={questClasses.sendButton} onClick={submitQuestInput} disabled={busy || (!questInput.trim() && questInputFileTokens.length === 0)}>
                    <IconSend />
                  </button>
                </div>
              </div>

              <div
                className={cn(questClasses.paneResizeHandle, isResizingArtifactPane && questClasses.paneResizeHandleActive)}
                onPointerDown={startArtifactPaneResize}
                onKeyDown={event => {
                  if (event.key === 'ArrowLeft') {
                    event.preventDefault();
                    nudgeArtifactPane(2);
                  } else if (event.key === 'ArrowRight') {
                    event.preventDefault();
                    nudgeArtifactPane(-2);
                  } else if (event.key === 'Home') {
                    event.preventDefault();
                    nudgeArtifactPane(artifactPaneMaxPercent);
                  } else if (event.key === 'End') {
                    event.preventDefault();
                    nudgeArtifactPane(-artifactPaneMaxPercent);
                  }
                }}
                role="separator"
                aria-orientation="vertical"
                aria-label="Resize artifact pane"
                aria-valuemin={artifactPaneMinPercent}
                aria-valuemax={artifactPaneMaxPercent}
                aria-valuenow={Math.round(artifactPanePercent)}
                tabIndex={0}
                title="Resize artifact pane"
              />

              <aside className={questClasses.rightPanel}>
                <div className={questClasses.artifactHeader}>
                  <div>
                    <h2>Artifact workspace</h2>
                    <p>
                      {panel === 'artifact' && artifact
                        ? `${artifact.kind.replace('_', ' ')} · ${artifact.label}`
                        : panel === 'spec'
                          ? 'Spec.md'
                          : panel === 'intent'
                            ? 'Intent record'
                            : panel === 'review'
                              ? 'Review bundle'
                              : panel === 'knowledge'
                                ? 'Knowledge references'
                                : 'Quest evidence and controls'}
                    </p>
                  </div>
                  <div className={questClasses.panelTabs}>
                    {panelTabs.map(tab => {
                      const active = panel === tab.id;
                      return (
                        <button
                          key={tab.id}
                          className={cn(questClasses.panelTab, active && questClasses.panelTabActive)}
                          onClick={() => { setArtifact(null); setPanel(tab.id); }}
                          title={t(tab.labelKey)}
                          aria-label={t(tab.labelKey)}
                          aria-current={active ? 'page' : undefined}
                        >
                          {tab.icon}
                          {active && <span>{t(tab.labelKey)}</span>}
                        </button>
                      );
                    })}
                    {artifact && (
                      <button
                        className={cn(questClasses.panelTab, panel === 'artifact' && questClasses.panelTabActive)}
                        onClick={() => setPanel('artifact')}
                        title={artifact.label}
                        aria-label={artifact.label}
                        aria-current={panel === 'artifact' ? 'page' : undefined}
                      >
                        <IconFile />
                        {panel === 'artifact' && <span>{artifact.label}</span>}
                      </button>
                    )}
                  </div>
                </div>

                {creatingQuestGoal && (
                  <div className={questClasses.reviewEmpty}>
                    <IconFile />
                    <strong>Artifact workspace</strong>
                    <p className={mutedText}>Artifacts will appear here when the agent creates them.</p>
                  </div>
                )}

                {!creatingQuestGoal && panel === 'overview' && selected && (
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
                      {extraQuestArtifacts(selected).map(artifact => (
                        <button
                          className={artifactRowClass}
                          key={`${artifact.kind}-${artifact.path ?? artifact.label}`}
                          onClick={() => openArtifact(artifact)}
                        >
                          {artifact.kind === 'thinking' ? <IconSparkles /> : <IconFile />}
                          <span><strong>{artifact.label}</strong><small>{artifact.path}</small></span>
                        </button>
                      ))}
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
                      <button className={artifactRowClass} onClick={() => { setArtifact(null); setPanel('review'); }} disabled={!selected.review}>
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
                        <button className={artifactRowClass} key={entry.id} onClick={() => { setArtifact(null); setPanel('knowledge'); }}>
                          <IconSparkles /><span><strong>{entry.category}</strong><small>{entry.content}</small></span>
                        </button>
                      ))}
                    </section>
                  </div>
                )}

                {!creatingQuestGoal && panel === 'intent' && selected && (
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

                {!creatingQuestGoal && panel === 'spec' && selected && (
                  <div className={documentPanelClass}>
                    <div className={questClasses.sectionHeading}>
                      <div><span>{t('quest_ai_tool_spec')}</span><strong>{t('quest_ai_tool_spec_desc')}</strong></div>
                      <div>
                        <button className={sectionHeadingButton} onClick={() => { setSpecMode(specMode === 'edit' ? 'preview' : 'edit'); setSpecSelection(null); setSpecSelectionQuestion(''); }}>
                          {specMode === 'edit' ? 'Preview' : 'Edit'}
                        </button>
                        <button className={sectionHeadingButton} onClick={saveSpec} disabled={busy || specDraft === selected.spec}><IconEdit /> {t('btn_save')}</button>
                        <button className={cn(sectionHeadingButton, primaryButton)} onClick={execute} disabled={busy || selected.status === 'archived'}>
                          {busy ? <QuestLoader /> : <IconPlay />} {t('quest_approve')}
                        </button>
                      </div>
                    </div>
                    <div className="relative flex min-h-0 flex-1">
                      {specSelection && (
                        specSelection.mode === 'button' ? (
                          <button
                            className={askAiSelectionButtonClass}
                            style={{ top: specSelection.top, left: specSelection.left }}
                            onClick={askAiAboutSpecSelection}
                            type="button"
                          >
                            <IconSparkles /> Ask AI
                          </button>
                        ) : (
                          <div
                            className={askAiSelectionPromptClass}
                            style={{ top: specSelection.top, left: specSelection.left }}
                          >
                            <textarea
                              ref={specAskInputRef}
                              className={askAiSelectionPromptInputClass}
                              value={specSelectionQuestion}
                              onChange={event => setSpecSelectionQuestion(event.target.value)}
                              onKeyDown={event => {
                                if (event.key === 'Escape') {
                                  event.preventDefault();
                                  setSpecSelection(null);
                                  setSpecSelectionQuestion('');
                                  return;
                                }
                                if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
                                  event.preventDefault();
                                  submitSpecSelectionQuestion();
                                }
                              }}
                              placeholder="Ask about this selection..."
                              disabled={busy}
                            />
                            <div className={askAiSelectionPromptActionsClass}>
                              <button
                                className="border-[var(--border-light)] bg-transparent text-[var(--text-secondary)] hover:border-[var(--border-light)] hover:text-[var(--text-primary)]"
                                type="button"
                                onClick={() => {
                                  setSpecSelection(null);
                                  setSpecSelectionQuestion('');
                                }}
                                disabled={busy}
                              >
                                Cancel
                              </button>
                              <button
                                className="border-[var(--brand)] bg-[var(--brand)] text-[var(--bg-base)] hover:border-[var(--brand-hover)] hover:bg-[var(--brand-hover)]"
                                type="button"
                                onClick={submitSpecSelectionQuestion}
                                disabled={busy || !specSelectionQuestion.trim()}
                              >
                                <IconSend /> Ask
                              </button>
                            </div>
                          </div>
                        )
                      )}
                      {specMode === 'edit' ? (
                        <textarea
                          value={specDraft}
                          onChange={event => {
                            setSpecDraft(event.target.value);
                            setSpecSelection(null);
                          }}
                          onSelect={event => captureSpecTextareaSelection(event.currentTarget)}
                          disabled={!['draft', 'clarifying', 'specified', 'planning', 'waiting_for_user', 'blocked'].includes(selected.status)}
                        />
                      ) : (
                        <div
                          className={markdownPreviewClass}
                          onMouseUp={event => captureSpecPreviewSelection(event.currentTarget)}
                          onKeyUp={event => captureSpecPreviewSelection(event.currentTarget)}
                        >
                          <ReactMarkdown remarkPlugins={[remarkGfm]}>
                            {specDraft || selected.spec}
                          </ReactMarkdown>
                        </div>
                      )}
                    </div>
                  </div>
                )}

                {!creatingQuestGoal && panel === 'artifact' && artifact && selected && (
                  <div className={questClasses.artifactViewer}>
                    <header>
                      <button className={sectionHeadingButton} onClick={() => { setArtifact(null); setPanel('overview'); }}><IconChevronRight /> {t('quest_tab_overview')}</button>
                      <button className={sectionHeadingButton} onClick={() => onOpenEditor(selected.project.path, artifactFor(artifact.kind, artifact.label, artifact.path))}><IconCode /> {t('quest_open_editor')}</button>
                    </header>
                    <div>
                      <span>{artifact.kind.replace('_', ' ')}</span>
                      <h2>{artifact.label}</h2>
                      {artifact.path && <p>{artifact.path}</p>}
                    </div>
                    <pre>{artifactContentKey === `${selected.id}:${artifact.path ?? ''}` && artifactContent !== null
                      ? artifactContent
                      : artifact.kind === 'changed_file'
                        ? selected.review?.changed_files.find(file => file.path === artifact.path)?.diff ?? t('quest_no_diff')
                        : JSON.stringify(
                          artifact.kind === 'trace'
                            ? selected.events
                            : artifact.kind === 'thinking'
                              ? selected.events.filter(event => event.kind === 'thinking' && (!artifact.path || event.details && typeof event.details === 'object' && (event.details as { path?: unknown }).path === artifact.path))
                              : artifact.kind === 'exploration'
                                ? selected.review?.exploration_attempts.find(attempt => attempt.artifact_path === artifact.path || attempt.label === artifact.label)
                                : artifact.kind === 'checkpoint'
                                  ? selected.checkpoints.find(checkpoint => checkpoint.artifact_path === artifact.path || checkpoint.label === artifact.label || checkpoint.id === artifact.path)
                                  : artifact.kind === 'validation'
                                    ? selected.review?.validations.find(validation => validation.name === artifact.label)
                                    : artifact.kind === 'review_finding'
                                      ? selected.review?.findings.find(finding => finding.artifact_path === artifact.path || finding.title === artifact.label || finding.summary === artifact.label)
                                        ?? selected.review?.unresolved_issues.find(issue => issue === artifact.label)
                                      : selected.artifact_links.find(link => link.path === artifact.path || link.label === artifact.label)
                                        ?? selected.review?.unresolved_issues.find(issue => issue === artifact.label),
                          null,
                          2,
                        )}</pre>
                  </div>
                )}

                {!creatingQuestGoal && panel === 'knowledge' && selected && (
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

                {!creatingQuestGoal && panel === 'review' && selected && (
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
            <small>{quest.project.name} · <em>{statusBadgeLabel(quest.status)}</em> · {formatTime(quest.updated_at_ms)}</small>
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
