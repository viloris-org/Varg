import React, { useCallback, useContext, useEffect, useMemo, useRef, useState, createContext } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { vscDarkPlus } from 'react-syntax-highlighter/dist/esm/styles/prism';
import { rpc, streamCopilotPlan } from '../api';
import { useTranslation } from '../i18n';
import { listKnowledge, promoteQuest, type KnowledgeEntry } from '../quest';
import { safeJsonStringify } from '../safeJson';
import {
  aiEntityContextCompBadgeClass,
  aiPlanBadgeClass,
  aiPlanItemButtonClass,
  buttonClass,
} from '../uiClasses';
import {
  IconSend, IconBot, IconCheck, IconX, IconAlertCircle,
  IconChevronDown, IconChevronRight, IconInfo, IconLoader,
  IconSave, IconUndo, IconPlay, IconStop, IconSettings, IconSparkles, IconRefresh,
  IconBrain, IconFile, IconHand, IconShield, IconShieldCheck, IconNewChat,
} from '../icons';

const cls = (...parts: Array<string | false | null | undefined>) => parts.filter(Boolean).join(' ');

const panelIconButtonClass = 'flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded border-0 bg-transparent text-[var(--text-muted)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]';
const contextTagClass = 'rounded-[10px] bg-[var(--bg-base)] px-2 py-0.5 text-[11px] text-[var(--text-secondary)]';
const contextSelectedTagClass = 'bg-[var(--accent)] text-white';
const contextKnowledgeTagClass = 'border border-[rgba(34,197,94,0.22)] bg-[rgba(34,197,94,0.08)] text-[#86efac]';
const evidenceToggleClass = 'flex w-fit cursor-pointer items-center gap-1 rounded border-0 bg-transparent px-0 py-0 text-[11px] font-medium text-[var(--text-muted)] hover:text-[var(--text-primary)]';
const toolCallBaseClass = 'flex items-center gap-1.5 rounded-md px-2.5 py-1 font-[var(--font-mono)] text-xs';
const toolCallClass = (complete: boolean) => cls(toolCallBaseClass, complete ? 'border border-[rgba(34,197,94,0.2)] bg-[rgba(34,197,94,0.1)] text-[#4ade80]' : 'border border-[var(--border-light)] bg-[var(--accent-dim)] text-[var(--text-secondary)]');
const messageClass = (role: AiMessage['role']) => cls('flex gap-2', role === 'user' && 'flex-row-reverse');
const assistantMarkdownClass = [
  '[&_h1]:my-[0.6em] [&_h1]:mb-[0.3em] [&_h1]:text-[1.2em] [&_h1]:font-semibold [&_h1]:leading-[1.3]',
  '[&_h2]:my-[0.6em] [&_h2]:mb-[0.3em] [&_h2]:text-[1.1em] [&_h2]:font-semibold [&_h2]:leading-[1.3]',
  '[&_h3]:my-[0.6em] [&_h3]:mb-[0.3em] [&_h3]:text-[1em] [&_h3]:font-semibold [&_h3]:leading-[1.3]',
  '[&_h4]:my-[0.6em] [&_h4]:mb-[0.3em] [&_h4]:font-semibold [&_h4]:leading-[1.3]',
  '[&_p]:my-[0.4em] [&_ul]:my-[0.4em] [&_ol]:my-[0.4em] [&_ul]:pl-[1.5em] [&_ol]:pl-[1.5em] [&_li]:my-[0.2em]',
  '[&_code:not(pre_code)]:rounded [&_code:not(pre_code)]:bg-[var(--bg-tertiary,rgba(255,255,255,0.08))] [&_code:not(pre_code)]:px-[5px] [&_code:not(pre_code)]:py-px [&_code:not(pre_code)]:font-[var(--font-mono)] [&_code:not(pre_code)]:text-[0.9em]',
  '[&_pre]:my-[0.5em] [&_pre]:overflow-x-auto [&_pre]:rounded-md',
  '[&_blockquote]:my-[0.5em] [&_blockquote]:border-l-[3px] [&_blockquote]:border-[var(--border)] [&_blockquote]:py-[0.2em] [&_blockquote]:pr-[0.8em] [&_blockquote]:pl-[0.8em] [&_blockquote]:text-[var(--text-secondary)]',
  '[&_a]:text-[var(--accent)] [&_a]:no-underline hover:[&_a]:underline',
  '[&_table]:my-[0.5em] [&_table]:block [&_table]:max-w-full [&_table]:overflow-x-auto [&_table]:border-collapse [&_table]:text-[0.9em] [&_table]:whitespace-normal',
  '[&_th]:border [&_th]:border-[var(--border)] [&_th]:bg-[var(--bg-tertiary,rgba(255,255,255,0.05))] [&_th]:px-2 [&_th]:py-1 [&_th]:align-top',
  '[&_td]:border [&_td]:border-[var(--border)] [&_td]:px-2 [&_td]:py-1 [&_td]:align-top',
  '[&_hr]:my-[0.8em] [&_hr]:border-0 [&_hr]:border-t [&_hr]:border-[var(--border)]',
].join(' ');
const messageContentClass = (role: AiMessage['role']) => cls('rounded-xl px-3 py-2 text-[13px] leading-normal', role === 'user' ? 'rounded-br bg-[var(--accent-strong)] text-white' : `rounded-bl bg-[var(--bg-secondary)] text-[var(--text-primary)] ${assistantMarkdownClass}`);
const thinkingHeaderClass = 'flex w-full cursor-pointer items-center gap-1.5 border-0 bg-transparent px-2.5 py-1.5 text-left font-[var(--font-sans)] text-[11px] font-medium text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] [&_svg]:shrink-0 [&_svg]:opacity-70';
const cardHeaderClass = 'flex w-full cursor-pointer items-center gap-1.5 border-0 bg-[var(--bg-secondary)] px-2.5 py-1.5 text-left text-[11px] text-[var(--text-secondary)]';
const planItemStateClass = (state: 'allowed' | 'denied' | 'auto') => cls('shrink-0 rounded px-1.5 py-0.5 text-[10px] font-semibold', state === 'allowed' && 'bg-[rgba(16,185,129,0.12)] text-[#10b981]', state === 'denied' && 'bg-[rgba(239,68,68,0.1)] text-[#ef4444]', state === 'auto' && 'bg-[rgba(148,163,184,0.1)] text-[var(--text-muted)]');
const traceResultClass = (result: string) => result === 'applied' ? 'text-[#10b981]' : 'text-[#ef4444]';
const consoleLevelClass = (level: string) => cls('font-bold uppercase text-[var(--text-secondary)]', level === 'error' && 'text-[#ef4444]', (level === 'warn' || level === 'warning') && 'text-[#f59e0b]');
const permissionStateClass = (allowed: boolean) => cls('text-[9px] font-semibold', allowed ? 'text-[#22c55e]' : 'text-[#f87171]');
const permissionButtonClass = 'cursor-pointer rounded border border-[var(--border)] bg-[var(--bg-surface)] px-[7px] py-1 font-[var(--font-sans)] text-[9px] font-medium text-[var(--text-secondary)] hover:border-[var(--border-light)] hover:text-[var(--text-primary)]';
const changeKindClass = (kind: CopilotOperation['permission_kind']) => cls('rounded-[3px] px-1 py-0.5 text-center font-[var(--font-mono)] text-[10px] font-bold', kind === 'write' && 'bg-[rgba(245,158,11,0.14)] text-[#f59e0b]', kind === 'read' && 'bg-[rgba(34,197,94,0.12)] text-[#22c55e]', kind === 'command' && 'bg-[var(--accent-dim)] text-[var(--text-secondary)]', kind === 'unsupported' && 'bg-[rgba(239,68,68,0.12)] text-[#f87171]');
const workflowStepClass = (active = false) => cls('rounded-[5px] px-[3px] py-1.5 text-[10px] font-medium text-[var(--text-muted)]', active && 'bg-[var(--accent-dim)] text-[var(--accent)]');
const mentionItemClass = (active: boolean) => cls('flex w-full cursor-pointer items-center gap-2 border-0 bg-transparent px-2.5 py-1.5 text-left font-[var(--font-sans)] text-xs text-[var(--text-primary)] hover:bg-[var(--accent-dim)]', active && 'bg-[var(--accent-dim)]');
const messageStateClass = (state: 'queued' | 'interrupted') => cls('mt-1.5 w-fit rounded px-1.5 py-0.5 text-[9px] font-semibold', state === 'queued' ? 'bg-[rgba(245,158,11,0.12)] text-[#fbbf24]' : 'bg-[rgba(148,163,184,0.12)] text-[#94a3b8]');
const workspaceTabClass = (active: boolean) => cls('min-w-[72px] cursor-pointer rounded border-0 bg-transparent px-2.5 text-[11px] text-[var(--text-secondary)]', active && 'bg-[var(--bg-active)] text-[var(--text-primary)]');
const commonSpinnerClass = 'animate-spin';

// ─── Types ──────────────────────────────────────────────────────────────────

export interface CopilotOperation {
  index: number;
  preview: string;
  requires_write: boolean;
  id?: string;
  kind?: string;
  permission_kind: 'read' | 'write' | 'command' | 'unsupported';
  target?: string;
  risk_hint?: 'low' | 'medium' | 'high' | 'unsupported' | string;
  requires_approval?: boolean;
  undo_hint?: 'available' | 'unavailable' | 'unknown' | string;
  validation_hint?: string | null;
  command?: string | null;
  permanently_allowed?: boolean;
}

export interface CopilotPlan {
  message: string;
  operations: CopilotOperation[];
  read_only: boolean;
  requires_write: boolean;
  knowledge_entries_used?: number;
  completed?: boolean;
  needs_continuation?: boolean;
  continuation_reason?: string | null;
  task_updates?: CopilotTaskUpdate[];
}

export interface TraceEntry {
  tool: string;
  result: string;
  recovery_hint: string;
}

interface ConsoleEntry {
  level: string;
  message: string;
  subsystem: string;
}

interface ApplyResult {
  operations_performed: number;
  completed: boolean;
  summary: string | null;
  trace_entries: TraceEntry[];
  console_entries: ConsoleEntry[];
  undo_available?: boolean;
  undo_label?: string | null;
  needs_continuation?: boolean;
  continuation_reason?: string | null;
  task_updates?: CopilotTaskUpdate[];
}

interface CopilotTask {
  id: string;
  title: string;
  done: boolean;
}

interface CopilotTaskUpdate {
  id: string;
  title?: string | null;
  done?: boolean | null;
}

interface UndoResult {
  applied: boolean;
  summary: string;
  trace_entries: TraceEntry[];
}

export interface CompletedChangeBundle {
  summary: string;
  operationsPerformed: number;
  traceEntries: TraceEntry[];
  consoleEntries: ConsoleEntry[];
  undoAvailable: boolean;
  undoLabel: string | null;
}

export type AiStatus = 'idle' | 'thinking' | 'ready' | 'executing' | 'complete' | 'error';
type AiWorkspaceView = 'chat' | 'changes';
type ThinkingEffort = 'off' | 'low' | 'medium' | 'high';
type ApprovalMode = 'ask' | 'auto-safe' | 'full-access';

interface AiMessage {
  id: string;
  role: 'user' | 'assistant' | 'system';
  content: string;
  thinking?: string;
  /** Inline tool result cards */
  cards?: AiCard[];
  /** Active tool calls being constructed during streaming */
  activeToolCalls?: ActiveToolCall[];
  timestamp: number;
  queued?: boolean;
  interrupted?: boolean;
}

interface QueuedPrompt {
  id: string;
  prompt: string;
}

interface AiCard {
  type: 'plan' | 'trace' | 'console' | 'graph' | 'entity-list' | 'error';
  data: any;
}

interface ActiveToolCall {
  id: string;
  name: string;
  argumentsPreview: string;
  complete: boolean;
}

interface ParsedToolContent {
  content: string;
  toolCalls: ActiveToolCall[];
}

function objectEnd(source: string, start: number): number {
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let i = start; i < source.length; i += 1) {
    const char = source[i];
    if (inString) {
      if (escaped) {
        escaped = false;
      } else if (char === '\\') {
        escaped = true;
      } else if (char === '"') {
        inString = false;
      }
      continue;
    }
    if (char === '"') {
      inString = true;
    } else if (char === '{') {
      depth += 1;
    } else if (char === '}') {
      depth -= 1;
      if (depth === 0) return i + 1;
    }
  }
  return -1;
}

