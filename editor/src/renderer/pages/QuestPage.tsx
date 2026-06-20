import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  addQuestNote,
  branchQuest,
  cancelQuest,
  continueQuest,
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
  requestQuestRevision,
  requestQuestQuickFix,
  transitionQuest,
  updateQuestIntent,
  updateQuestKnowledgeContext,
  updateQuestSpec,
  type KnowledgeEntry,
  type QuestDetail,
  type QuestRecord,
  type QuestReviewAction,
  type QuestStatus,
} from '../quest';
import {
  IconAlertCircle,
  IconCheck,
  IconChevronRight,
  IconCode,
  IconEdit,
  IconFile,
  IconLoader,
  IconPlay,
  IconPlus,
  IconRefresh,
  IconSend,
  IconSparkles,
  IconTrash,
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

const buttonBase = 'inline-flex h-[30px] cursor-pointer items-center gap-[6px] rounded-[5px] border border-[#d9d9d6] bg-white px-[10px] text-[11px] font-semibold text-[#303236] hover:border-[#b8b8b2] hover:bg-[#f6f6f4] hover:text-[#111827] disabled:cursor-default disabled:opacity-40';
const sectionHeadingButton = buttonBase;
const primaryButton = 'border-[#303236] bg-[#303236] text-white hover:border-[#111827] hover:bg-[#111827] hover:text-white';
const mutedText = 'm-0 text-[12px] text-[#9ca3af]';
const panelSection = '[&_section]:mb-5 [&_section]:border-b [&_section]:border-[#eeeeeb] [&_section]:pb-4 [&_h2]:mb-[10px] [&_h2]:mt-0 [&_h2]:flex [&_h2]:items-center [&_h2]:justify-between [&_h2]:gap-[10px] [&_h2]:text-[12px] [&_h2]:font-medium [&_h2]:text-[#5f646d] [&_h2_b]:text-[11px] [&_h2_b]:font-medium [&_h2_b]:text-[#9ca3af]';
const artifactRowClass = 'box-border grid w-full cursor-pointer grid-cols-[18px_minmax(0,1fr)] items-center gap-[9px] border-0 bg-transparent py-2 text-left text-[#4b5563] disabled:cursor-default disabled:opacity-50 [&_small]:mt-[3px] [&_small]:block [&_small]:overflow-hidden [&_small]:text-ellipsis [&_small]:whitespace-nowrap [&_small]:text-[11px] [&_small]:text-[#8b9099] [&_span]:min-w-0 [&_strong]:block [&_strong]:overflow-hidden [&_strong]:text-ellipsis [&_strong]:whitespace-nowrap [&_strong]:text-[12px] [&_strong]:font-medium [&_strong]:text-[#303236]';
const fileRowClass = 'box-border grid min-h-[34px] w-full grid-cols-[18px_minmax(0,1fr)_auto] items-center gap-[9px] border-0 bg-transparent py-[7px] text-left text-[#4b5563] hover:bg-[#f7f7f5] [&_small]:mt-[3px] [&_small]:block [&_small]:text-[10px] [&_small]:text-[#8b9099] [&_span]:min-w-0 [&_strong]:block [&_strong]:overflow-hidden [&_strong]:text-ellipsis [&_strong]:whitespace-nowrap [&_strong]:text-[12px] [&_strong]:font-medium [&_strong]:text-[#4b5563] [&>b]:text-[11px] [&>b]:font-mono [&>b]:text-[#059669] [&>b_i]:not-italic [&>b_i]:text-[#dc2626]';
const validationRowClass = cn(fileRowClass, 'cursor-pointer font-[inherit] [&>b]:text-[10px] [&>b]:uppercase [&_svg]:text-[#4ade80]');
const selectableFileRowClass = cn(fileRowClass, 'cursor-pointer grid-cols-[18px_16px_minmax(0,1fr)_auto] [&_input]:m-0 [&_input]:h-[13px] [&_input]:w-[13px]');
const transactionRowClass = cn(selectableFileRowClass, 'min-h-[46px] py-[9px] [&_small]:whitespace-normal [&_strong]:text-[#111827]');
const documentPanelClass = 'flex min-h-0 flex-col p-[14px] [&_textarea]:box-border [&_textarea]:min-h-0 [&_textarea]:w-full [&_textarea]:flex-1 [&_textarea]:resize-none [&_textarea]:rounded-lg [&_textarea]:border [&_textarea]:border-[#e5e5e2] [&_textarea]:bg-white [&_textarea]:px-[26px] [&_textarea]:py-[22px] [&_textarea]:font-mono [&_textarea]:text-[11px] [&_textarea]:leading-[1.75] [&_textarea]:text-[#202124] [&_textarea]:outline-none [&_textarea:disabled]:text-[#94a3b8] [&_textarea:disabled]:opacity-75 [&_textarea:focus]:border-[#3b5d91]';
const issueClass = 'flex items-start gap-2 rounded-md border border-[#713f12] bg-[rgba(120,53,15,0.12)] px-3 py-[11px] text-[9px] leading-[1.5] text-[#fcd34d] [&>button:last-child]:ml-auto [&>button:last-child]:cursor-pointer [&>button:last-child]:rounded [&>button:last-child]:border [&>button:last-child]:border-[#854d0e] [&>button:last-child]:bg-transparent [&>button:last-child]:px-2 [&>button:last-child]:py-[5px] [&>button:last-child]:text-[10px] [&>button:last-child]:font-bold [&>button:last-child]:text-[#fde68a] [&_svg]:shrink-0';
const clearIssueClass = 'border-[#14532d] bg-[rgba(20,83,45,0.14)] text-[#86efac]';
const issueOpenClass = 'flex min-w-0 flex-1 cursor-pointer items-start gap-2 border-0 bg-transparent p-0 text-left font-[inherit] text-inherit hover:underline hover:underline-offset-2';
const reviewActionButtonClass = 'h-[30px] cursor-pointer rounded-[5px] border border-[#c7c7c2] bg-[#f1f1ef] px-[10px] text-[9px] font-bold text-[#303236] disabled:cursor-default disabled:opacity-40';
const decisionButtonClass = 'inline-flex h-8 cursor-pointer items-center gap-[6px] rounded-[5px] border border-[var(--border-light)] bg-[var(--bg-surface)] px-[11px] text-[9px] font-bold text-[var(--text-secondary)] disabled:cursor-default disabled:opacity-40';

const questClasses = {
  shell: 'grid h-screen w-screen grid-rows-[48px_minmax(0,1fr)_24px] overflow-hidden bg-[#fbfbfa] font-[Inter,var(--font-sans)] text-[#202124]',
  globalHeader: 'grid grid-cols-[220px_minmax(0,1fr)_auto] items-center border-b border-[#e6e6e3] bg-[rgba(255,255,255,0.94)] text-[#242629] max-[900px]:grid-cols-[150px_minmax(0,1fr)_auto] [&_nav]:flex [&_nav]:h-full [&_nav]:items-stretch [&_nav]:gap-0.5',
  brand: 'flex items-baseline gap-2 px-4 [&_span]:text-[13px] [&_span]:font-extrabold [&_span]:text-[#303236] [&_strong]:text-[11px] [&_strong]:text-[#4b5563]',
  topNavButton: 'cursor-pointer border-0 border-b-2 border-transparent bg-transparent px-[14px] text-[11px] font-semibold text-[#4b5563] disabled:cursor-default disabled:opacity-40 max-[900px]:px-[7px]',
  topNavButtonActive: 'border-b-[#111827] text-[#111827]',
  globalActions: 'flex gap-[6px] pr-[10px]',
  layout: 'grid min-h-0 grid-cols-[280px_minmax(0,1fr)] bg-[#fbfbfa] max-[900px]:grid-cols-[220px_minmax(0,1fr)]',
  sidebar: 'grid grid-rows-[auto_minmax(0,auto)_minmax(0,auto)_1fr] overflow-y-auto border-r border-[#e5e5e2] bg-[#f7f7f5] text-[#222326]',
  sidebarHeading: 'flex min-h-[88px] items-center justify-between px-3 py-[14px]',
  newButton: 'grid h-[38px] w-full cursor-pointer grid-cols-[18px_minmax(0,1fr)_auto] items-center gap-2 rounded-lg border border-[#dededb] bg-white px-[11px] text-left text-[13px] text-[#191b1f] shadow-[0_1px_2px_rgba(15,23,42,0.04)] disabled:cursor-default disabled:opacity-40 [&_kbd]:font-mono [&_kbd]:text-[10px] [&_kbd]:text-[#8a8f98] [&_svg]:text-[#303236]',
  sidebarFooter: 'self-end grid gap-1 p-3 [&_button]:h-8 [&_button]:rounded-[7px] [&_button]:border-0 [&_button]:bg-transparent [&_button]:text-left [&_button]:text-[12px] [&_button]:text-[#4b5563]',
  home: 'flex min-h-0 flex-col items-center justify-center bg-[#fbfbfa] px-8 pb-[90px] pt-10 text-[#202124]',
  orb: 'mb-[26px] grid h-[60px] w-[60px] place-items-center rounded-full border border-[#eeeeeb] bg-white text-[#d6d8dd] shadow-[0_8px_30px_rgba(15,23,42,0.05)]',
  startLine: 'mb-7 flex flex-wrap items-center justify-center gap-[9px] text-[12px] text-[#6b7280] [&_b]:font-medium [&_b]:text-[#34373d] [&_span]:font-medium',
  promptBox: 'w-[min(800px,calc(100vw-360px))] min-w-[min(800px,calc(100vw-360px))] rounded-lg border border-[#dededb] bg-white shadow-[0_18px_50px_rgba(15,23,42,0.08)] max-[900px]:w-[min(680px,calc(100vw-280px))] max-[900px]:min-w-0 [&_footer]:flex [&_footer]:h-[42px] [&_footer]:items-center [&_footer]:justify-between [&_footer]:pb-2 [&_footer]:pl-[13px] [&_footer]:pr-[9px] [&_footer_div]:flex [&_footer_div]:gap-4 [&_footer_div]:text-[12px] [&_footer_div]:text-[#5f646d] [&_textarea]:box-border [&_textarea]:h-[94px] [&_textarea]:w-full [&_textarea]:resize-none [&_textarea]:border-0 [&_textarea]:bg-transparent [&_textarea]:px-[14px] [&_textarea]:pb-2 [&_textarea]:pt-[14px] [&_textarea]:font-[Inter,var(--font-sans)] [&_textarea]:text-[14px] [&_textarea]:leading-[1.5] [&_textarea]:text-[#202124] [&_textarea]:outline-none [&_textarea::placeholder]:text-[#a3a7ae]',
  promptSubmit: 'grid h-[30px] w-[30px] cursor-pointer place-items-center rounded-[7px] border-0 bg-[#8c8f94] text-white hover:enabled:bg-[#303236] disabled:cursor-default disabled:opacity-45',
  introCard: 'mt-16 grid w-[min(640px,calc(100vw-440px))] grid-cols-[96px_minmax(0,1fr)] gap-[18px] rounded-lg border border-dashed border-[#e3e5e8] bg-white p-3 text-[#4b5563] max-[900px]:w-[min(680px,calc(100vw-280px))] max-[900px]:grid-cols-1 max-[900px]:min-w-0 [&>svg]:h-[70px] [&>svg]:w-24 [&>svg]:rounded-md [&>svg]:bg-[#eef7f0] [&>svg]:p-4 [&>svg]:text-[#a8d8b5] [&_p]:m-0 [&_p]:text-[12px] [&_p]:leading-[1.5] [&_p]:text-[#8b9099] [&_strong]:mb-2 [&_strong]:mt-1 [&_strong]:block [&_strong]:text-[14px] [&_strong]:text-[#202124]',
  workspace: 'grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden bg-white',
  header: 'flex min-h-[78px] items-center justify-between gap-[18px] border-b border-[#e6e6e3] bg-white px-[18px] py-[10px] max-[900px]:flex-col max-[900px]:items-start [&_h1]:mb-[3px] [&_h1]:mt-1 [&_h1]:text-[15px] [&_h1]:font-semibold [&_h1]:text-[#202124] [&_p]:m-0 [&_p]:max-w-[820px] [&_p]:text-[11px] [&_p]:leading-[1.45] [&_p]:text-[#6b7280]',
  projectLine: 'flex items-center gap-[5px] font-mono text-[10px] text-[#8b9099] [&_svg]:w-[9px]',
  titleEdit: 'my-[7px] flex items-center gap-[6px] [&_button]:inline-flex [&_button]:h-[30px] [&_button]:cursor-pointer [&_button]:items-center [&_button]:gap-[5px] [&_button]:rounded-[5px] [&_button]:border [&_button]:border-[var(--border-light)] [&_button]:bg-[var(--bg-surface)] [&_button]:px-[9px] [&_button]:text-[9px] [&_button]:font-semibold [&_button]:text-[var(--text-secondary)] [&_button:disabled]:cursor-default [&_button:disabled]:opacity-40 [&_input]:h-[31px] [&_input]:w-[min(520px,50vw)] [&_input]:rounded-[5px] [&_input]:border [&_input]:border-[#52525b] [&_input]:bg-[#0d0e12] [&_input]:px-[10px] [&_input]:text-[15px] [&_input]:font-bold [&_input]:text-[#f1f5f9] [&_input]:outline-none',
  headerActions: 'flex flex-wrap items-center justify-end gap-[6px] max-[900px]:justify-start',
  cockpit: 'grid min-h-0 grid-cols-[minmax(420px,1fr)_minmax(380px,44%)] overflow-hidden bg-white max-[900px]:grid-cols-1 max-[900px]:grid-rows-[minmax(0,1fr)_minmax(360px,44vh)]',
  runStream: 'grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)_52px] border-r border-[#e6e6e3] bg-[#fbfbfa] max-[900px]:border-b max-[900px]:border-r-0',
  streamPrompt: 'mx-[22px] mb-2 mt-4 rounded-[7px] border border-[#e5e5e2] bg-[#f1f1ef] px-3 py-[9px] text-[13px] text-[#202124]',
  streamList: 'min-h-0 overflow-auto px-[22px] pb-[22px]',
  streamEntry: 'grid grid-cols-[16px_minmax(0,1fr)] gap-[9px] my-[6px] [&>div]:min-w-0 [&>div]:rounded-[5px] [&>div]:border [&>div]:border-[#e3e3df] [&>div]:bg-white [&>div]:px-[10px] [&>div]:py-2 [&_details]:mt-[6px] [&_details]:text-[10px] [&_details]:text-[#6b7280] [&_header]:flex [&_header]:items-center [&_header]:justify-between [&_header]:gap-[10px] [&_pre]:mt-[7px] [&_pre]:max-h-[260px] [&_pre]:overflow-auto [&_pre]:rounded-[5px] [&_pre]:bg-[#f7f7f5] [&_pre]:p-[9px] [&_pre]:font-mono [&_pre]:text-[10px] [&_pre]:leading-[1.55] [&_pre]:text-[#4b5563] [&_small]:mt-1 [&_small]:block [&_small]:text-[11px] [&_small]:text-[#8b9099] [&_strong]:overflow-hidden [&_strong]:text-ellipsis [&_strong]:whitespace-nowrap [&_strong]:text-[12px] [&_strong]:font-medium [&_strong]:text-[#202124] [&_summary]:cursor-pointer [&_time]:shrink-0 [&_time]:font-mono [&_time]:text-[10px] [&_time]:text-[#9ca3af]',
  nextEntry: '[&>div]:border-[#d7d7d3] [&_time]:font-bold [&_time]:text-[#111827]',
  liveEntry: '[&>div]:border-[#d7d7d3] [&>div]:bg-[#f6f6f4] [&_time]:font-bold [&_time]:text-[#303236]',
  timelineDot: 'relative mt-4 h-[9px] w-[9px] rounded-full border border-[#b8bec7] bg-white after:absolute after:left-1 after:top-[10px] after:h-[calc(100%+34px)] after:w-px after:bg-[#e0e0dc] after:content-[""]',
  timelineDotLast: 'after:hidden',
  timelineDotNext: 'border-[#111827]',
  timelineDotLive: 'border-[#303236] shadow-[0_0_0_3px_rgba(48,50,54,0.12)]',
  reviewChip: 'ml-6 mt-3 inline-flex w-max cursor-pointer items-center gap-[6px] rounded-md border border-[#dededb] bg-white px-[10px] py-[7px] text-[12px] text-[#059669] [&_span]:text-[#dc2626]',
  steerBar: 'grid grid-cols-[118px_minmax(0,1fr)_34px] items-center gap-3 border-t border-[#e6e6e3] bg-white px-3 py-2 text-[11px] text-[#8b9099] [&_input]:min-w-0 [&_input]:border-0 [&_input]:bg-transparent [&_input]:text-[12px] [&_input]:text-[#8b9099] [&_input]:outline-none [&_select]:h-7 [&_select]:min-w-0 [&_select]:rounded-[5px] [&_select]:border [&_select]:border-[#e6e6e3] [&_select]:bg-transparent [&_select]:px-[6px] [&_select]:text-[11px] [&_select]:text-[#4b5563] [&_select]:outline-none',
  sendButton: 'grid h-[30px] w-[30px] cursor-pointer place-items-center rounded-[5px] border border-[#303236] bg-[#303236] text-white disabled:cursor-default disabled:opacity-45',
  rightPanel: 'grid min-h-0 min-w-0 grid-rows-[37px_minmax(0,1fr)] bg-white',
  panelTabs: 'flex min-w-0 border-b border-[#e6e6e3] bg-[#fbfbfa]',
  panelTab: 'inline-flex cursor-pointer items-center gap-[6px] border-0 border-r border-[#e6e6e3] border-b-2 border-b-transparent bg-transparent px-[13px] text-[12px] text-[#5f646d]',
  panelTabActive: 'border-b-[#111827] bg-white text-[#111827]',
  overview: cn('min-h-0 overflow-auto p-4', panelSection),
  sectionHeading: 'flex items-center justify-between gap-3 py-[15px] [&>div:first-child]:flex [&>div:first-child]:flex-col [&>div:first-child]:gap-1 [&>div:last-child]:flex [&>div:last-child]:gap-[7px] [&_span]:text-[10px] [&_span]:font-extrabold [&_span]:tracking-[0.12em] [&_span]:text-[#64748b] [&_strong]:text-[11px] [&_strong]:text-[var(--text-secondary)]',
  artifactViewer: 'min-h-0 overflow-auto p-4 [&>div_span]:text-[11px] [&>div_span]:capitalize [&>div_span]:text-[#8b9099] [&_h2]:my-[6px] [&_h2]:text-[16px] [&_h2]:text-[#202124] [&_header]:mb-[18px] [&_header]:flex [&_header]:justify-between [&_header]:gap-2 [&_p]:m-0 [&_p]:font-mono [&_p]:text-[11px] [&_p]:text-[#6b7280] [&_pre]:mt-[7px] [&_pre]:max-h-[260px] [&_pre]:overflow-auto [&_pre]:rounded-[5px] [&_pre]:bg-[#f7f7f5] [&_pre]:p-[9px] [&_pre]:font-mono [&_pre]:text-[10px] [&_pre]:leading-[1.55] [&_pre]:text-[#4b5563]',
  knowledge: cn('grid gap-[14px] overflow-auto p-[14px]', panelSection),
  review: cn('min-h-0 overflow-auto p-4', panelSection),
  reviewEmpty: 'flex h-full min-h-[360px] flex-col items-center justify-center gap-2 text-[var(--text-muted)]',
  reviewSummary: 'mb-[18px] grid grid-cols-[72px_minmax(0,1fr)] gap-4 rounded-lg border border-[#e5e5e2] bg-[#fbfbfa] p-3 [&_div]:flex [&_div]:flex-col [&_div]:gap-[5px] [&_p]:m-0 [&_p]:text-[12px] [&_p]:leading-[1.6] [&_p]:text-[#4b5563] [&_span]:text-[10px] [&_span]:font-extrabold [&_span]:tracking-[0.12em] [&_span]:text-[#64748b] [&_strong]:text-[12px] [&_strong]:uppercase [&_strong]:text-[#059669]',
  reviewMetrics: 'grid grid-cols-[repeat(auto-fit,minmax(118px,1fr))] gap-2 [&_div]:min-h-[58px] [&_div]:rounded-md [&_div]:border [&_div]:border-[var(--border)] [&_div]:bg-[#131419] [&_div]:p-[10px] [&_span]:block [&_span]:overflow-hidden [&_span]:text-ellipsis [&_span]:whitespace-nowrap [&_span]:text-[10px] [&_span]:uppercase [&_span]:text-[var(--text-muted)] [&_strong]:mt-[7px] [&_strong]:block [&_strong]:font-mono [&_strong]:text-[13px] [&_strong]:text-[#e5e7eb]',
  metricNote: 'mb-0 mt-2 text-[9px] leading-[1.5] text-[var(--text-muted)]',
  reviewActions: 'flex flex-wrap gap-2',
  decisionRow: 'flex flex-wrap gap-2',
  decisionHistory: 'mt-[10px] grid gap-[6px] [&>div]:flex [&>div]:items-center [&>div]:gap-2 [&_button]:h-7 [&_button]:cursor-pointer [&_button]:rounded-[5px] [&_button]:border [&_button]:border-[#dc2626] [&_button]:bg-white [&_button]:px-[9px] [&_button]:text-[10px] [&_button]:text-[#b91c1c] [&_small]:flex-1 [&_small]:rounded-[5px] [&_small]:border [&_small]:border-[var(--border-light)] [&_small]:bg-[var(--bg-surface)] [&_small]:px-2 [&_small]:py-[7px] [&_small]:text-[10px] [&_small]:text-[var(--text-muted)]',
  footer: 'flex items-center justify-between border-t border-[#eeeeeb] bg-[#fbfbfa] px-[10px] font-mono text-[10px] text-[#8b9099]',
  errorToast: 'fixed right-4 top-[58px] z-[80] grid w-[min(360px,calc(100vw-32px))] cursor-pointer grid-cols-[18px_minmax(0,1fr)_14px] items-center gap-[10px] rounded-lg border border-[#fecaca] bg-[#fff7f7] px-3 py-[11px] text-left text-[#991b1b] shadow-[0_16px_42px_rgba(15,23,42,0.16)] [&_small]:block [&_small]:overflow-hidden [&_small]:text-ellipsis [&_small]:whitespace-nowrap [&_small]:text-[10px] [&_small]:text-[#7f1d1d] [&_span]:min-w-0 [&_strong]:mb-0.5 [&_strong]:block [&_strong]:text-[12px]',
  errorModal: 'fixed inset-0 z-[90] grid place-items-center bg-[rgba(15,23,42,0.28)] p-6 [&>div]:grid [&>div]:max-h-[min(520px,calc(100vh-48px))] [&>div]:w-[min(680px,100%)] [&>div]:grid-rows-[auto_minmax(0,1fr)_auto] [&>div]:rounded-lg [&>div]:border [&>div]:border-[#e5e7eb] [&>div]:bg-white [&>div]:text-[#111827] [&>div]:shadow-[0_24px_80px_rgba(15,23,42,0.22)] [&_footer]:flex [&_footer]:items-center [&_footer]:justify-end [&_footer]:gap-[10px] [&_footer]:border-t [&_footer]:border-[#f1f5f9] [&_footer]:px-[14px] [&_footer]:py-3 [&_header]:flex [&_header]:items-center [&_header]:justify-between [&_header]:gap-[10px] [&_header]:border-b [&_header]:border-[#f1f5f9] [&_header]:px-[14px] [&_header]:py-3 [&_header_span]:flex [&_header_span]:items-center [&_header_span]:gap-2 [&_header_span]:text-[13px] [&_header_span]:font-bold [&_header_span]:text-[#991b1b] [&_pre]:m-0 [&_pre]:overflow-auto [&_pre]:whitespace-pre-wrap [&_pre]:break-words [&_pre]:bg-[#f8fafc] [&_pre]:p-[14px] [&_pre]:font-mono [&_pre]:text-[11px] [&_pre]:leading-[1.6] [&_pre]:text-[#334155]',
  modalButton: 'inline-flex h-[30px] cursor-pointer items-center gap-[6px] rounded-[5px] border border-[#d1d5db] bg-white px-[10px] text-[11px] font-semibold text-[#374151]',
  group: 'px-3 pb-[10px] pt-[6px] [&>header]:flex [&>header]:items-center [&>header]:justify-between [&>header]:px-1 [&>header]:pb-2 [&>header]:pt-[7px] [&>header]:text-[12px] [&>header]:font-medium [&>header]:text-[#7a7f89] [&>header_b]:text-[10px] [&>header_b]:text-[#9ca3af] [&>p]:px-[22px] [&>p]:py-2 [&>p]:text-[11px] [&>p]:text-[#9ca3af]',
  groupButton: 'mb-0.5 grid w-full cursor-pointer grid-cols-[8px_minmax(0,1fr)] items-start gap-[9px] rounded-[7px] border border-transparent bg-transparent px-[7px] py-2 text-left text-[#3a3d43] hover:border-[#d7d7d3] hover:bg-[#f1f1ef] [&_div]:min-w-0 [&_small]:block [&_small]:overflow-hidden [&_small]:text-ellipsis [&_small]:whitespace-nowrap [&_small]:text-[10px] [&_small]:text-[#8b9099] [&_strong]:mb-1 [&_strong]:block [&_strong]:overflow-hidden [&_strong]:text-ellipsis [&_strong]:whitespace-nowrap [&_strong]:text-[12px] [&_strong]:font-medium [&_strong]:text-[#303236]',
  groupButtonActive: 'border-[#d7d7d3] bg-[#f1f1ef]',
  knowledgeRow: 'grid gap-2 rounded-md border border-[var(--border-light)] bg-[var(--bg-surface)] p-[10px] [&_footer]:flex [&_footer]:items-center [&_footer]:justify-start [&_footer]:gap-2 [&_footer_button]:inline-flex [&_footer_button]:h-7 [&_footer_button]:cursor-pointer [&_footer_button]:items-center [&_footer_button]:gap-[5px] [&_footer_button]:rounded-[5px] [&_footer_button]:border [&_footer_button]:border-[var(--border-light)] [&_footer_button]:bg-white [&_footer_button]:px-[9px] [&_footer_button]:text-[10px] [&_footer_button]:text-[var(--text-secondary)] [&_header]:flex [&_header]:items-center [&_header]:justify-between [&_header]:gap-2 [&_header_b]:font-mono [&_header_b]:text-[9px] [&_header_b]:text-[var(--text-muted)] [&_header_span]:text-[10px] [&_header_span]:font-bold [&_header_span]:uppercase [&_header_span]:text-[var(--text-secondary)] [&_p]:m-0 [&_p]:text-[12px] [&_p]:leading-[1.45] [&_p]:text-[var(--text-primary)] [&_small]:text-[10px] [&_small]:text-[var(--text-muted)]',
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

function statusDotClass(status: QuestStatus): string {
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
    'grid grid-cols-[18px_minmax(0,1fr)] items-start gap-2 text-[#6b7280] [&>span]:mt-px [&>span]:grid [&>span]:h-[13px] [&>span]:w-[13px] [&>span]:place-items-center [&>span]:rounded-full [&>span]:border [&>span]:border-[#8b9099] [&>span]:bg-white [&_p]:m-0 [&_p]:text-[12px] [&_p]:leading-[1.35]',
    status === 'done' && '[&>span]:border-[#d1d5db] [&>span]:bg-[#d1d5db] [&>span]:text-white [&_p]:text-[#a0a4ab]',
    status === 'current' && '[&>span]:border-[#111827] [&_p]:text-[#202124]',
  );
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

function statusAction(status: QuestStatus): { label: string; next: QuestStatus } | null {
  switch (status) {
    case 'running':
      return { label: 'Pause', next: 'waiting_for_user' };
    case 'waiting_for_user':
    case 'blocked':
      return { label: 'Resume', next: 'running' };
    case 'ready_for_review':
      return { label: 'Accept', next: 'completed' };
    default:
      return null;
  }
}

function defaultPanelForQuest(detail: QuestDetail): QuestPanel {
  return detail.status === 'draft' || detail.status === 'specified' ? 'spec' : 'overview';
}

function progressItems(detail: QuestDetail): Array<{ title: string; status: 'done' | 'current' | 'pending' }> {
  const taskEvents = detail.events.filter(event => event.kind === 'task_created');
  const titles = taskEvents.length > 0
    ? taskEvents.map(event => event.summary)
    : ['Review AI-generated spec', 'Approve Quest execution', 'Review evidence and decide'];
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
  const [quests, setQuests] = useState<QuestRecord[]>([]);
  const [knowledge, setKnowledge] = useState<KnowledgeEntry[]>([]);
  const [selected, setSelected] = useState<QuestDetail | null>(null);
  const [panel, setPanel] = useState<QuestPanel>('overview');
  const [artifact, setArtifact] = useState<QuestArtifactSelection | null>(null);
  const [intentDraft, setIntentDraft] = useState('');
  const [specDraft, setSpecDraft] = useState('');
  const [goal, setGoal] = useState('');
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

  const reportError = useCallback((reason: unknown) => {
    setError(String(reason));
    setErrorOpen(false);
  }, []);

  const resetReviewSelection = useCallback((detail: QuestDetail) => {
    setSelectedReviewFiles(new Set(detail.review?.changed_files.map(file => file.path) ?? []));
    setSelectedReviewGroups(new Set(detail.review?.transaction_groups.map(group => group.id) ?? []));
  }, []);

  const refreshList = useCallback(async (preferredId?: string | null) => {
    const result = await listQuests();
    setQuests(result.quests);
    const id = preferredId === null ? result.quests[0]?.id : preferredId ?? selected?.id ?? result.quests[0]?.id;
    if (id) {
      const detail = await getQuest(id);
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setPanel(defaultPanelForQuest(detail));
      resetReviewSelection(detail);
    } else {
      setSelected(null);
      setIntentDraft('');
      setSpecDraft('');
      setSelectedReviewFiles(new Set());
      setSelectedReviewGroups(new Set());
    }
  }, [resetReviewSelection, selected?.id]);

  const refreshKnowledge = useCallback(async () => {
    const result = await listKnowledge();
    setKnowledge(result.entries);
  }, []);

  useEffect(() => {
    refreshList(initialQuestId).catch(reportError);
    refreshKnowledge().catch(reportError);
  }, []); // Load the cross-project registry once on entry.

  const visibleQuests = useMemo(() => {
    const active = quests.filter(quest => ACTIVE_STATUSES.includes(quest.status));
    const history = quests.filter(quest => !ACTIVE_STATUSES.includes(quest.status));
    return { active, history };
  }, [quests]);

  const selectQuest = useCallback(async (quest: QuestRecord) => {
    setError(null);
    try {
      const detail = await getQuest(quest.id);
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
      setRenaming(false);
      setArtifact(null);
      setPanel(defaultPanelForQuest(detail));
      resetReviewSelection(detail);
    } catch (reason) {
      reportError(reason);
    }
  }, [reportError, resetReviewSelection]);

  const create = useCallback(async () => {
    if (!goal.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const detail = await createQuest('', goal.trim());
      setGoal('');
      setSelected(detail);
      setIntentDraft(detail.intent);
      setSpecDraft(detail.spec);
      setTitleDraft(detail.title);
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
  }, [goal, refreshList]);

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

  const remove = useCallback(async () => {
    if (!selected || selected.status !== 'archived') return;
    setBusy(true);
    setError(null);
    try {
      await deleteQuest(selected.id);
      setSelected(null);
      setIntentDraft('');
      setSpecDraft('');
      setSelectedReviewFiles(new Set());
      setSelectedReviewGroups(new Set());
      setTitleDraft('');
      setArtifact(null);
      setRenaming(false);
      await refreshList(null);
    } catch (reason) {
      reportError(reason);
    } finally {
      setBusy(false);
    }
  }, [refreshList, selected]);

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
          <strong>Quests</strong>
        </div>
        <nav>
          <button className={cn(questClasses.topNavButton, questClasses.topNavButtonActive)}>Quests</button>
        </nav>
        <div className={questClasses.globalActions}>
          <button
            className={buttonBase}
            onClick={() => selected && onOpenEditor(
              selected.project.path,
              artifactFor('intent', 'Quest intent', selected.intent_path),
            )}
            disabled={!selected}
          >
            <IconCode /> Open Editor
          </button>
          <button className={buttonBase} onClick={onCloseProject} title="Close project"><IconX /></button>
        </div>
      </header>

      <div className={questClasses.layout}>
        <aside className={questClasses.sidebar}>
          <div className={questClasses.sidebarHeading}>
            <button className={questClasses.newButton} onClick={() => setSelected(null)} disabled={!currentProjectPath}>
              <IconPlus /> New Quest <kbd>Ctrl N</kbd>
            </button>
          </div>

          <QuestGroup label="Quests" quests={visibleQuests.active} selectedId={selected?.id} onSelect={selectQuest} />
          <QuestGroup label="Completed" quests={visibleQuests.history} selectedId={selected?.id} onSelect={selectQuest} />
          <div className={questClasses.sidebarFooter}>
            <button onClick={() => setPanel('knowledge')}>Knowledge <b>{knowledge.filter(entry => entry.status === 'pending').length}</b></button>
            <button disabled>Marketplace</button>
          </div>
        </aside>

        {!selected ? (
          <main className={questClasses.home}>
            <div className={questClasses.orb}><IconSparkles size={28} /></div>
            <h1 className="m-0 mb-3 text-[clamp(28px,3vw,40px)] font-[650] leading-[1.1] text-[#202124]">Quest on, hands off</h1>
            <div className={questClasses.startLine}>
              <span>Start in</span>
              <b>{currentProjectPath ? 'Aster' : 'No project'}</b>
              <span>Local</span>
              <span>main</span>
            </div>
            <div className={questClasses.promptBox}>
              <textarea
                value={goal}
                onChange={event => setGoal(event.target.value)}
                placeholder="Describe a Quest outcome. AI will create the spec and task artifacts with tools."
                disabled={!currentProjectPath}
              />
              <footer>
                <div>
                  <span>AI required</span>
                  <span>tool-created spec</span>
                </div>
                <button className={questClasses.promptSubmit} onClick={create} disabled={busy || !goal.trim() || !currentProjectPath} title="Create Quest">
                  {busy ? <IconLoader className="animate-spin" /> : <IconSend />}
                </button>
              </footer>
            </div>
            <div className={questClasses.introCard}>
              <IconSparkles />
              <div>
                <strong>Meet Quest Mode</strong>
                <p>Describe the outcome. Aster drafts a named spec, tracks execution, and keeps review separate from active project changes.</p>
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
                      <span>branched from {selected.branch_of}</span>
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
                    <button onClick={rename} disabled={busy || !titleDraft.trim()}><IconCheck /> Save</button>
                    <button onClick={() => { setTitleDraft(selected.title); setRenaming(false); }}><IconX /></button>
                  </div>
                ) : (
                  <h1>{selected.title}</h1>
                )}
                <p>{selected.goal}</p>
              </div>
              <div className={questClasses.headerActions}>
                <span className={cn('rounded-full border border-current px-2 py-[5px] text-[10px] font-extrabold uppercase', statusTextClass(selected.status))}>{selected.status}</span>
                {!renaming && <button className={buttonBase} onClick={() => setRenaming(true)} disabled={busy}><IconEdit /> Rename</button>}
                <button className={buttonBase} onClick={() => onOpenEditor(
                  selected.project.path,
                  artifactFor('intent', 'Quest intent', selected.intent_path),
                )}><IconCode /> Open Editor</button>
                <button className={buttonBase} onClick={branchSelectedQuest} disabled={busy}><IconSparkles /> Branch</button>
                <button className={buttonBase} onClick={exportSelectedQuest} disabled={busy}>Export</button>
                {action && (
                  <button
                    className={buttonBase}
                    onClick={() => action.label === 'Resume'
                      ? continueSelectedQuest('Resume Quest from current evidence')
                      : transition(action.next)}
                    disabled={busy}
                  >
                    {action.label}
                  </button>
                )}
                {selected.status === 'ready_for_review' && (
                  <button className={buttonBase} onClick={rejectSelectedQuest} disabled={busy}>Reject</button>
                )}
                {!['archived', 'canceled', 'completed', 'running'].includes(selected.status) && (
                  <button className={buttonBase} onClick={cancelSelectedQuest} disabled={busy}>Cancel</button>
                )}
                {!['archived', 'canceled', 'running'].includes(selected.status) && (
                  <button className={buttonBase} onClick={() => transition('archived')} disabled={busy}>Archive</button>
                )}
                {['archived', 'canceled', 'completed'].includes(selected.status) && (
                  <button className={buttonBase} onClick={reopenSelectedQuest} disabled={busy}>Reopen</button>
                )}
                {selected.status === 'archived' && (
                  <button className={cn(buttonBase, 'hover:border-[#7f1d1d] hover:text-[#fecaca]')} onClick={remove} disabled={busy}><IconTrash /> Delete</button>
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
                      <header><strong>Quest goal accepted</strong><time>{formatTime(selected.created_at_ms)}</time></header>
                      <small>user prompt</small>
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
                            <summary>Evidence</summary>
                            <pre>{formatEventDetails(event.details)}</pre>
                          </details>
                        )}
                      </div>
                    </article>
                  ))}
                  <article className={cn(questClasses.streamEntry, questClasses.nextEntry)}>
                    <span className={cn(questClasses.timelineDot, questClasses.timelineDotNext)} />
                    <div>
                      <header><strong>{selected.next_action.label}</strong><time>next</time></header>
                      <small>{selected.next_action.reason}</small>
                    </div>
                  </article>
                  {['draft', 'specified'].includes(selected.status) && (
                    <article className={cn(questClasses.streamEntry, questClasses.nextEntry)}>
                      <span className={cn(questClasses.timelineDot, questClasses.timelineDotNext)} />
                      <div>
                        <header><strong>Spec is ready for review</strong><time>next</time></header>
                        <small>edit the spec in the right panel, then approve and execute</small>
                      </div>
                    </article>
                  )}
                  {executingQuestId === selected.id && (
                    <article className={cn(questClasses.streamEntry, questClasses.liveEntry)}>
                      <span className={cn(questClasses.timelineDot, questClasses.timelineDotLive, questClasses.timelineDotLast)} />
                      <div>
                        <header><strong>Quest execution is running</strong><time>live</time></header>
                        <small>waiting for workspace events and review bundle</small>
                      </div>
                    </article>
                  )}
                  {selected.review && (
                    <button className={questClasses.reviewChip} onClick={() => setPanel('review')}>
                      <IconCheck /> Review +{selected.review.changed_files.reduce((sum, file) => sum + file.additions, 0)}
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
                    <option value="steer">Steer</option>
                    <option value="clarify">Clarify</option>
                    <option value="manual_intervention">Manual edit</option>
                    <option value="pause">Pause note</option>
                  </select>
                  <input
                    value={questInput}
                    onChange={event => setQuestInput(event.target.value)}
                    onKeyDown={event => {
                      if (event.key === 'Enter') submitQuestInput();
                    }}
                    placeholder="Steer this Quest, answer a clarification, or record a manual edit"
                    disabled={busy || !selected}
                  />
                  <button className={questClasses.sendButton} onClick={submitQuestInput} disabled={busy || !questInput.trim()}>
                    <IconSend />
                  </button>
                </div>
              </div>

              <aside className={questClasses.rightPanel}>
                <div className={questClasses.panelTabs}>
                  <button className={cn(questClasses.panelTab, panel === 'overview' && questClasses.panelTabActive)} onClick={() => setPanel('overview')}><IconRefresh /> Overview</button>
                  <button className={cn(questClasses.panelTab, panel === 'intent' && questClasses.panelTabActive)} onClick={() => setPanel('intent')}><IconFile /> Intent</button>
                  <button className={cn(questClasses.panelTab, panel === 'spec' && questClasses.panelTabActive)} onClick={() => setPanel('spec')}><IconFile /> Spec</button>
                  <button className={cn(questClasses.panelTab, panel === 'review' && questClasses.panelTabActive)} onClick={() => setPanel('review')}><IconCheck /> Review</button>
                  <button className={cn(questClasses.panelTab, panel === 'knowledge' && questClasses.panelTabActive)} onClick={() => setPanel('knowledge')}><IconSparkles /> Knowledge</button>
                </div>

                {panel === 'overview' && (
                  <div className={questClasses.overview}>
                    <section>
                      <h2>Progress</h2>
                      <ol className="m-0 grid list-none gap-[10px] p-0">
                        {progressItems(selected).map((item, index) => (
                          <li key={`${item.title}-${index}`} className={progressItemClass(item.status)}>
                            <span>{item.status === 'done' ? <IconCheck /> : null}</span>
                            <p>{item.title}</p>
                          </li>
                        ))}
                      </ol>
                    </section>

                    <section>
                      <h2>Artifacts</h2>
                      <button className={artifactRowClass} onClick={() => { setArtifact(null); setPanel('intent'); }}>
                        <IconFile /><span><strong>Quest intent</strong><small>{selected.intent_path}</small></span>
                      </button>
                      <button className={artifactRowClass} onClick={() => { setArtifact(null); setPanel('spec'); }}>
                        <IconFile /><span><strong>Quest spec</strong><small>{selected.spec_path ?? 'not generated'}</small></span>
                      </button>
                      <button className={artifactRowClass} onClick={() => openArtifact({ kind: 'trace', label: 'Timeline trace', path: selected.trace_path })}>
                        <IconFile /><span><strong>Timeline trace</strong><small>{selected.trace_path}</small></span>
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
                            <small>{checkpoint.workspace_id ?? 'workspace pending'} · {formatTime(checkpoint.timestamp_ms)}</small>
                          </span>
                        </button>
                      ))}
                      {selected.review?.exploration_attempts.map(attempt => (
                        <button
                          className={artifactRowClass}
                          key={attempt.id}
                          onClick={() => openArtifact({ kind: 'exploration', label: attempt.label, path: attempt.artifact_path })}
                        >
                          <IconSparkles /><span><strong>{attempt.label}</strong><small>{attempt.outcome}{attempt.selected ? ' · selected' : ''}</small></span>
                        </button>
                      ))}
                      <button className={artifactRowClass} onClick={() => setPanel('review')} disabled={!selected.review}>
                        <IconCheck /><span><strong>Review bundle</strong><small>{selected.review ? `${selected.review.changed_files.length} changed files` : 'not ready'}</small></span>
                      </button>
                    </section>

                    <section>
                      <h2>Changed files <b>{selected.review?.changed_files.length ?? 0}</b></h2>
                      {selected.review?.changed_files.map(file => (
                        <button
                          className={cn(fileRowClass, 'cursor-pointer')}
                          key={file.path}
                          onClick={() => openArtifact({ kind: 'changed_file', label: file.path, path: file.path })}
                        >
                          <IconFile /><span><strong>{file.path}</strong><small>{file.status}</small></span>
                          <b>+{file.additions} <i>-{file.deletions}</i></b>
                        </button>
                      )) ?? <p className={mutedText}>No file changes yet</p>}
                    </section>

                    <section>
                      <h2>Validation</h2>
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
                              {validation.command && ` · ${validation.policy_approved ? 'policy-approved' : 'unapproved'}: ${validation.command}`}
                              {validation.log && ' · log attached'}
                            </small>
                          </span>
                          <b>{validation.status}</b>
                        </button>
                      )) ?? <p className={mutedText}>No validation evidence yet</p>}
                    </section>

                    <section>
                      <h2>References</h2>
                      {selected.attached_knowledge.length === 0 ? (
                        <p className={mutedText}>No Knowledge attached</p>
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
                      <div><span>DURABLE INTENT</span><strong>Edit the task brief before execution or during revision</strong></div>
                      <div>
                        <button
                          className={sectionHeadingButton}
                          onClick={() => onOpenEditor(selected.project.path, artifactFor('intent', 'Quest intent', selected.intent_path))}
                        >
                          <IconCode /> Open Editor
                        </button>
                        <button className={sectionHeadingButton} onClick={saveIntent} disabled={busy || intentDraft === selected.intent}><IconEdit /> Save</button>
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
                      <div><span>AI TOOL SPEC</span><strong>Review the model-created artifact before execution</strong></div>
                      <div>
                        <button
                          className={sectionHeadingButton}
                          onClick={() => onOpenEditor(selected.project.path, artifactFor('spec', 'Quest spec', selected.spec_path ?? 'spec.md'))}
                        >
                          <IconCode /> Open Editor
                        </button>
                        <button className={sectionHeadingButton} onClick={saveSpec} disabled={busy || specDraft === selected.spec}><IconEdit /> Save</button>
                        <button className={cn(sectionHeadingButton, primaryButton)} onClick={execute} disabled={busy || selected.status === 'archived'}>
                          {busy ? <IconLoader className="animate-spin" /> : <IconPlay />} Approve &amp; execute
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
                      <button className={sectionHeadingButton} onClick={() => setPanel('overview')}><IconChevronRight /> Overview</button>
                      <button className={sectionHeadingButton} onClick={() => onOpenEditor(selected.project.path, artifactFor(artifact.kind, artifact.label, artifact.path))}><IconCode /> Open Editor</button>
                    </header>
                    <div>
                      <span>{artifact.kind.replace('_', ' ')}</span>
                      <h2>{artifact.label}</h2>
                      {artifact.path && <p>{artifact.path}</p>}
                    </div>
                    <pre>{artifact.kind === 'changed_file'
                      ? selected.review?.changed_files.find(file => file.path === artifact.path)?.diff ?? 'No diff available'
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
                        Pending knowledge <b>{knowledge.filter(entry => entry.status === 'pending').length}</b>
                        <button className={sectionHeadingButton} onClick={revalidateKnowledgeEntries} disabled={busy}>Revalidate</button>
                      </h2>
                      {knowledge.filter(entry => entry.status === 'pending').length === 0 ? (
                        <p className={mutedText}>No pending memory proposals</p>
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
                      <h2>Approved knowledge <b>{knowledge.filter(entry => entry.status === 'approved').length}</b></h2>
                      {knowledge.filter(entry => entry.status === 'approved').length === 0 ? (
                        <p className={mutedText}>No approved project knowledge yet</p>
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
                        <strong>No review bundle yet</strong>
                        <span>Approve the spec and execute the Quest first.</span>
                      </div>
                    ) : (
                      <>
                        <div className={questClasses.reviewSummary}>
                          <div><span>RISK</span><strong>{selected.review.risk}</strong></div>
                          <p>{selected.review.summary}</p>
                        </div>
                        <section>
                          <h2>Capability metrics</h2>
                          <div className={questClasses.reviewMetrics}>
                            <div>
                              <span>First action</span>
                              <strong>{formatMetricDuration(selected.review.metrics?.intent_to_first_action_ms)}</strong>
                            </div>
                            <div>
                              <span>Tool latency</span>
                              <strong>{formatMetricDuration(selected.review.metrics?.tool_call_latency_ms)}</strong>
                            </div>
                            <div>
                              <span>Validators</span>
                              <strong>{formatMetricDuration(selected.review.metrics?.validator_turnaround_ms)}</strong>
                            </div>
                            <div>
                              <span>Context relevance</span>
                              <strong>{formatMetricScore(selected.review.metrics?.context_relevance_score)}</strong>
                            </div>
                            <div>
                              <span>Recovery</span>
                              <strong>{formatMetricScore(selected.review.metrics?.failed_action_recovery_rate)}</strong>
                            </div>
                            <div>
                              <span>Evidence quality</span>
                              <strong>{formatMetricScore(selected.review.metrics?.review_evidence_quality_score)}</strong>
                            </div>
                            <div>
                              <span>Attempts</span>
                              <strong>{selected.review.metrics?.isolated_attempt_count ?? 0}</strong>
                            </div>
                            <div>
                              <span>Validation failures</span>
                              <strong>{selected.review.metrics?.validation_failure_count ?? 0}/{selected.review.metrics?.validation_count ?? 0}</strong>
                            </div>
                          </div>
                          {(selected.review.metrics?.notes ?? []).length > 0 && (
                            <p className={questClasses.metricNote}>{selected.review.metrics?.notes?.join(' ')}</p>
                          )}
                        </section>
                        <section>
                          <h2>Unresolved issues</h2>
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
                                <button onClick={() => requestQuickFix(finding.summary)} disabled={busy}>Quick fix</button>
                              </div>
                            ))
                          ) : selected.review.unresolved_issues.length === 0
                            ? <div className={cn(issueClass, clearIssueClass)}><IconCheck /> No unresolved issues</div>
                            : selected.review.unresolved_issues.map(issue => (
                              <div className={issueClass} key={issue}>
                                <button
                                  className={issueOpenClass}
                                  onClick={() => openArtifact({ kind: 'review_finding', label: issue })}
                                >
                                  <IconAlertCircle /> {issue}
                                </button>
                                <button onClick={() => requestQuickFix(issue)} disabled={busy}>Quick fix</button>
                              </div>
                            ))}
                        </section>
                        <section>
                          <h2>Next actions</h2>
                          {(selected.review.next_actions ?? []).length === 0 ? (
                            <p className={mutedText}>No guided next action is attached to this review.</p>
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
                          <h2>Exploration attempts</h2>
                          {selected.review.exploration_attempts.length === 0 ? (
                            <p className={mutedText}>No alternative attempts were preserved for this result.</p>
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
                              <b>{attempt.selected ? 'selected' : attempt.outcome}</b>
                            </button>
                          ))}
                        </section>
                        <section>
                          <h2>Transaction groups</h2>
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
                                    <small>{group.summary} · {group.files.length} file(s) · {group.risk || 'risk pending'}</small>
                                  </span>
                                  <b>+{totals.additions} <i>-{totals.deletions}</i></b>
                                </label>
                              );
                            })
                          ) : selected.review.changed_files.length === 0 ? (
                            <p className={mutedText}>No changed files can be applied.</p>
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
                          <h2>Final decision</h2>
                          <div className={questClasses.decisionRow}>
                            <button
                              className={cn(decisionButtonClass, primaryButton)}
                              onClick={() => selected.review?.transaction_groups.length
                                ? applySelectedQuest(undefined, Array.from(selectedReviewGroups))
                                : applySelectedQuest(Array.from(selectedReviewFiles))
                              }
                              disabled={busy || selected.status !== 'ready_for_review' || (selected.review.transaction_groups.length ? selectedReviewGroups.size === 0 : selectedReviewFiles.size === 0)}
                            >
                              <IconCheck /> Apply selected
                            </button>
                            <button
                              className={decisionButtonClass}
                              onClick={() => applySelectedQuest()}
                              disabled={busy || selected.status !== 'ready_for_review' || selected.review.changed_files.length === 0}
                            >
                              Apply all
                            </button>
                            <button
                              className={decisionButtonClass}
                              onClick={() => selected.review?.transaction_groups.length
                                ? discardSelectedQuest(undefined, Array.from(selectedReviewGroups))
                                : discardSelectedQuest(Array.from(selectedReviewFiles))
                              }
                              disabled={busy || selected.status !== 'ready_for_review' || (selected.review.transaction_groups.length ? selectedReviewGroups.size === 0 : selectedReviewFiles.size === 0)}
                            >
                              <IconX /> Discard selected
                            </button>
                            <button className={decisionButtonClass} onClick={rejectSelectedQuest} disabled={busy || selected.status !== 'ready_for_review'}><IconX /> Reject result</button>
                            <button className={decisionButtonClass} onClick={reviseSelectedQuest} disabled={busy || !['ready_for_review', 'blocked', 'waiting_for_user'].includes(selected.status)}><IconRefresh /> Request revision</button>
                          </div>
                          {selected.decisions.length > 0 && (
                            <div className={questClasses.decisionHistory}>
                              {selected.decisions.map(decision => (
                                <div key={`${decision.kind}-${decision.timestamp_ms}`}>
                                  <small>{decision.kind.replace('_', ' ')} · {decision.summary}</small>
                                  {decision.rollback_id && decision.kind !== 'rollback' && (
                                    <button onClick={() => rollbackSelectedQuest(decision.rollback_id!)} disabled={busy}>
                                      Roll back
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
        <span>Registry: Aster user data · cross-project</span>
        <span>{currentProjectPath ? `Current project: ${currentProjectPath}` : 'No project open'}</span>
      </footer>
      {error && (
        <button className={questClasses.errorToast} onClick={() => setErrorOpen(true)} title="View error details">
          <IconAlertCircle />
          <span>
            <strong>Quest failed</strong>
            <small>{error}</small>
          </span>
          <IconChevronRight />
        </button>
      )}
      {errorOpen && error && (
        <div className={questClasses.errorModal} role="dialog" aria-modal="true" aria-label="Quest error details">
          <div>
            <header>
              <span><IconAlertCircle /> Quest error</span>
              <button className={questClasses.modalButton} onClick={() => setErrorOpen(false)} title="Close"><IconX /></button>
            </header>
            <pre>{error}</pre>
            <footer>
              <button className={questClasses.modalButton} onClick={() => setError(null)}>Dismiss</button>
              <button className={cn(questClasses.modalButton, 'border-[#111827] bg-[#111827] text-white')} onClick={() => setErrorOpen(false)}>Close</button>
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
}: {
  label: string;
  quests: QuestRecord[];
  selectedId?: string;
  onSelect: (quest: QuestRecord) => void;
}) {
  return (
    <section className={questClasses.group}>
      <header><span>{label}</span><b>{quests.length}</b></header>
      {quests.map(quest => (
        <button key={quest.id} className={cn(questClasses.groupButton, selectedId === quest.id && questClasses.groupButtonActive)} onClick={() => onSelect(quest)}>
          <span className={cn('mt-[3px] h-[6px] w-[6px] rounded-full', statusDotClass(quest.status))} />
          <div>
            <strong>{quest.title}</strong>
            <small>{quest.project.name} · {formatTime(quest.updated_at_ms)}</small>
          </div>
        </button>
      ))}
      {quests.length === 0 && <p>None</p>}
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
            {attached ? <IconX /> : <IconPlus />} {attached ? 'Detach' : 'Attach'}
          </button>
        )}
        {entry.status !== 'approved' && <button onClick={onApprove} disabled={busy}><IconCheck /> Approve</button>}
        {entry.status === 'pending' && <button onClick={onReject} disabled={busy}><IconX /> Reject</button>}
        <button onClick={onRemove} disabled={busy}><IconTrash /> Remove</button>
      </footer>
    </article>
  );
}