function linePrefixStart(source: string, objectStart: number): number {
  const lineStart = Math.max(source.lastIndexOf('\n', objectStart - 1) + 1, 0);
  const prefix = source.slice(lineStart, objectStart);
  return /^[A-Za-z0-9_-]{1,8}$/.test(prefix) ? lineStart : objectStart;
}

function argumentsPreviewFromValue(value: unknown): string {
  if (typeof value === 'string') return value;
  if (value && typeof value === 'object') return safeJsonStringify(value);
  return '';
}

function toolCallFromObject(value: unknown, fallbackIndex: number): ActiveToolCall | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  if (typeof record.name === 'string' && ('arguments_delta' in record || 'arguments' in record)) {
    return {
      id: typeof record.id === 'string' && record.id ? record.id : `embedded-tool-${fallbackIndex}`,
      name: record.name,
      argumentsPreview: argumentsPreviewFromValue(record.arguments_delta ?? record.arguments),
      complete: true,
    };
  }
  if (typeof record.action === 'string') {
    const action = record.action;
    const isToolAction = /^(read_|write_|create_|update_|delete_|apply_|run_|execute_|inspect_|diagnose_)/.test(action)
      || action === 'write_script'
      || action === 'create_scene_object'
      || action === 'set_component';
    const hasToolShape = 'path' in record || 'content' in record || 'target' in record || 'command' in record || 'args' in record;
    if (isToolAction && hasToolShape) {
      return {
        id: `embedded-tool-${fallbackIndex}`,
        name: action,
        argumentsPreview: safeJsonStringify(record),
        complete: true,
      };
    }
  }
  return null;
}

function extractEmbeddedToolCalls(content: string): ParsedToolContent {
  const toolCalls: ActiveToolCall[] = [];
  let cleaned = '';
  let cursor = 0;
  let inFence = false;
  const lines = content.split(/(\n)/);
  const visible = lines.join('');

  while (cursor < visible.length) {
    const fenceAt = visible.indexOf('```', cursor);
    const objectAt = visible.indexOf('{', cursor);
    if (objectAt === -1) {
      cleaned += visible.slice(cursor);
      break;
    }
    if (fenceAt !== -1 && fenceAt < objectAt) {
      const fenceEnd = visible.indexOf('```', fenceAt + 3);
      const end = fenceEnd === -1 ? visible.length : fenceEnd + 3;
      cleaned += visible.slice(cursor, end);
      cursor = end;
      inFence = !inFence && fenceEnd === -1;
      continue;
    }
    if (inFence) {
      cleaned += visible.slice(cursor, objectAt + 1);
      cursor = objectAt + 1;
      continue;
    }

    const end = objectEnd(visible, objectAt);
    if (end === -1) {
      cleaned += visible.slice(cursor);
      break;
    }
    const objectText = visible.slice(objectAt, end);
    let parsed: unknown;
    try {
      parsed = JSON.parse(objectText);
    } catch {
      cleaned += visible.slice(cursor, objectAt + 1);
      cursor = objectAt + 1;
      continue;
    }
    const toolCall = toolCallFromObject(parsed, toolCalls.length);
    if (!toolCall) {
      cleaned += visible.slice(cursor, objectAt + 1);
      cursor = objectAt + 1;
      continue;
    }
    const removeStart = linePrefixStart(visible, objectAt);
    cleaned += visible.slice(cursor, removeStart);
    toolCalls.push(toolCall);
    cursor = end;
  }

  return {
    content: cleaned.replace(/\n{3,}/g, '\n\n').trimEnd(),
    toolCalls,
  };
}

function appendToolCallDelta(toolCalls: ActiveToolCall[], delta: string): ActiveToolCall[] {
  try {
    const tcDelta = JSON.parse(delta);
    const next = [...toolCalls];
    if (tcDelta.name) {
      next.push({
        id: tcDelta.id || `tc-${next.length}`,
        name: tcDelta.name,
        argumentsPreview: '',
        complete: false,
      });
    }
    if (tcDelta.arguments_delta && next.length > 0) {
      const last = next[next.length - 1];
      next[next.length - 1] = {
        ...last,
        argumentsPreview: last.argumentsPreview + tcDelta.arguments_delta,
      };
    }
    return next;
  } catch {
    return toolCalls;
  }
}

// ─── Context Bar ────────────────────────────────────────────────────────────

interface EntityDetails {
  id: string;
  name: string;
  tag: string;
  transform: {
    position: [number, number, number];
    rotation: [number, number, number, number];
    scale: [number, number, number];
  };
  components: Array<{ type: string; data: any }>;
}

// ─── Model Selector ─────────────────────────────────────────────────────────

interface ModelInfo {
  id: string;
  display_name: string;
  provider: string;
  context_window: number;
  default_max_tokens: number;
  capabilities: { can_reason: boolean; supports_vision: boolean; supports_tools: boolean };
}

interface ProviderMeta {
  provider: string;
  display_name: string;
  requires_api_key: boolean;
  requires_endpoint: boolean;
  endpoint_configurable: boolean;
  default_endpoint: string | null;
  models: ModelInfo[];
}

interface CopilotSettingsFull {
  provider: string;
  model: string;
  api_endpoint: string | null;
  api_key: string | null;
  max_tokens: number;
  allowed_commands?: string[];
}

function useModelOptions() {
  const [currentModel, setCurrentModel] = useState<string>('');
  const [currentProvider, setCurrentProvider] = useState<string>('stub');
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [discoveryError, setDiscoveryError] = useState<string | null>(null);

  const loadModels = useCallback(async () => {
    setLoading(true);
    setDiscoveryError(null);
    try {
      const settings = await rpc<CopilotSettingsFull>('app/get_copilot_settings').catch(() => null);
      if (!settings) { setLoading(false); return; }
      setCurrentProvider(settings.provider);
      setCurrentModel(settings.provider === 'stub' ? 'none' : settings.model);

      let providerModels: ModelInfo[] = [];

      if (settings.provider !== 'stub') {
        try {
          providerModels = await rpc<ModelInfo[]>('app/detect_models', {
            provider: settings.provider,
          });
        } catch (error) {
          setDiscoveryError(String(error));
        }
      }

      if (providerModels.length === 0 && settings.provider !== 'custom') {
        const reg = await rpc<{ providers: ProviderMeta[] }>('app/get_model_registry').catch(() => ({ providers: [] }));
        providerModels = reg.providers
          .filter(p => p.provider === settings.provider)
          .flatMap(p => p.models);
      }

      setModels(providerModels);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { loadModels(); }, [loadModels]);

  const selectModel = useCallback(async (modelId: string) => {
    setCurrentModel(modelId);
    const settings = await rpc<CopilotSettingsFull>('app/get_copilot_settings').catch(() => null);
    if (settings) {
      const { api_key: _apiKey, ...payload } = settings;
      await rpc('app/update_copilot_settings', { ...payload, model: modelId });
    }
  }, []);

  return {
    currentModel,
    currentProvider,
    models,
    loading,
    discoveryError,
    loadModels,
    selectModel,
  };
}

function CompactModelMenuItem({
  active,
  title,
  description,
  onClick,
}: {
  active: boolean;
  title: string;
  description?: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={cls(
        'grid min-h-7 w-full cursor-pointer grid-cols-[minmax(0,1fr)_16px] items-center gap-2 border-0 bg-transparent px-2.5 py-1 text-left font-[var(--font-sans)] text-[11px] text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]',
        active && 'text-[var(--text-primary)]',
      )}
      onClick={onClick}
    >
      <span className="grid min-w-0 gap-0.5">
        <span className="truncate font-medium">{title}</span>
        {description && <span className="line-clamp-2 text-[10px] leading-[1.35] text-[var(--text-muted)]">{description}</span>}
      </span>
      <span className="grid h-4 w-4 place-items-center text-[var(--text-secondary)]">
        {active && <IconCheck size={12} />}
      </span>
    </button>
  );
}

const reasoningMenuOptions: Array<{ value: ThinkingEffort; label: string; titleKey: string }> = [
  { value: 'off', label: 'Off', titleKey: 'thinking_off' },
  { value: 'low', label: 'Low', titleKey: 'thinking_low' },
  { value: 'medium', label: 'Medium', titleKey: 'thinking_medium' },
  { value: 'high', label: 'High', titleKey: 'thinking_high' },
];

function CompactModelMenu({
  thinkingEffort,
  onThinkingEffortChange,
  compact = false,
}: {
  thinkingEffort: ThinkingEffort;
  onThinkingEffortChange: (value: ThinkingEffort) => void;
  compact?: boolean;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const {
    currentModel,
    currentProvider,
    models,
    loading,
    discoveryError,
    loadModels,
    selectModel,
  } = useModelOptions();

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener('pointerdown', onPointerDown);
    return () => document.removeEventListener('pointerdown', onPointerDown);
  }, [open]);

  const currentModelInfo = models.find(model => model.id === currentModel);
  const noProviderConfigured = currentProvider === 'stub';
  const modelLabel = noProviderConfigured ? 'none' : currentModelInfo?.display_name ?? currentModel ?? t('model_none');
  const known = !noProviderConfigured && models.some(model => model.id === currentModel);
  const reasoningLabel = reasoningMenuOptions.find(option => option.value === thinkingEffort)?.label ?? 'Medium';

  return (
    <div ref={menuRef} className="relative min-w-0">
      <button
        type="button"
        className={cls(
          'flex h-7 max-w-[220px] cursor-pointer items-center gap-1.5 rounded-md border border-[var(--border)] bg-[rgba(255,255,255,0.04)] px-2 text-[11px] text-[var(--text-primary)] transition-colors hover:border-[var(--border-light)] hover:bg-[var(--bg-hover)] focus:outline-none focus:ring-2 focus:ring-[var(--accent-dim)]',
          compact && 'border-0 bg-transparent px-1 text-[var(--text-secondary)] hover:bg-[rgba(255,255,255,0.05)] hover:text-[var(--text-primary)]',
        )}
        onClick={() => setOpen(value => !value)}
        title={`${modelLabel || t('model_none')} / ${t('thinking_effort_title')}: ${reasoningLabel}`}
        aria-haspopup="menu"
        aria-expanded={open}
      >
        <IconBrain size={12} />
        <span className="min-w-0 truncate">{modelLabel || t('model_none')}</span>
        <span className="shrink-0 text-[var(--text-muted)]">{reasoningLabel}</span>
        <IconChevronDown size={12} />
      </button>

      {open && (
        <div className="absolute bottom-full left-0 z-[120] mb-2 w-[208px] overflow-hidden rounded-lg border border-[var(--border)] bg-[var(--bg-surface)] py-1 shadow-[0_18px_48px_rgba(0,0,0,0.42)]" role="menu">
          <section className="py-1">
            <div className="px-2.5 pb-1 text-[10px] text-[var(--text-muted)]">Reasoning</div>
            <div className="grid">
              {reasoningMenuOptions.map(option => {
                const active = option.value === thinkingEffort;
                return (
                  <button
                    key={option.value}
                    type="button"
                    className={cls(
                      'grid h-7 cursor-pointer grid-cols-[minmax(0,1fr)_16px] items-center gap-2 border-0 bg-transparent px-2.5 text-left font-[var(--font-sans)] text-[12px] text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]',
                      active && 'text-[var(--text-primary)]',
                    )}
                    onClick={() => onThinkingEffortChange(option.value)}
                    title={t(option.titleKey)}
                    aria-pressed={active}
                  >
                    <span>{option.label}</span>
                    <span className="grid h-4 w-4 place-items-center text-[var(--text-secondary)]">
                      {active && <IconCheck size={12} />}
                    </span>
                  </button>
                );
              })}
            </div>
          </section>
          <div className="mx-2 h-px bg-[var(--border)]" />
          <section className="py-1">
            <div className="flex items-center justify-between gap-2 px-2.5 pb-1">
              <div className="text-[10px] text-[var(--text-muted)]">Model</div>
              <button
                type="button"
                className="flex h-5 cursor-pointer items-center gap-1 rounded border-0 bg-transparent px-1 text-[10px] text-[var(--text-muted)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
                onClick={loadModels}
                title={t('model_refresh')}
              >
                <IconRefresh size={10} />
                <span>{t('model_refresh_short')}</span>
              </button>
            </div>
            <div className="max-h-[196px] overflow-y-auto">
              {loading && (
                <div className="flex items-center gap-1.5 px-2.5 py-2 text-[11px] text-[var(--text-muted)]">
                  <IconLoader className={commonSpinnerClass} size={12} />
                  <span>{t('model_loading')}</span>
                </div>
              )}
              {!loading && (noProviderConfigured || (models.length === 0 && !currentModel)) && (
                <div className="px-2.5 py-2 text-[11px] text-[var(--text-muted)]">none</div>
              )}
              {!loading && !noProviderConfigured && models.map(model => (
                <CompactModelMenuItem
                  key={model.id}
                  active={model.id === currentModel}
                  title={model.display_name}
                  description={model.provider}
                  onClick={() => {
                    selectModel(model.id);
                    setOpen(false);
                  }}
                />
              ))}
              {!loading && !known && currentModel && (
                <CompactModelMenuItem
                  active
                  title={currentModel}
                  description={t('model_custom')}
                  onClick={() => undefined}
                />
              )}
              {discoveryError && (
                <div className="mx-2 mt-1 border-t border-[var(--border)] px-0 py-1.5 text-[10px] leading-[1.35] text-[var(--warning)]" title={discoveryError}>
                  {t('model_discovery_failed')}
                </div>
              )}
            </div>
          </section>
        </div>
      )}
    </div>
  );
}

// ─── Context Bar ────────────────────────────────────────────────────────────

function ContextBar({ projectName, selectedEntity, sceneObjectCount, onSettingsClick, onNewChat, conversationTurns, attachedKnowledgeCount }: {
  projectName?: string;
  selectedEntity?: string | null;
  sceneObjectCount: number;
  onSettingsClick?: () => void;
  onNewChat?: () => void;
  conversationTurns: number;
  attachedKnowledgeCount: number;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex min-h-[42px] items-center gap-1.5 border-b border-[var(--border)] bg-[var(--bg-surface)] px-2.5 py-[7px]">
      {projectName && <span className={contextTagClass}>{projectName}</span>}
      <span className={contextTagClass}>{sceneObjectCount} {t('label_objects')}</span>
      {selectedEntity && (
        <span className={cls(contextTagClass, contextSelectedTagClass)}>@ {selectedEntity}</span>
      )}
      {attachedKnowledgeCount > 0 && (
        <span className={cls(contextTagClass, contextKnowledgeTagClass)}>{attachedKnowledgeCount} Knowledge</span>
      )}
      {conversationTurns > 0 && (
        <span className={cls(contextTagClass, "max-[1050px]:hidden")}>{conversationTurns} {conversationTurns !== 1 ? t('label_turns') : t('label_turn')}</span>
      )}
      <div className="ml-auto flex items-center gap-0.5">
        <button className={panelIconButtonClass} onClick={onNewChat} title={t('ai_new_chat')} aria-label={t('ai_new_chat')}>
          <IconNewChat />
        </button>
        <button className={panelIconButtonClass} onClick={onSettingsClick} title={t('ai_settings')} aria-label={t('ai_settings')}>
          <IconSettings />
        </button>
      </div>
    </div>
  );
}

// ─── Entity Context Card ───────────────────────────────────────────────────

function EntityContextCard({ entity }: { entity: EntityDetails }) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const pos = entity.transform.position;
  const comps = entity.components;

  return (
    <div className="border-b border-[var(--border)] bg-[var(--bg-hover)]">
      <button className="flex w-full cursor-pointer items-center gap-1.5 border-0 bg-transparent px-2.5 py-1.5 text-left font-[var(--font-sans)] text-xs text-[var(--text-primary)] hover:bg-[var(--bg-active)] [&_svg]:h-3 [&_svg]:w-3 [&_svg]:shrink-0 [&_svg]:opacity-60" onClick={() => setExpanded(!expanded)}>
        {expanded ? <IconChevronDown /> : <IconChevronRight />}
        <span className="font-semibold text-[var(--accent)]">{entity.name}</span>
        <span className="rounded-[3px] bg-[var(--bg-surface)] px-[5px] py-px text-[10px] text-[var(--text-muted)]">{entity.tag || t('entity_untagged')}</span>
        <span className="ml-auto text-[10px] text-[var(--text-muted)]">{comps.length} {comps.length !== 1 ? t('label_comps') : t('label_comp')}</span>
      </button>
      {expanded && (
        <div className="flex flex-col gap-[3px] py-1 pr-2.5 pb-2 pl-7">
          <div className="flex items-center gap-1.5 text-[11px]">
            <span className="min-w-[55px] text-[var(--text-muted)]">{t('prop_position')}</span>
            <span className="font-[var(--font-mono)] text-[10px] text-[var(--text-secondary)]">
              {pos[0].toFixed(2)}, {pos[1].toFixed(2)}, {pos[2].toFixed(2)}
            </span>
          </div>
          {comps.map((c, i) => (
            <div key={i} className="flex items-center gap-1.5 text-[11px]">
              <span className={aiEntityContextCompBadgeClass}>{c.type}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ─── Message Bubble ─────────────────────────────────────────────────────────

function ToolCallIndicator({ toolCalls }: { toolCalls: ActiveToolCall[] }) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const pending = toolCalls.some(tc => !tc.complete);
  return (
    <div className="mb-1.5 flex flex-col gap-1">
      <button className={evidenceToggleClass} onClick={() => setExpanded(open => !open)}>
        {expanded ? <IconChevronDown size={12} /> : <IconChevronRight size={12} />}
        <span>{pending ? t('queue_responding') : 'Evidence'}</span>
      </button>
      {expanded && toolCalls.map((tc, i) => (
        <div key={i} className={toolCallClass(tc.complete)}>
          <span className={cls("shrink-0 text-[11px]", !tc.complete && commonSpinnerClass)}>{tc.complete ? '✓' : '⟳'}</span>
          <span className="font-semibold">{tc.name}</span>
          {!tc.complete && <span className="opacity-70 italic">{t('tool_calling')}</span>}
          {tc.complete && tc.argumentsPreview && (
            <span className="overflow-hidden text-ellipsis whitespace-nowrap opacity-80">
              {(() => {
                try {
                  const args = JSON.parse(tc.argumentsPreview);
                  const keys = Object.keys(args);
                  if (keys.length === 0) return null;
                  const fullJson = safeJsonStringify(args, 2);
                  const preview = keys.map(k => {
                    const v = args[k];
                    const display = typeof v === 'string' ? `"${v.slice(0, 30)}${v.length > 30 ? '...' : ''}"` : safeJsonStringify(v);
                    return `${k}: ${display}`;
                  }).join(', ');
                  return <span className="text-[11px]" title={fullJson}>({preview})</span>;
                } catch {
                  return <span className="text-[11px]">({t('tool_parsing')})</span>;
                }
              })()}
            </span>
          )}
        </div>
      ))}
    </div>
  );
}

function MessageBubble({ msg }: { msg: AiMessage }) {
  const { t } = useTranslation();
  const [thinkingExpanded, setThinkingExpanded] = useState(false);

  return (
    <div className={messageClass(msg.role)}>
      <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-[var(--bg-secondary)] text-xs">
        {msg.role === 'assistant' ? <IconBot /> : <span>U</span>}
      </div>
      <div className="flex max-w-[85%] flex-col gap-1.5">
        {msg.thinking && (
          <div className="mb-2 overflow-hidden rounded-[7px] border border-[var(--border)] bg-[var(--bg-surface)]">
            <button
              className={thinkingHeaderClass}
              onClick={() => setThinkingExpanded(!thinkingExpanded)}
            >
              {thinkingExpanded ? <IconChevronDown size={12} /> : <IconChevronRight size={12} />}
              <IconBrain size={12} />
              <span>Thinking</span>
            </button>
            {thinkingExpanded && (
              <div className="whitespace-pre-wrap border-t border-[var(--border)] px-2.5 py-2 text-xs leading-normal text-[var(--text-secondary)]">{msg.thinking}</div>
            )}
          </div>
        )}
        {msg.activeToolCalls && msg.activeToolCalls.length > 0 && (
          <ToolCallIndicator toolCalls={msg.activeToolCalls} />
        )}
        <div className={messageContentClass(msg.role)}>
          {msg.role === 'assistant' ? (
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={{
                code({ className, children, ...props }) {
                  const match = /language-(\w+)/.exec(className || '');
                  const codeString = String(children).replace(/\n$/, '');
                  if (match) {
                    return (
                      <div className="group relative my-[0.5em]">
                        <button
                          className="absolute top-1.5 right-1.5 z-[1] cursor-pointer rounded border border-[var(--border)] bg-[var(--bg-secondary)] px-2 py-0.5 text-[10px] text-[var(--text-secondary)] opacity-0 transition-opacity duration-150 group-hover:opacity-100 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
                          onClick={() => navigator.clipboard.writeText(codeString)}
                          title={t('btn_copy')}
                        >
                          {t('btn_copy')}
                        </button>
                        <SyntaxHighlighter
                          style={vscDarkPlus}
                          language={match[1]}
                          PreTag="div"
                          customStyle={{ margin: '0', borderRadius: '6px', fontSize: '0.85em' }}
                        >
                          {codeString}
                        </SyntaxHighlighter>
                      </div>
                    );
                  }
                  return <code className={className} {...props}>{children}</code>;
                },
              }}
            >
              {msg.content}
            </ReactMarkdown>
          ) : (
            msg.content
          )}
        </div>
        {msg.queued && <div className={messageStateClass('queued')}>{t('msg_queued')}</div>}
        {msg.interrupted && <div className={messageStateClass('interrupted')}>{t('msg_interrupted')}</div>}
        {msg.cards && msg.cards.map((card, i) => (
          <InlineCard key={i} card={card} />
        ))}
      </div>
    </div>
  );
}

// ─── Plan Approval Context (lets PlanCard access approval callbacks) ─────────

interface PlanApprovalCtx {
  approved: Set<number>;
  denied: Set<number>;
  decideOperation: (op: CopilotOperation, decision: 'once' | 'session' | 'always' | 'deny') => void;
  active: boolean; // false when plan is already executed / no longer pending
}

const PlanApprovalContext = createContext<PlanApprovalCtx | null>(null);

const approvalModeStorageKey = 'varg.aiApprovalMode';

function readStoredApprovalMode(): ApprovalMode {
  try {
    const stored = localStorage.getItem(approvalModeStorageKey);
    if (stored === 'auto-safe' || stored === 'full-access') return stored;
  } catch { /* localStorage unavailable */ }
  return 'ask';
}

function approvalModeIcon(mode: ApprovalMode) {
  if (mode === 'full-access') return <IconShieldCheck size={13} />;
  if (mode === 'auto-safe') return <IconShield size={13} />;
  return <IconHand size={13} />;
}

function ApprovalModeMenu({
  mode,
  onChange,
}: {
  mode: ApprovalMode;
  onChange: (mode: ApprovalMode) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const options: Array<{ mode: ApprovalMode; title: string; desc: string }> = [
    { mode: 'ask', title: t('approval_mode_ask'), desc: t('approval_mode_ask_desc') },
    { mode: 'auto-safe', title: t('approval_mode_auto_safe'), desc: t('approval_mode_auto_safe_desc') },
    { mode: 'full-access', title: t('approval_mode_full_access'), desc: t('approval_mode_full_access_desc') },
  ];
  const selected = options.find(option => option.mode === mode) ?? options[0];

  return (
    <div className="relative">
      <button
        type="button"
        className="flex h-7 cursor-pointer items-center gap-1.5 rounded-md border-0 bg-transparent px-1.5 text-[11px] font-medium text-[var(--warning)] hover:bg-[var(--bg-hover)]"
        onClick={() => setOpen(current => !current)}
        aria-haspopup="menu"
        aria-expanded={open}
        title={selected.desc}
      >
        {approvalModeIcon(mode)}
        <span className="max-w-[124px] truncate">{selected.title}</span>
        <IconChevronDown size={12} />
      </button>
      {open && (
        <div
          className="absolute bottom-[calc(100%+6px)] left-0 z-[120] w-[388px] max-w-[calc(100vw-32px)] rounded-xl border border-[var(--border)] bg-[#171716] p-1.5 shadow-[0_18px_50px_rgba(0,0,0,0.42)]"
          role="menu"
        >
          <div className="flex items-center justify-between px-2 py-1.5 text-[11px] text-[var(--text-muted)]">
            <span>{t('approval_mode_prompt')}</span>
            <button
              type="button"
              className="cursor-pointer border-0 bg-transparent p-0 text-[11px] text-[var(--text-secondary)] underline underline-offset-2 hover:text-[var(--text-primary)]"
              onClick={() => setOpen(false)}
            >
              {t('approval_mode_learn_more')}
            </button>
          </div>
          <div className="grid gap-0.5">
            {options.map(option => (
              <button
                key={option.mode}
                type="button"
                className="grid cursor-pointer grid-cols-[22px_minmax(0,1fr)_18px] items-center gap-2 rounded-lg border-0 bg-transparent px-2 py-2 text-left font-[var(--font-sans)] hover:bg-[var(--bg-hover)]"
                onClick={() => {
                  onChange(option.mode);
                  setOpen(false);
                }}
                role="menuitemradio"
                aria-checked={mode === option.mode}
              >
                <span className="text-[var(--text-muted)]">{approvalModeIcon(option.mode)}</span>
                <span className="min-w-0">
                  <span className="block truncate text-[12px] font-medium text-[var(--text-primary)]">{option.title}</span>
                  <span className="block truncate text-[11px] text-[var(--text-muted)]">{option.desc}</span>
                </span>
                {mode === option.mode && <IconCheck size={14} className="text-[var(--text-secondary)]" />}
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

// ─── Inline Cards ───────────────────────────────────────────────────────────

function InlineCard({ card }: { card: AiCard }) {
  const [expanded, setExpanded] = useState(card.type === 'plan');
  const label = card.type === 'trace' || card.type === 'console' ? 'Evidence' : card.type;

  return (
    <div className="overflow-hidden rounded-lg border border-[var(--border)]">
      <button className={cardHeaderClass} onClick={() => setExpanded(!expanded)}>
        {expanded ? <IconChevronDown /> : <IconChevronRight />}
        <span className="capitalize">{label}</span>
      </button>
      {expanded && (
        <div className="px-2.5 py-2">
          {card.type === 'plan' && <PlanCard data={card.data} />}
          {card.type === 'trace' && <TraceCard data={card.data} />}
          {card.type === 'console' && <ConsoleCard data={card.data} />}
          {card.type === 'error' && <ErrorCard data={card.data} />}
          {card.type === 'entity-list' && <EntityListCard data={card.data} />}
        </div>
      )}
    </div>
  );
}

function PlanCard({ data }: { data: CopilotPlan }) {
  const { t } = useTranslation();
  const ctx = useContext(PlanApprovalContext);

  return (
    <div className="">
      {data.operations.map((op) => {
        const isRead = op.permission_kind === 'read';
        const isUnsupported = op.permission_kind === 'unsupported';
        const isAutoAllowed = op.requires_approval === false;
        const isApproved = ctx?.approved.has(op.index);
        const isDenied = ctx?.denied.has(op.index);
        const showControls = ctx?.active && !isRead && !isUnsupported && !isAutoAllowed;

        return (
          <div key={op.index} className="flex min-h-7 items-center gap-1.5 py-[5px] text-xs">
            <span className={aiPlanBadgeClass(op.requires_write ? 'write' : 'read')}>
              {op.permission_kind === 'read' ? 'R' : op.permission_kind === 'command' ? 'CMD' : op.permission_kind === 'unsupported' ? 'BLOCK' : 'W'}
            </span>
            <span className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap">{op.preview}</span>
            {showControls && (
              <div className="flex shrink-0 items-center gap-1">
                {isApproved ? (
                  <span className={planItemStateClass('allowed')}>{t('op_allowed_icon')}</span>
                ) : isDenied ? (
                  <span className={planItemStateClass('denied')}>{t('op_denied_icon')}</span>
                ) : (
                  <>
                    <button
                      className={aiPlanItemButtonClass('allow')}
                      onClick={() => ctx.decideOperation(op, 'once')}
                      title="Allow this operation once"
                    >
                      {t('btn_allow')}
                    </button>
                    <button
                      className={aiPlanItemButtonClass('deny')}
                      onClick={() => ctx.decideOperation(op, 'deny')}
                      title="Deny this operation"
                    >
                      {t('btn_deny')}
                    </button>
                  </>
                )}
              </div>
            )}
            {(isRead || isAutoAllowed) && (
              <span className={planItemStateClass('auto')}>{t('op_auto')}</span>
            )}
            {isUnsupported && (
              <span className={planItemStateClass('denied')}>{t('op_unsupported')}</span>
            )}
          </div>
        );
      })}
    </div>
  );
}

function CopilotTaskCard({
  tasks,
  collapsed,
  onToggle,
}: {
  tasks: CopilotTask[];
  collapsed: boolean;
  onToggle: () => void;
}) {
  const doneCount = tasks.filter(task => task.done).length;
  return (
    <div className="mb-2 overflow-hidden rounded-[8px] border border-[var(--border)] bg-[var(--bg-base)] shadow-[0_8px_24px_rgba(0,0,0,0.14)]">
      <button
        type="button"
        className="flex min-h-[32px] w-full cursor-pointer items-center gap-1.5 border-0 bg-transparent px-2.5 py-1.5 text-left font-[var(--font-sans)] text-[11px] font-semibold text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
        onClick={onToggle}
      >
        {collapsed ? <IconChevronRight size={12} /> : <IconChevronDown size={12} />}
        <IconBot size={12} />
        <span className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap">Tasks</span>
        <span className="rounded-full bg-[var(--accent-dim)] px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--text-muted)]">{doneCount}/{tasks.length}</span>
      </button>
      {!collapsed && (
        <div className="grid gap-px border-t border-[var(--border)] p-1.5">
          {tasks.map(task => (
            <div key={task.id} className="grid min-h-[26px] grid-cols-[16px_minmax(0,1fr)] items-center gap-1.5 rounded-md px-1.5 text-[11px] text-[var(--text-secondary)]">
              <span className={cls(
                'flex h-[13px] w-[13px] items-center justify-center rounded border',
                task.done ? 'border-[#22c55e] bg-[rgba(34,197,94,0.14)] text-[#4ade80]' : 'border-[var(--border-light)] text-transparent',
              )}>
                {task.done && <IconCheck size={10} />}
              </span>
              <span className={cls('min-w-0 overflow-hidden text-ellipsis whitespace-nowrap', task.done && 'text-[var(--text-muted)] line-through')}>
                {task.title}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function TraceCard({ data }: { data: TraceEntry[] }) {
  return (
    <div className="">
      {data.map((entry, i) => (
        <div key={i} className="flex justify-between py-0.5 text-[11px]">
          <span className="">{entry.tool}</span>
          <span className={traceResultClass(entry.result)}>{entry.result}</span>
        </div>
      ))}
    </div>
  );
}

function ConsoleCard({ data }: { data: ConsoleEntry[] }) {
  return (
    <div className="flex max-h-[220px] flex-col gap-1 overflow-auto">
      {data.map((entry, i) => (
        <div key={i} className="grid grid-cols-[52px_86px_minmax(0,1fr)] items-start gap-1.5 border-b border-[var(--border)] py-1 text-[11px] last:border-b-0">
          <span className={consoleLevelClass(entry.level)}>{entry.level}</span>
          <span className="overflow-hidden text-ellipsis whitespace-nowrap text-[var(--text-muted)]">{entry.subsystem}</span>
          <span className="min-w-0 whitespace-pre-wrap break-anywhere">{entry.message}</span>
        </div>
      ))}
    </div>
  );
}

function ErrorCard({ data }: { data: string }) {
  return (
    <div className="flex items-center gap-1.5 text-xs text-[#ef4444]">
      <IconAlertCircle />
      <span>{data}</span>
    </div>
  );
}

function EntityListCard({ data }: { data: Array<{ id: string; name: string }> }) {
  return (
    <div className="flex flex-col gap-0.5">
      {data.map((e) => (
        <div key={e.id} className="flex gap-2 text-xs">
          <span className="font-mono text-[11px] text-[var(--text-secondary)]">{e.id}</span>
          <span className="">{e.name}</span>
        </div>
      ))}
    </div>
  );
}

// ─── Quick Actions ──────────────────────────────────────────────────────────

function QuickActions({ onAction }: { onAction: (action: string) => void }) {
  const { t } = useTranslation();
  return (
    <div className="mb-2 flex items-center gap-1">
      <span className="mr-auto text-[10px] font-semibold uppercase tracking-[0.06em] text-[var(--text-muted)]">{t('ai_workspace_label')}</span>
      <button className="flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-md border border-[var(--border)] bg-[var(--bg-base)] text-sm text-[var(--text-secondary)] hover:border-[var(--border-light)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]" onClick={() => onAction('save')} title={t('command_save')} aria-label={t('command_save')}>
        <IconSave />
      </button>
      <button className="flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-md border border-[var(--border)] bg-[var(--bg-base)] text-sm text-[var(--text-secondary)] hover:border-[var(--border-light)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]" onClick={() => onAction('undo')} title={t('command_undo')} aria-label={t('command_undo')}>
        <IconUndo />
      </button>
      <button className="flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-md border border-[var(--border)] bg-[var(--bg-base)] text-sm text-[var(--text-secondary)] hover:border-[var(--border-light)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]" onClick={() => onAction('play')} title={t('command_play')} aria-label={t('command_play')}>
        <IconPlay />
      </button>
    </div>
  );
}

// ─── Main AI Panel ──────────────────────────────────────────────────────────

export interface AiPanelProps {
  projectName?: string;
  selectedEntity?: string | null;
  selectedEntityName?: string | null;
  sceneObjectCount: number;
  sceneObjects?: Array<{ id: string; name: string }>;
  onQuickAction: (action: string) => void;
  onSceneChanged?: () => void;
  onFocusPosition?: (pos: [number, number, number]) => void;
  chatOnly?: boolean;
  onWorkspaceStateChange?: (state: AiWorkspaceState) => void;
  contextualRequest?: { id: number; prompt: string } | null;
  onContextualRequestConsumed?: (id: number) => void;
  onOpenSettings?: () => void;
  onOpenQuest?: (questId?: string | null) => void;
  compact?: boolean;
}

export interface AiWorkspaceState {
  status: AiStatus;
  plan: CopilotPlan | null;
  approved: Set<number>;
  completedBundle: CompletedChangeBundle | null;
  applyApproved: () => void;
  discardProposal: () => void;
  denied: Set<number>;
}

export default function AiPanel({
  projectName,
  selectedEntity,
  selectedEntityName,
  sceneObjectCount,
  sceneObjects,
  onQuickAction,
  onSceneChanged,
  onFocusPosition,
  chatOnly = false,
  onWorkspaceStateChange,
  contextualRequest,
  onContextualRequestConsumed,
  onOpenSettings,
  onOpenQuest,
  compact = false,
}: AiPanelProps) {
  const { t } = useTranslation();
  const [input, setInput] = useState('');
  const [messages, setMessages] = useState<AiMessage[]>(() => {
    try {
      const saved = localStorage.getItem('varg.aiMessages');
      return saved ? JSON.parse(saved) : [];
    } catch { return []; }
  });
  const [status, setStatus] = useState<AiStatus>('idle');
  const [plan, setPlan] = useState<CopilotPlan | null>(null);
  const [approved, setApproved] = useState<Set<number>>(new Set());
  const [denied, setDenied] = useState<Set<number>>(new Set());
  const [approvalMode, setApprovalMode] = useState<ApprovalMode>(() => readStoredApprovalMode());
  const [entityDetails, setEntityDetails] = useState<EntityDetails | null>(null);
  const [mentionQuery, setMentionQuery] = useState<string | null>(null);
  const [mentionIndex, setMentionIndex] = useState(0);
  const [conversationTurns, setConversationTurns] = useState(0);
  const [workspaceView, setWorkspaceView] = useState<AiWorkspaceView>('chat');
  const [completedBundle, setCompletedBundle] = useState<CompletedChangeBundle | null>(null);
  const [pendingContinuation, setPendingContinuation] = useState<string | null>(null);
  const [queuedPrompts, setQueuedPrompts] = useState<QueuedPrompt[]>([]);
  const [interruptRequested, setInterruptRequested] = useState(false);
  const [requestActive, setRequestActive] = useState(false);
  const [thinkingEffort, setThinkingEffort] = useState<ThinkingEffort>('medium');
  const [knowledgeEntries, setKnowledgeEntries] = useState<KnowledgeEntry[]>([]);
  const [selectedKnowledgeIds, setSelectedKnowledgeIds] = useState<Set<string>>(new Set());
  const [knowledgeOpen, setKnowledgeOpen] = useState(false);
  const [promotingQuest, setPromotingQuest] = useState(false);
  const [copilotTasks, setCopilotTasks] = useState<CopilotTask[]>([]);
  const [taskCardVisible, setTaskCardVisible] = useState(false);
  const [taskCardCollapsed, setTaskCardCollapsed] = useState(false);
  const activeRequestRef = useRef(false);
  const interruptRequestedRef = useRef(false);
  const queuedPromptsRef = useRef<QueuedPrompt[]>([]);
  const taskHideTimerRef = useRef<number | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const nextMsgIdRef = useRef(
    messages.reduce((next, message) => {
      const numericId = Number.parseInt(message.id, 10);
      return Number.isFinite(numericId) ? Math.max(next, numericId + 1) : next;
    }, 1),
  );
  const isUserScrollingRef = useRef(false);
  const lastPromptRef = useRef<string | null>(null);
  const cancelRef = useRef<(() => void) | null>(null);

  // Auto-scroll — only if user is at the bottom
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
    if (atBottom || !isUserScrollingRef.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [messages]);

  // Persist messages to localStorage
  useEffect(() => {
    try {
      localStorage.setItem('varg.aiMessages', safeJsonStringify(messages));
    } catch { /* quota exceeded */ }
  }, [messages]);

  // Track user scroll position
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const onScroll = () => {
      isUserScrollingRef.current = el.scrollHeight - el.scrollTop - el.clientHeight > 120;
    };
    el.addEventListener('scroll', onScroll, { passive: true });
    return () => el.removeEventListener('scroll', onScroll);
  }, []);

  useEffect(() => { inputRef.current?.focus(); }, []);

  useEffect(() => {
    try {
      localStorage.setItem(approvalModeStorageKey, approvalMode);
    } catch { /* localStorage unavailable */ }
  }, [approvalMode]);

  useEffect(() => {
    listKnowledge()
      .then(result => {
        const approvedEntries = result.entries.filter(entry => entry.status === 'approved');
        setKnowledgeEntries(approvedEntries);
        setSelectedKnowledgeIds(current => {
          const approvedIds = new Set(approvedEntries.map(entry => entry.id));
          return new Set([...current].filter(id => approvedIds.has(id)));
        });
      })
      .catch(() => {
        setKnowledgeEntries([]);
        setSelectedKnowledgeIds(new Set());
      });
  }, []);

  useEffect(() => {
    rpc<{ turns: number }>('copilot/get_conversation_length')
      .then(result => setConversationTurns(result.turns))
      .catch(() => setConversationTurns(0));
  }, []);

  // Fetch entity details when selection changes
  useEffect(() => {
    if (!selectedEntity) {
      setEntityDetails(null);
      return;
    }
    rpc<EntityDetails>('shell/get_entity', { id: selectedEntity })
      .then(setEntityDetails)
      .catch(() => setEntityDetails(null));
  }, [selectedEntity]);

  const addMessage = useCallback((role: AiMessage['role'], content: string, cards?: AiCard[]) => {
    setMessages(prev => [...prev, {
      id: String(nextMsgIdRef.current++),
      role,
      content,
      cards,
      timestamp: Date.now(),
    }]);
  }, []);

  const updateQueuedPrompts = useCallback((next: QueuedPrompt[]) => {
    queuedPromptsRef.current = next;
    setQueuedPrompts(next);
  }, []);

  // ── New Chat (clear conversation history) ──

  const handleNewChat = useCallback(async () => {
    try {
      await rpc('copilot/clear_conversation');
    } catch { /* ignore */ }
    setMessages([]);
    localStorage.removeItem('varg.aiMessages');
    setPlan(null);
    setApproved(new Set());
    setDenied(new Set());
    setStatus('idle');
    setInput('');
    setConversationTurns(0);
    setCompletedBundle(null);
    setCopilotTasks([]);
    setTaskCardVisible(false);
    setTaskCardCollapsed(false);
    updateQueuedPrompts([]);
    activeRequestRef.current = false;
    interruptRequestedRef.current = false;
    setInterruptRequested(false);
    setRequestActive(false);
    setWorkspaceView('chat');
    inputRef.current?.focus();
  }, [updateQueuedPrompts]);

  const applyTaskUpdates = useCallback((updates?: CopilotTaskUpdate[]) => {
    if (!updates || updates.length === 0) return;
    if (taskHideTimerRef.current !== null) {
      window.clearTimeout(taskHideTimerRef.current);
      taskHideTimerRef.current = null;
    }
    setTaskCardVisible(true);
    setTaskCardCollapsed(false);
    setCopilotTasks(current => {
      let next = [...current];
      for (const update of updates) {
        const id = update.id?.trim();
        if (!id) continue;
        const existingIndex = next.findIndex(task => task.id === id);
        if (existingIndex >= 0) {
          const existing = next[existingIndex];
          next[existingIndex] = {
            ...existing,
            title: update.title?.trim() || existing.title,
            done: typeof update.done === 'boolean' ? update.done : existing.done,
          };
        } else {
          next.push({
            id,
            title: update.title?.trim() || id,
            done: Boolean(update.done),
          });
        }
      }
      return next;
    });
  }, []);

  const executeApproved = useCallback(async (approvedSet?: Set<number>) => {
    const indices = Array.from(approvedSet ?? approved);
    if (indices.length === 0) return;

    setStatus('executing');
    setWorkspaceView('chat');

    try {
      const result = await rpc<ApplyResult>('copilot/apply', {
        approved_indices: indices,
        approval_mode: approvalMode,
      });

      setStatus('complete');
      setPlan(null);

      const summary = result.summary || `${t('ai_applied_ops').replace('{count}', String(result.operations_performed))}`;
      setCompletedBundle({
        summary,
        operationsPerformed: result.operations_performed,
        traceEntries: result.trace_entries,
        consoleEntries: result.console_entries,
        undoAvailable: Boolean(result.undo_available),
        undoLabel: result.undo_label ?? null,
      });
      addMessage('assistant', summary);
      applyTaskUpdates(result.task_updates);
      if (result.needs_continuation) {
        setPendingContinuation(result.continuation_reason ?? 'Continue the original task.');
        addMessage('system', t('ai_continuing'));
      } else {
        setWorkspaceView('changes');
      }

      // Immediately refresh viewport and scene tree
      onSceneChanged?.();
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err.message || 'Unknown error';
      setStatus('error');
      addMessage('assistant', t('ai_execution_failed'), [{ type: 'error', data: msg }]);
    }
  }, [approved, addMessage, applyTaskUpdates, approvalMode, onSceneChanged, t]);

  // ── Submit ──

  const submitPrompt = useCallback(async (
    prompt: string,
    continuation = false,
    existingMessageId?: string,
  ) => {
    if (!prompt.trim() || activeRequestRef.current) return;

    if (!continuation) lastPromptRef.current = prompt;

    activeRequestRef.current = true;
    setRequestActive(true);
    interruptRequestedRef.current = false;
    setInterruptRequested(false);
    if (existingMessageId) {
      setMessages(prev => prev.map(message => message.id === existingMessageId
        ? { ...message, queued: false }
        : message));
    } else {
      addMessage('user', prompt);
    }
    setInput('');
    setStatus('thinking');
    setPlan(null);
    setApproved(new Set());
    setDenied(new Set());
    setCompletedBundle(null);
    setWorkspaceView('chat');
    const streamingMessageId = String(nextMsgIdRef.current++);
    setMessages(prev => [...prev, {
      id: streamingMessageId,
      role: 'assistant',
      content: '',
      timestamp: Date.now(),
    }]);

    // Let React paint the user's message before starting provider I/O.
    await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));

    try {
      // Build RPC params with structured entity context
      const planParams: Record<string, unknown> = { prompt, approval_mode: approvalMode };
      if (entityDetails) {
        planParams.selected_entity = entityDetails;
      }
      if (thinkingEffort !== 'off') {
        planParams.thinking_effort = thinkingEffort;
      }
      if (selectedKnowledgeIds.size > 0) {
        planParams.knowledge_ids = Array.from(selectedKnowledgeIds);
      }
      planParams.editor_context = {
        project_name: projectName ?? null,
        scene_object_count: sceneObjectCount,
        selected_entity_id: selectedEntity ?? null,
        selected_entity_name: selectedEntityName ?? null,
        attached_knowledge_count: selectedKnowledgeIds.size,
        recent_messages: messages.slice(-6).map(message => ({
          role: message.role,
          content: message.content,
        })),
      };

      const streamHandle = streamCopilotPlan<CopilotPlan>(planParams, (delta, kind) => {
        if (interruptRequestedRef.current) return;
        setMessages(prev => prev.map(message => {
          if (message.id !== streamingMessageId) return message;
          if (kind === 'thinking') {
            return {
              ...message,
              thinking: (message.thinking ?? '') + delta,
            };
          }
          if (kind === 'tool_call') {
            return {
              ...message,
              activeToolCalls: appendToolCallDelta(message.activeToolCalls ?? [], delta),
            };
          }
          const parsed = extractEmbeddedToolCalls(message.content + delta);
          return {
            ...message,
            content: parsed.content,
            activeToolCalls: [...(message.activeToolCalls ?? []), ...parsed.toolCalls],
          };
        }));
      });
      // Store cancel function for interrupt
      cancelRef.current = streamHandle.cancel;
      const result = await streamHandle.promise;
      if (interruptRequestedRef.current) {
        setMessages(prev => prev.map(message => message.id === streamingMessageId
          ? { ...message, content: message.content || t('msg_interrupted'), interrupted: true }
          : message));
        setPlan(null);
        setApproved(new Set());
        setStatus('idle');
        return;
      }
      setPlan(result.operations.length > 0 ? result : null);
      applyTaskUpdates(result.task_updates);

      const autoApproved = new Set<number>();
      result.operations.forEach((op) => {
        const needsApproval = op.requires_approval ?? (
          op.permission_kind !== 'read'
          && !(op.permission_kind === 'command' && op.permanently_allowed)
        );
        if (!needsApproval) {
          autoApproved.add(op.index);
        }
      });
      setApproved(autoApproved);
      setDenied(new Set());
      setStatus(result.operations.length > 0 ? 'ready' : 'complete');
      setConversationTurns(t => t + 1);

      const finalFallback = result.operations.length > 0
          ? `${t('ai_propose_ops').replace('{count}', String(result.operations.length))}`
          : t('ai_no_changes');
      const finalContent = result.message || finalFallback;
      setMessages(prev => prev.map(message => message.id === streamingMessageId
        ? (() => {
            const parsed = extractEmbeddedToolCalls(message.content || finalContent);
            return {
              ...message,
              content: parsed.content || finalFallback,
              activeToolCalls: [...(message.activeToolCalls ?? []), ...parsed.toolCalls].map(tc => ({ ...tc, complete: true })),
              cards: result.operations.length > 0
                ? [{ type: 'plan', data: result }]
                : undefined,
            };
          })()
        : message));
      if (result.operations.length > 0) setWorkspaceView('changes');
      if (result.needs_continuation) {
        setPendingContinuation(result.continuation_reason ?? 'Continue the original task.');
        addMessage('system', t('ai_continuing'));
      }

      // Reads, session-approved writes, and permanently-approved commands do not prompt.
      if (result.operations.length > 0 && autoApproved.size === result.operations.length) {
        await executeApproved(autoApproved);
      }
    } catch (err: any) {
      if (interruptRequestedRef.current) {
        setMessages(prev => prev.map(message => message.id === streamingMessageId
          ? { ...message, content: message.content || t('msg_interrupted'), interrupted: true }
          : message));
        setStatus('idle');
        return;
      }
      const msg = typeof err === 'string' ? err : err.message || 'Unknown error';
      const displayMsg = msg.includes('api_key') || msg.includes('API key') || msg.includes('401')
        ? t('error_api_key')
        : msg.includes('rate_limit') || msg.includes('429')
          ? t('error_rate_limit')
          : msg.includes('timeout') || msg.includes('timed out')
            ? t('error_timeout')
            : msg.includes('network') || msg.includes('fetch')
              ? t('error_network')
              : msg;
      setStatus('error');
      setMessages(prev => prev.map(message => message.id === streamingMessageId
        ? { ...message, content: displayMsg, cards: [{ type: 'error', data: msg }] }
        : message));
    } finally {
      activeRequestRef.current = false;
      setRequestActive(false);
      interruptRequestedRef.current = false;
      setInterruptRequested(false);
      cancelRef.current = null;
    }
  }, [entityDetails, addMessage, approvalMode, thinkingEffort, selectedKnowledgeIds, applyTaskUpdates, executeApproved]);

  const queueOrSubmitPrompt = useCallback((prompt: string) => {
    const trimmed = prompt.trim();
    if (!trimmed) return;
    setInput('');
    if (!activeRequestRef.current && status !== 'executing') {
      submitPrompt(trimmed);
      return;
    }

    const id = String(nextMsgIdRef.current++);
    setMessages(prev => [...prev, {
      id,
      role: 'user',
      content: trimmed,
      timestamp: Date.now(),
      queued: true,
    }]);
    updateQueuedPrompts([...queuedPromptsRef.current, { id, prompt: trimmed }]);
    setPendingContinuation(null);
  }, [status, submitPrompt, updateQueuedPrompts]);

  const requestInterrupt = useCallback(() => {
    if (!activeRequestRef.current) return;
    interruptRequestedRef.current = true;
    setInterruptRequested(true);
    setPendingContinuation(null);
    cancelRef.current?.();
    cancelRef.current = null;
  }, []);

  useEffect(() => {
    if (!contextualRequest) return;
    queueOrSubmitPrompt(contextualRequest.prompt);
    onContextualRequestConsumed?.(contextualRequest.id);
  }, [contextualRequest, onContextualRequestConsumed, queueOrSubmitPrompt]);

  const undoLastAiEdit = useCallback(async () => {
    setStatus('executing');
    try {
      const result = await rpc<UndoResult>('copilot/undo_last');
      setCompletedBundle(current => current ? {
        ...current,
        undoAvailable: false,
        traceEntries: [...current.traceEntries, ...result.trace_entries],
        consoleEntries: current.consoleEntries,
      } : current);
      if (result.applied) {
        onSceneChanged?.();
      }
      addMessage('assistant', result.summary);
      setStatus('complete');
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err.message || 'Unknown error';
      setStatus('error');
      addMessage('assistant', t('ai_execution_failed'), [{ type: 'error', data: msg }]);
    }
  }, [addMessage, onSceneChanged, t]);

  const decideOperation = useCallback(async (
    operation: CopilotOperation,
    decision: 'once' | 'session' | 'always' | 'deny',
  ) => {
    if (operation.permission_kind === 'unsupported' && decision !== 'deny') {
      return;
    }
    if (decision === 'deny') {
      setApproved(current => {
        const next = new Set(current);
        next.delete(operation.index);
        return next;
      });
      setDenied(current => new Set(current).add(operation.index));
      return;
    }

    if (decision === 'session') {
      setApprovalMode('auto-safe');
      setApproved(current => {
        const next = new Set(current);
        plan?.operations
          .filter(item => item.permission_kind === 'write')
          .forEach(item => next.add(item.index));
        return next;
      });
      setDenied(current => {
        const next = new Set(current);
        plan?.operations
          .filter(item => item.permission_kind === 'write')
          .forEach(item => next.delete(item.index));
        return next;
      });
      return;
    }

    if (decision === 'always' && operation.command && operation.permission_kind === 'command') {
      await rpc('copilot/allow_command', { command: operation.command });
      setPlan(current => current ? {
        ...current,
        operations: current.operations.map(item => item.command === operation.command
          ? { ...item, permanently_allowed: true }
          : item),
      } : current);
      setApproved(current => {
        const next = new Set(current);
        plan?.operations
          .filter(item => item.command === operation.command)
          .forEach(item => next.add(item.index));
        return next;
      });
    } else {
      setApproved(current => new Set(current).add(operation.index));
    }
    setDenied(current => {
      const next = new Set(current);
      next.delete(operation.index);
      return next;
    });
  }, [plan]);

  useEffect(() => {
    if (!pendingContinuation || status !== 'complete' || queuedPromptsRef.current.length > 0) return;
    const reason = pendingContinuation;
    setPendingContinuation(null);
    submitPrompt(
      `Continue the original task using the tool results now present in the conversation. Do not repeat completed inspection. Reason to continue: ${reason}. Propose the concrete remaining operations, or emit complete when the original task is genuinely complete.`,
      true,
    );
  }, [pendingContinuation, status, submitPrompt]);

  useEffect(() => {
    if (requestActive || status === 'thinking' || status === 'executing') return;
    const [next, ...rest] = queuedPromptsRef.current;
    if (!next) return;
    updateQueuedPrompts(rest);
    submitPrompt(next.prompt, false, next.id);
  }, [requestActive, status, submitPrompt, updateQueuedPrompts]);

  // ── @ Mention logic ──

  const mentionMatches = useMemo(() => {
    if (mentionQuery === null || !sceneObjects) return [];
    const q = mentionQuery.toLowerCase();
    return sceneObjects.filter(o => o.name.toLowerCase().includes(q)).slice(0, 6);
  }, [mentionQuery, sceneObjects]);

  const handleInputChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const val = e.target.value;
    setInput(val);

    // Auto-resize textarea
    const ta = e.target;
    ta.style.height = 'auto';
    ta.style.height = Math.min(ta.scrollHeight, 200) + 'px';

    // Detect @ mention trigger
    const cursor = e.target.selectionStart ?? val.length;
    const beforeCursor = val.slice(0, cursor);
    const atIdx = beforeCursor.lastIndexOf('@');
    if (atIdx >= 0 && (atIdx === 0 || /\s/.test(beforeCursor[atIdx - 1]))) {
      const query = beforeCursor.slice(atIdx + 1);
      if (!query.includes(' ') && query.length < 30) {
        setMentionQuery(query);
        setMentionIndex(0);
        return;
      }
    }
    setMentionQuery(null);
  }, []);

  const insertMention = useCallback((obj: { id: string; name: string }) => {
    const cursor = inputRef.current?.selectionStart ?? input.length;
    const beforeCursor = input.slice(0, cursor);
    const atIdx = beforeCursor.lastIndexOf('@');
    const afterCursor = input.slice(cursor);
    const newInput = beforeCursor.slice(0, atIdx) + `@${obj.name} ` + afterCursor;
    setInput(newInput);
    setMentionQuery(null);
    inputRef.current?.focus();
  }, [input]);

  const discardProposal = useCallback(() => {
    setPlan(null);
    setApproved(new Set());
    setDenied(new Set());
    setStatus('idle');
    setWorkspaceView('chat');
  }, []);

  const attachedKnowledge = useMemo(
    () => knowledgeEntries.filter(entry => selectedKnowledgeIds.has(entry.id)),
    [knowledgeEntries, selectedKnowledgeIds],
  );

  useEffect(() => {
    if (!taskCardVisible || copilotTasks.length === 0 || !copilotTasks.every(task => task.done)) return;
    if (taskHideTimerRef.current !== null) window.clearTimeout(taskHideTimerRef.current);
    taskHideTimerRef.current = window.setTimeout(() => {
      setTaskCardVisible(false);
      setCopilotTasks([]);
      taskHideTimerRef.current = null;
    }, 4000);
    return () => {
      if (taskHideTimerRef.current !== null) {
        window.clearTimeout(taskHideTimerRef.current);
        taskHideTimerRef.current = null;
      }
    };
  }, [copilotTasks, taskCardVisible]);

  const buildQuestPromotionContext = useCallback(() => {
    const recentMessages = messages
      .slice(-8)
      .map(message => `${message.role.toUpperCase()}: ${message.content || ''}`.trim())
      .filter(Boolean)
      .join('\n\n');
    const selectedEntityContext = entityDetails
      ? [
          `Selected entity: ${entityDetails.name} (${entityDetails.id})`,
          `Tag: ${entityDetails.tag || 'untagged'}`,
          `Position: ${entityDetails.transform.position.map(value => value.toFixed(2)).join(', ')}`,
          `Components: ${entityDetails.components.map(component => component.type).join(', ') || 'none'}`,
        ].join('\n')
      : 'Selected entity: none';
    const proposedOperations = plan?.operations.length
      ? plan.operations
          .map(operation => [
            `- [${operation.permission_kind.toUpperCase()}] ${operation.preview}`,
            approved.has(operation.index) ? '  Decision: approved' : denied.has(operation.index) ? '  Decision: denied' : '  Decision: pending',
            operation.command ? `  Command: ${operation.command}` : null,
          ].filter(Boolean).join('\n'))
          .join('\n')
      : 'No active proposed operations.';
    const completed = completedBundle
      ? [
          `Last applied summary: ${completedBundle.summary}`,
          `Operations performed: ${completedBundle.operationsPerformed}`,
          `Trace entries: ${completedBundle.traceEntries.map(entry => `${entry.tool}:${entry.result}`).join(', ') || 'none'}`,
          `Console entries: ${completedBundle.consoleEntries.map(entry => `${entry.level}:${entry.message}`).join(' | ') || 'none'}`,
        ].join('\n')
      : 'No applied Copilot result in this session.';
    const knowledge = attachedKnowledge.length
      ? attachedKnowledge.map(entry => `- ${entry.category}: ${entry.content}`).join('\n')
      : 'No Knowledge entries attached.';

    return [
      `Source: Promoted from Editor Copilot`,
      `Project: ${projectName || 'current project'}`,
      `Scene object count: ${sceneObjectCount}`,
      selectedEntityContext,
      '',
      'Recent conversation:',
      recentMessages || 'No prior conversation captured.',
      '',
      'Proposed operations:',
      proposedOperations,
      '',
      'Latest applied result:',
      completed,
      '',
      'Attached Knowledge:',
      knowledge,
    ].join('\n');
  }, [approved, attachedKnowledge, completedBundle, denied, entityDetails, messages, plan, projectName, sceneObjectCount]);

  const promoteCurrentWorkToQuest = useCallback(async () => {
    if (promotingQuest) return;
    const prompt = lastPromptRef.current
      || messages.slice().reverse().find(message => message.role === 'user')?.content
      || input
      || t('ai_promote_default_goal');
    setPromotingQuest(true);
    try {
      const quest = await promoteQuest(prompt, buildQuestPromotionContext());
      addMessage('assistant', t('ai_promoted_to_quest').replace('{title}', quest.title));
      onOpenQuest?.(quest.id);
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err.message || 'Unknown error';
      setStatus('error');
      addMessage('assistant', t('ai_promote_failed'), [{ type: 'error', data: msg }]);
    } finally {
      setPromotingQuest(false);
    }
  }, [addMessage, buildQuestPromotionContext, input, messages, onOpenQuest, promotingQuest, t]);

  // ── Keyboard ──

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    // Handle mention navigation
    if (mentionQuery !== null && mentionMatches.length > 0) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setMentionIndex(i => Math.min(i + 1, mentionMatches.length - 1));
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setMentionIndex(i => Math.max(i - 1, 0));
        return;
      }
      if (e.key === 'Tab' || e.key === 'Enter') {
        e.preventDefault();
        insertMention(mentionMatches[mentionIndex]);
        return;
      }
      if (e.key === 'Escape') {
        setMentionQuery(null);
        return;
      }
    }

    if (e.key === 'Escape') {
      if (activeRequestRef.current) {
        requestInterrupt();
      } else if (plan && status === 'ready') {
        discardProposal();
      }
      return;
    }

    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      queueOrSubmitPrompt(input);
    }
  }, [input, queueOrSubmitPrompt, mentionQuery, mentionMatches, mentionIndex, insertMention, plan, status, requestInterrupt, discardProposal]);

  // ── Render ──

  const hasPlan = plan && plan.operations.length > 0 && status === 'ready';
  const approvedWriteCount = plan?.operations.filter(operation => (
    operation.requires_write && approved.has(operation.index)
  )).length ?? 0;
  const sendButtonMode: 'send' | 'stop' | 'stopping' | 'disabled' = requestActive && status !== 'executing'
    ? (interruptRequested ? 'stopping' : 'stop')
    : status === 'executing'
      ? 'disabled'
      : 'send';
  const sendButtonDisabled = sendButtonMode === 'send'
    ? !input.trim()
    : sendButtonMode !== 'stop';
  const sendButtonLabel = sendButtonMode === 'stop'
    ? t('btn_stop_response')
    : sendButtonMode === 'stopping'
      ? t('btn_stopping')
      : t('btn_send');
  const handleComposerAction = () => {
    if (sendButtonMode === 'stop') {
      requestInterrupt();
      return;
    }
    if (sendButtonMode === 'send') {
      queueOrSubmitPrompt(input);
    }
  };
  const toggleKnowledge = useCallback((id: string) => {
    setSelectedKnowledgeIds(current => {
      const next = new Set(current);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }, []);

  // Context value for inline plan card approval buttons
  const planApprovalCtx = useMemo<PlanApprovalCtx>(() => ({
    approved,
    denied,
    decideOperation,
    active: status === 'ready',
  }), [approved, denied, decideOperation, status]);
  const statusLabel: Record<AiStatus, string> = {
    idle: t('status_idle'),
    thinking: t('status_planning'),
    ready: t('status_waiting_review'),
    executing: t('status_applying'),
    complete: t('status_task_complete'),
    error: t('status_action_required'),
  };

  useEffect(() => {
    onWorkspaceStateChange?.({
      status,
      plan,
      approved,
      denied,
      completedBundle,
      applyApproved: () => executeApproved(),
      discardProposal,
    });
  }, [approved, completedBundle, denied, discardProposal, executeApproved, onWorkspaceStateChange, plan, status]);

  return (
    <div className={cls('flex h-full flex-col overflow-hidden', compact && 'bg-[#171716]')}>
      {compact ? (
        <div className="flex h-11 shrink-0 items-center gap-2 px-2 text-[12px] text-[var(--text-muted)]">
          <span className="mr-auto">Tasks</span>
          <button className={panelIconButtonClass} onClick={handleNewChat} title={t('ai_new_chat')} aria-label={t('ai_new_chat')}>
            <IconNewChat />
          </button>
          <button className={panelIconButtonClass} onClick={onOpenSettings} title={t('ai_settings')} aria-label={t('ai_settings')}>
            <IconSettings />
          </button>
          {onOpenQuest && (
            <button className={panelIconButtonClass} onClick={() => onOpenQuest()} title={t('quest_title')} aria-label={t('quest_title')}>
              <IconSparkles />
            </button>
          )}
        </div>
      ) : (
        <ContextBar
          projectName={projectName}
          selectedEntity={selectedEntityName}
          sceneObjectCount={sceneObjectCount}
          onSettingsClick={onOpenSettings}
          onNewChat={handleNewChat}
          conversationTurns={conversationTurns}
          attachedKnowledgeCount={attachedKnowledge.length}
        />
      )}

      {!chatOnly && (plan || completedBundle) && <div className="flex min-h-9 gap-1 border-b border-[var(--border)] bg-[var(--bg-surface)] px-2 py-1 [&_span]:min-w-4 [&_span]:rounded-lg [&_span]:bg-[var(--accent-dim)] [&_span]:px-[5px] [&_span]:py-px [&_span]:text-[9px] [&_span]:text-[var(--text-secondary)]" role="tablist" aria-label="AI workspace">
        {(['chat', 'changes'] as AiWorkspaceView[]).map(view => (
          <button
            key={view}
            className={workspaceTabClass(workspaceView === view)}
            onClick={() => setWorkspaceView(view)}
            role="tab"
            aria-selected={workspaceView === view}
          >
            {view === 'chat' ? t('tab_chat') : t('tab_changes')}
            {view === 'changes' && plan && plan.operations.length > 0 && (
              <span>{plan.operations.length}</span>
            )}
          </button>
        ))}
      </div>}

      {/* Entity context card — shown when an entity is selected */}
      {!compact && entityDetails && <EntityContextCard entity={entityDetails} />}

      {/* Messages */}
      <PlanApprovalContext.Provider value={planApprovalCtx}>
      <div
        ref={scrollRef}
        className={cls("flex flex-1 flex-col gap-3 overflow-y-auto p-4", compact && "gap-2 px-2 py-0", !chatOnly && workspaceView !== 'chat' && "bg-[var(--bg-base)] p-3")}
        aria-live="polite"
      >
        {!chatOnly && workspaceView === 'changes' && (
          <div className="flex flex-col gap-3">
            <div className="mb-2.5 text-[10px] font-bold uppercase tracking-[0.07em] text-[var(--text-secondary)]">{t('changes_bundle_title')}</div>
            {!plan || plan.operations.length === 0 ? (
              completedBundle ? (
                <div className="flex flex-col gap-3 rounded-[9px] border border-[rgba(34,197,94,0.32)] bg-[rgba(34,197,94,0.05)] p-3.5">
                  <div className="flex items-start gap-[9px] text-[#22c55e] [&>div]:flex [&>div]:flex-col [&>div]:gap-[3px] [&_strong]:text-xs [&_strong]:text-[var(--text-primary)] [&_span]:text-[11px] [&_span]:leading-[1.45] [&_span]:text-[var(--text-secondary)]">
                    <IconCheck />
                    <div>
                      <strong>{t('changes_applied')}</strong>
                      <span>{completedBundle.summary}</span>
                    </div>
                  </div>
                  <div className="grid grid-cols-2 gap-1.5 [&_span]:rounded-md [&_span]:bg-[var(--bg-base)] [&_span]:p-2 [&_span]:text-center [&_span]:text-[10px] [&_span]:text-[var(--text-muted)] [&_strong]:block [&_strong]:font-[var(--font-mono)] [&_strong]:text-sm [&_strong]:font-semibold [&_strong]:text-[var(--text-primary)]">
                    <span><strong>{completedBundle.operationsPerformed}</strong> {t('label_operations')}</span>
                    <span><strong>{completedBundle.traceEntries.length}</strong> {t('label_trace_entries')}</span>
                    <span><strong>{completedBundle.consoleEntries.length}</strong> console</span>
                  </div>
                  {completedBundle.undoAvailable && (
                    <button className="inline-flex min-h-[30px] w-max cursor-pointer items-center gap-1.5 rounded-md border border-[var(--border-light)] bg-[var(--bg-surface)] px-2.5 py-0 font-[var(--font-sans)] text-[10px] font-semibold text-[var(--text-secondary)] hover:border-[var(--accent)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]" onClick={undoLastAiEdit}>
                      <IconUndo /> Undo {completedBundle.undoLabel ?? 'AI edit'}
                    </button>
                  )}
                  {completedBundle.traceEntries.length > 0 && <InlineCard card={{ type: 'trace', data: completedBundle.traceEntries }} />}
                  {completedBundle.consoleEntries.length > 0 && <InlineCard card={{ type: 'console', data: completedBundle.consoleEntries }} />}
                </div>
              ) : (
                <div className="flex flex-col gap-1.5 rounded-[9px] border border-dashed border-[var(--border-light)] bg-[var(--accent-dim)] p-[18px] [&_strong]:text-xs [&_strong]:text-[var(--text-secondary)] [&_span]:text-[11px] [&_span]:leading-normal [&_span]:text-[var(--text-muted)]">
                  <strong>{t('changes_empty')}</strong>
                  <span>{t('changes_empty_desc')}</span>
                </div>
              )
            ) : <>
              <div className="flex items-center justify-between gap-2 text-[10px] text-[var(--text-muted)]">
                <span>{t('changes_decision_hint')}</span>
                {onOpenQuest && (
                  <button
                    className={buttonClass('ghost', 'sm')}
                    onClick={promoteCurrentWorkToQuest}
                    disabled={promotingQuest}
                  >
                    {promotingQuest ? <IconLoader className={commonSpinnerClass} /> : <IconSparkles />}
                    {t('ai_promote_to_quest')}
                  </button>
                )}
              </div>
              {plan.operations.map(operation => (
              <div key={operation.index} className="grid grid-cols-[58px_1fr] items-start gap-2 rounded-lg border border-dashed border-[var(--border)] bg-[var(--accent-dim)] p-2.5 text-[11px] leading-[1.45] text-[var(--text-secondary)] hover:border-[var(--border-light)]">
                <span className={changeKindClass(operation.permission_kind)}>
                  {operation.permission_kind.toUpperCase()}
                </span>
                <span className="leading-[1.45]" title={operation.preview}>{operation.preview}</span>
                <div className="col-start-2 flex flex-wrap gap-[5px]">
                  {operation.permission_kind === 'read' ? (
                    <span className={permissionStateClass(true)}>{t('op_allowed_auto')}</span>
                  ) : operation.permission_kind === 'unsupported' ? (
                    <span className={permissionStateClass(false)}>{t('op_unsupported')}</span>
            ) : approved.has(operation.index) ? (
              <span className={permissionStateClass(true)}>
                {operation.permission_kind === 'command' && operation.permanently_allowed ? t('op_always_allowed') : t('op_allowed')}
              </span>
            ) : operation.requires_approval === false ? (
              <span className={permissionStateClass(true)}>{t('op_allowed_auto')}</span>
            ) : denied.has(operation.index) ? (
              <span className={permissionStateClass(false)}>{t('op_denied_once')}</span>
            ) : operation.permission_kind === 'write' ? <>
                    <button className={permissionButtonClass} onClick={() => decideOperation(operation, 'once')}>{t('btn_allow_once')}</button>
                    <button className={permissionButtonClass} onClick={() => decideOperation(operation, 'session')}>{t('btn_allow_session')}</button>
                    <button className={permissionButtonClass} onClick={() => decideOperation(operation, 'deny')}>{t('btn_deny_once')}</button>
                  </> : <>
                    <button className={permissionButtonClass} onClick={() => decideOperation(operation, 'once')}>{t('btn_allow_once')}</button>
                    <button className={permissionButtonClass} onClick={() => decideOperation(operation, 'always')}>{t('btn_allow_always')}</button>
                    <button className={permissionButtonClass} onClick={() => decideOperation(operation, 'deny')}>{t('btn_deny_once')}</button>
                  </>}
                </div>
              </div>
              ))}
            </>}
          </div>
        )}
        {(chatOnly || workspaceView === 'chat') && <>
        {messages.length === 0 && (
          compact ? (
            <div className="flex flex-1 items-center justify-center text-[var(--text-muted)]">
              <div className="grid size-10 place-items-center rounded-full border border-[var(--border)] text-[var(--text-muted)] opacity-70">
                <IconBot size={22} />
              </div>
            </div>
          ) : (
            <div className="m-auto flex h-full max-w-[440px] flex-col items-center justify-start gap-2.5 px-5 pt-7 pb-5 text-center text-[var(--text-secondary)]">
              <div className="mb-0.5 flex h-[46px] w-[46px] items-center justify-center rounded-[14px] border border-[var(--border-light)] bg-[linear-gradient(145deg,var(--bg-elevated),var(--bg-surface))] text-[var(--accent)] shadow-[0_8px_24px_rgba(0,0,0,0.2)] [&_svg]:opacity-90"><IconSparkles size={24} /></div>
              <span className="text-[10px] font-bold uppercase tracking-[0.1em] text-[var(--accent)]">{t('ai_workspace_eyebrow')}</span>
              <p className="text-xl font-semibold text-[var(--text-primary)]">{t('ai_empty_title')}</p>
              <p className="max-w-[340px] text-[13px] leading-[1.55] text-[var(--text-secondary)]">
                {t('ai_empty_desc')}
              </p>
              <div className="my-2 mb-1 grid w-full grid-cols-4 rounded-lg border border-[var(--border)] bg-[rgba(0,0,0,0.12)] p-1" aria-label="AI editing workflow">
                <span className={workflowStepClass(true)}>{t('workflow_step_describe')}</span>
                <span className={workflowStepClass()}>{t('workflow_step_review')}</span>
                <span className={workflowStepClass()}>{t('workflow_step_apply')}</span>
                <span className={workflowStepClass()}>{t('workflow_step_verify')}</span>
              </div>
              <div className="mt-1 grid w-full gap-[7px]">
                <button className="flex cursor-pointer flex-col gap-0.5 rounded-lg border border-[var(--border)] bg-[var(--bg-surface)] px-3 py-2.5 text-left font-[inherit] text-[var(--text-secondary)] transition-[background,border-color,transform] duration-[var(--transition-fast)] hover:-translate-y-px hover:border-[var(--accent)] hover:bg-[var(--accent-dim)] [&_strong]:text-xs [&_strong]:font-semibold [&_strong]:text-[var(--text-primary)] [&_span]:text-[11px] [&_span]:text-[var(--text-muted)]" onClick={() => submitPrompt('Create a playable third-person character with a following camera and basic movement controls')}>
                  <strong>{t('prompt_playable_char')}</strong>
                  <span>{t('prompt_playable_char_desc')}</span>
                </button>
                <button className="flex cursor-pointer flex-col gap-0.5 rounded-lg border border-[var(--border)] bg-[var(--bg-surface)] px-3 py-2.5 text-left font-[inherit] text-[var(--text-secondary)] transition-[background,border-color,transform] duration-[var(--transition-fast)] hover:-translate-y-px hover:border-[var(--accent)] hover:bg-[var(--accent-dim)] [&_strong]:text-xs [&_strong]:font-semibold [&_strong]:text-[var(--text-primary)] [&_span]:text-[11px] [&_span]:text-[var(--text-muted)]" onClick={() => submitPrompt('Improve the lighting and atmosphere of this scene while preserving the current composition')}>
                  <strong>{t('prompt_improve_scene')}</strong>
                  <span>{t('prompt_improve_scene_desc')}</span>
                </button>
                <button className="flex cursor-pointer flex-col gap-0.5 rounded-lg border border-[var(--border)] bg-[var(--bg-surface)] px-3 py-2.5 text-left font-[inherit] text-[var(--text-secondary)] transition-[background,border-color,transform] duration-[var(--transition-fast)] hover:-translate-y-px hover:border-[var(--accent)] hover:bg-[var(--accent-dim)] [&_strong]:text-xs [&_strong]:font-semibold [&_strong]:text-[var(--text-primary)] [&_span]:text-[11px] [&_span]:text-[var(--text-muted)]" onClick={() => submitPrompt('Inspect the current project and recommend the highest-impact next improvement')}>
                  <strong>{t('prompt_inspect')}</strong>
                  <span>{t('prompt_inspect_desc')}</span>
                </button>
              </div>
              <button className="mt-1 flex cursor-pointer items-center gap-[5px] rounded-md border-0 bg-transparent px-3 py-1.5 font-[inherit] text-[11px] text-[var(--text-muted)] hover:text-[var(--accent)]" onClick={onOpenSettings}>
                <IconSettings size={12} />
                <span>{t('ai_model_settings')}</span>
                <IconChevronRight size={12} />
              </button>
            </div>
          )
        )}
        {messages.map((msg) => (
          <MessageBubble key={msg.id} msg={msg} />
        ))}

        {/* Executing indicator */}
        {status === 'executing' && (
          <div className="flex items-center gap-2 px-3 py-2 text-[13px] text-[var(--text-secondary)]">
            <IconLoader className={commonSpinnerClass} />
            <span>{t('status_executing')}</span>
          </div>
        )}
        {status === 'thinking' && (
          <div className="flex items-center gap-2 px-3 py-2 text-[13px] text-[var(--text-secondary)]">
            <IconLoader className={commonSpinnerClass} />
            <span>{t('status_thinking')}</span>
          </div>
        )}
        {status === 'error' && lastPromptRef.current && (
          <div className="flex items-center gap-2 px-3 py-2">
            <button
              className={buttonClass('secondary', 'sm')}
              onClick={() => {
                const p = lastPromptRef.current;
                if (p) {
                  setStatus('idle');
                  submitPrompt(p);
                }
              }}
            >
              {t('btn_retry')}
            </button>
          </div>
        )}
        </>}
      </div>
      </PlanApprovalContext.Provider>

      {/* Plan approval bar */}
      {hasPlan && (() => {
        const approvableOps = plan!.operations.filter(op => op.permission_kind === 'write' || op.permission_kind === 'command');
        const pendingOps = approvableOps.filter(op => !approved.has(op.index) && !denied.has(op.index));
        const approveAll = () => {
          const count = approvableOps.length;
          if (!window.confirm(t('confirm_approve_all').replace('{count}', String(count)))) return;
          setApproved(current => {
            const next = new Set(current);
            approvableOps.forEach(op => next.add(op.index));
            return next;
          });
          setDenied(current => {
            const next = new Set(current);
            approvableOps.forEach(op => next.delete(op.index));
            return next;
          });
        };
        return (
          <div className="flex items-center gap-2 border-t border-[var(--border)] bg-[var(--bg-surface)] px-3 py-2">
            <button
              className={buttonClass('primary', 'sm')}
              onClick={() => executeApproved()}
              disabled={approved.size === 0}
              title={approved.size === 0 ? 'Approve at least one operation below, or click \"Approve all\" to continue' : undefined}
            >
              {t('btn_continue_allowed').replace('{count}', String(approved.size))}
            </button>
            {pendingOps.length > 0 && (
              <button
                className={buttonClass('secondary', 'sm')}
                onClick={approveAll}
                title={`Approve all ${pendingOps.length} pending write/command operation${pendingOps.length === 1 ? '' : 's'}`}
              >
                {t('btn_approve_all').replace('{count}', String(pendingOps.length))}
              </button>
            )}
            <button
              className={buttonClass('ghost', 'sm')}
              onClick={discardProposal}
            >
              {t('btn_discard')}
            </button>
            {onOpenQuest && (
              <button
                className={buttonClass('secondary', 'sm')}
                onClick={promoteCurrentWorkToQuest}
                disabled={promotingQuest}
              >
                {promotingQuest ? <IconLoader className={commonSpinnerClass} /> : <IconSparkles />}
                {t('ai_promote_to_quest')}
              </button>
            )}
            {approvedWriteCount > 0 && <span className="shrink-0 rounded bg-[rgba(245,158,11,0.12)] px-1.5 py-[3px] text-[9px] font-semibold text-[#f59e0b]">{approvedWriteCount} {approvedWriteCount === 1 ? t('label_write') : t('label_writes')}</span>}
          </div>
        );
      })()}

      {/* Quick Actions + Input */}
      <div className={cls(
        "relative border-t border-[var(--border)] bg-[var(--bg-surface)] px-3 pt-2.5 pb-3 shadow-[0_-8px_24px_rgba(0,0,0,0.12)]",
        compact && "border-t-0 bg-transparent px-2 pb-2 pt-2 shadow-none",
      )}>
        {taskCardVisible && copilotTasks.length > 0 && (
          <CopilotTaskCard
            tasks={copilotTasks}
            collapsed={taskCardCollapsed}
            onToggle={() => setTaskCardCollapsed(value => !value)}
          />
        )}

        {!compact && <QuickActions onAction={onQuickAction} />}

        {!compact && knowledgeEntries.length > 0 && (
          <div className="mb-2 overflow-hidden rounded-[7px] border border-[var(--border)] bg-[var(--bg-base)]">
            <button
              className="flex h-[30px] w-full cursor-pointer items-center gap-1.5 border-0 bg-transparent px-2 text-left font-[var(--font-sans)] text-[11px] font-semibold text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] [&_b]:ml-auto [&_b]:inline-flex [&_b]:h-[18px] [&_b]:min-w-[18px] [&_b]:items-center [&_b]:justify-center [&_b]:rounded-[9px] [&_b]:bg-[rgba(34,197,94,0.12)] [&_b]:text-[10px] [&_b]:text-[#86efac]"
              onClick={() => setKnowledgeOpen(open => !open)}
              type="button"
            >
              {knowledgeOpen ? <IconChevronDown size={12} /> : <IconChevronRight size={12} />}
              <IconSparkles size={12} />
              <span>Knowledge</span>
              <b>{attachedKnowledge.length}</b>
            </button>
            {knowledgeOpen && (
              <div className="grid max-h-[148px] gap-px overflow-y-auto border-t border-[var(--border)]">
                {knowledgeEntries.map(entry => (
                  <label key={entry.id} className="grid cursor-pointer grid-cols-[16px_minmax(0,1fr)] items-start gap-[7px] px-2 py-[7px] hover:bg-[var(--bg-hover)] [&_input]:mt-0.5 [&_input]:h-[13px] [&_input]:w-[13px] [&_span]:grid [&_span]:min-w-0 [&_span]:gap-0.5 [&_strong]:text-[10px] [&_strong]:font-bold [&_strong]:uppercase [&_strong]:text-[var(--text-primary)] [&_small]:line-clamp-2 [&_small]:overflow-hidden [&_small]:text-[11px] [&_small]:leading-[1.35] [&_small]:text-[var(--text-muted)]">
                    <input
                      type="checkbox"
                      checked={selectedKnowledgeIds.has(entry.id)}
                      onChange={() => toggleKnowledge(entry.id)}
                    />
                    <span>
                      <strong>{entry.category}</strong>
                      <small>{entry.content}</small>
                    </span>
                  </label>
                ))}
              </div>
            )}
          </div>
        )}

        {(requestActive || status === 'executing' || queuedPrompts.length > 0) && (
          <div className="mb-2 flex items-center gap-1.5 px-1 text-[11px] text-[var(--text-secondary)] [&_svg]:h-[11px] [&_svg]:w-[11px] [&_svg]:shrink-0" role="status">
            <IconLoader className={requestActive || status === 'executing' ? commonSpinnerClass : undefined} />
            <span>
              {interruptRequested
                ? t('queue_stopping')
                : queuedPrompts.length > 0
                  ? t('queue_messages_queued').replace('{count}', String(queuedPrompts.length))
                  : status === 'executing'
                    ? t('queue_applying')
                    : t('queue_responding')}
            </span>
          </div>
        )}

        {/* @ Mention autocomplete dropdown */}
        {mentionQuery !== null && mentionMatches.length > 0 && (
          <div className="absolute right-2 bottom-full left-2 z-[100] mb-1 max-h-[180px] overflow-y-auto rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-surface)] shadow-[var(--shadow-md)]" role="listbox" aria-label={t('mention_suggestions')}>
            {mentionMatches.map((obj, i) => (
              <button
                key={obj.id}
                role="option"
                aria-selected={i === mentionIndex}
                className={mentionItemClass(i === mentionIndex)}
                onMouseDown={(e) => { e.preventDefault(); insertMention(obj); }}
              >
                <span className="text-[11px] text-[var(--accent)]">⬡</span>
                <span className="font-medium">{obj.name}</span>
              </button>
            ))}
          </div>
        )}

        {!compact && <div className="mb-1.5 flex items-center justify-between gap-3 text-[11px] font-semibold text-[var(--text-primary)] [&_span:last-child]:text-[9px] [&_span:last-child]:font-normal [&_span:last-child]:text-[var(--text-muted)] max-[1050px]:[&_span:last-child]:hidden">
          <span>{requestActive || status === 'executing' ? t('input_queue_next') : t('input_describe')}</span>
          <span>{requestActive || status === 'executing' ? t('input_queue_hint') : t('input_send_hint')}</span>
        </div>}
        <div className={cls("flex items-end gap-1.5", compact && "rounded-[18px] bg-[#302f2c] p-2 shadow-[0_8px_24px_rgba(0,0,0,0.32)]")}>
          <textarea
            ref={inputRef as React.RefObject<HTMLTextAreaElement>}
            className={cls(
              "max-h-[200px] min-h-[42px] flex-1 resize-none rounded-[9px] border border-[var(--border)] bg-[var(--bg-base)] px-3 py-[9px] font-[inherit] text-[13px] text-[var(--text-primary)] outline-none focus:border-[var(--accent)] focus:shadow-[0_0_0_2px_var(--accent-dim)]",
              compact && "min-h-[44px] border-0 bg-transparent px-1 py-1 text-[13px] focus:shadow-none",
            )}
            placeholder={t('ai_input_placeholder')}
            value={input}
            onChange={handleInputChange}
            onKeyDown={handleKeyDown}
            rows={2}
          />
          <button
            className={cls(
              "flex h-10 w-9 cursor-pointer items-center justify-center rounded-lg border-0 bg-[var(--brand)] text-white transition-[background,opacity] duration-[var(--transition-fast)] hover:not-disabled:bg-[var(--brand-hover)] disabled:cursor-not-allowed disabled:opacity-40",
              sendButtonMode === 'stop' && "bg-[#b91c1c] hover:not-disabled:bg-[#dc2626]",
              sendButtonMode === 'stopping' && "bg-[#7f1d1d]",
              compact && "h-8 w-8 shrink-0 rounded-full bg-[#9a9a9a] text-[#202020] hover:not-disabled:bg-white",
              compact && sendButtonMode === 'stop' && "bg-[#ef4444] text-white hover:not-disabled:bg-[#f87171]",
              compact && sendButtonMode === 'stopping' && "bg-[#7f1d1d] text-white",
            )}
            onClick={handleComposerAction}
            disabled={sendButtonDisabled}
            aria-label={sendButtonLabel}
            title={sendButtonLabel}
          >
            {sendButtonMode === 'stop' ? <IconStop /> : sendButtonMode === 'stopping' ? <IconLoader className={commonSpinnerClass} /> : <IconSend />}
          </button>
        </div>
        <div className={cls("mt-2 flex items-center gap-2", compact && "px-1 text-[11px]")}>
          <ApprovalModeMenu mode={approvalMode} onChange={setApprovalMode} />
          <CompactModelMenu
            thinkingEffort={thinkingEffort}
            onThinkingEffortChange={setThinkingEffort}
            compact={compact}
          />
        </div>
        {compact && (
          <div className="mt-1 flex items-center gap-1.5 px-1 text-[11px] text-[var(--text-muted)]">
            <IconFile size={12} />
            <span>{t('ai_scope_local_project')}</span>
          </div>
        )}
      </div>
    </div>
  );
}
