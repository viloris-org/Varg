import React, { useCallback, useEffect, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { rpc, streamCopilotPlan } from '../api';
import { useTranslation } from '../i18n';
import {
  buttonClass,
  copilotPlanBadgeReadClass,
  copilotStatusBadgeClass,
} from '../uiClasses';
import {
  IconSend, IconBot, IconCheck, IconX, IconAlertCircle,
  IconChevronDown, IconChevronRight, IconInfo, IconLoader,
  IconCode, IconFile, IconMessageSquare, IconSparkles,
} from '../icons';

// ─── Types ──────────────────────────────────────────────────────────────────

interface CopilotOperation {
  index: number;
  preview: string;
  requires_write: boolean;
  id?: string;
  kind?: string;
  permission_kind?: 'read' | 'write' | 'command' | 'unsupported';
  target?: string;
  risk_hint?: 'low' | 'medium' | 'high' | 'unsupported' | string;
  requires_approval?: boolean;
  undo_hint?: 'available' | 'unavailable' | 'unknown' | string;
  validation_hint?: string | null;
}

interface CopilotPlan {
  message: string;
  operations: CopilotOperation[];
  read_only: boolean;
  requires_write: boolean;
}

interface TraceEntry {
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
}

type CopilotStatus = 'idle' | 'planning' | 'ready' | 'executing' | 'complete' | 'error';

const cx = (...classes: Array<string | false | null | undefined>) => classes.filter(Boolean).join(' ');

const panelClass = 'flex h-full flex-col overflow-hidden bg-[var(--bg-surface)] text-xs';
const contextBarClass = 'flex flex-shrink-0 flex-col gap-2 border-b border-[var(--border)] bg-[var(--bg-base)] px-2.5 py-2';
const contextTitleClass = 'flex items-center justify-between gap-2 text-[10px] font-semibold uppercase tracking-[0.45px] text-[var(--text-muted)]';
const contextChipsClass = 'flex min-w-0 flex-wrap gap-1.5';
const contextChipClass = 'inline-flex h-6 max-w-full items-center gap-1.5 rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-surface)] px-2 text-[10px] font-medium text-[var(--text-secondary)] [&_span]:min-w-0 [&_span]:overflow-hidden [&_span]:text-ellipsis [&_span]:whitespace-nowrap [&_svg]:shrink-0 [&_svg]:text-[var(--text-muted)]';
const messagesClass = 'flex flex-1 flex-col gap-2 overflow-y-auto p-2.5 [&::-webkit-scrollbar]:w-1 [&::-webkit-scrollbar-thumb]:rounded-sm [&::-webkit-scrollbar-thumb]:bg-[var(--border)]';
const emptyClass = 'flex flex-col items-center gap-2.5 px-4 py-7 text-center text-[var(--text-muted)] [&_p]:max-w-[230px] [&_p]:text-[11px] [&_p]:leading-normal [&_svg]:h-8 [&_svg]:w-8 [&_svg]:opacity-45';
const messageClass = 'flex gap-2 rounded-[var(--radius-md)] px-2 py-1.5 animate-[fadeIn_150ms_ease]';
const messageAvatarClass = 'flex h-[22px] w-[22px] flex-shrink-0 items-center justify-center rounded-full bg-[var(--accent-dim)] text-[11px] font-bold text-[var(--accent)]';
const messageContentClass = 'min-w-0 flex-1 break-words text-xs leading-normal text-[var(--text-primary)]';
const planClass = 'my-1 overflow-hidden rounded-[var(--radius-md)] border border-[var(--border-light)] bg-[var(--bg-surface)] shadow-[var(--shadow-sm)]';
const planHeaderClass = 'flex items-center gap-1.5 border-b border-[var(--border)] bg-[var(--bg-hover)] px-2.5 py-2 text-[11px] font-semibold text-[var(--text-secondary)]';
const planHeaderTextClass = 'min-w-0 flex-1';
const planIntroClass = 'border-b border-[var(--border)] px-2.5 py-2 text-[11px] leading-normal text-[var(--text-muted)]';
const planListClass = 'grid gap-px bg-[var(--border)]';
const planItemClass = 'grid cursor-pointer grid-cols-[16px_minmax(0,1fr)] gap-2 bg-[var(--bg-surface)] px-2.5 py-2.5 text-xs transition-[background] duration-[var(--transition-fast)] hover:bg-[var(--bg-hover)]';
const planCheckboxClass = 'h-3.5 w-3.5 flex-shrink-0 accent-[var(--accent)]';
const planPreviewClass = 'min-w-0 text-[12px] font-medium leading-normal text-[var(--text-primary)]';
const planMetaClass = 'mt-1.5 flex min-w-0 flex-wrap items-center gap-1.5';
const planTargetClass = 'min-w-0 max-w-full overflow-hidden text-ellipsis whitespace-nowrap rounded-[3px] bg-[var(--bg-base)] px-1.5 py-px font-[var(--font-mono)] text-[10px] text-[var(--text-muted)]';
const planActionsClass = 'flex items-center justify-between gap-1.5 border-t border-[var(--border)] px-2.5 py-2';
const planActionGroupClass = 'flex min-w-0 gap-1.5';
const planFooterMetaClass = 'min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-[10px] text-[var(--text-muted)]';
const executingClass = 'flex items-center gap-2 px-2.5 py-2 text-[11px] font-medium text-[var(--accent)]';
const errorClass = 'flex items-center gap-1.5 rounded-[var(--radius-sm)] border border-[rgba(239,68,68,0.2)] bg-[var(--danger-dim)] px-2.5 py-1.5 text-[11px] text-[var(--danger)]';
const traceClass = 'flex-shrink-0 border-t border-[var(--border)]';
const traceToggleClass = 'flex w-full cursor-pointer items-center gap-1.5 border-0 bg-transparent px-2.5 py-1.5 font-[var(--font-sans)] text-[11px] text-[var(--text-secondary)] transition-[background] duration-[var(--transition-fast)] hover:bg-[var(--bg-hover)]';
const traceEntriesClass = 'max-h-[120px] overflow-y-auto px-2.5 pt-1 pb-2';
const traceEntryClass = 'grid grid-cols-[minmax(72px,0.8fr)_minmax(0,1fr)] gap-2 py-0.5 font-[var(--font-mono)] text-[10px]';
const traceToolClass = 'min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-[var(--text-muted)]';
const optionsClass = 'flex-shrink-0 border-t border-[var(--border)] px-2.5 py-1';
const autoAcceptClass = 'flex cursor-pointer items-center gap-1.5 text-[10px] text-[var(--text-muted)]';
const autoAcceptCheckboxClass = 'h-3 w-3 accent-[var(--accent)]';
const inputRowClass = 'flex flex-shrink-0 gap-1 border-t border-[var(--border)] px-2 py-1.5';
const inputClass = 'flex-1 rounded-[var(--radius-sm)] border border-[var(--border)] bg-[var(--bg-base)] px-2.5 py-1.5 font-[var(--font-sans)] text-xs text-[var(--text-primary)] outline-none transition-[border-color] duration-[var(--transition-fast)] placeholder:text-[var(--text-muted)] focus:border-[var(--accent)] disabled:cursor-not-allowed disabled:opacity-50';
const sendButtonClass = 'flex h-7 w-7 flex-shrink-0 cursor-pointer items-center justify-center rounded-[var(--radius-sm)] border-0 bg-[var(--brand)] text-white transition-[background] duration-[var(--transition-fast)] hover:not-disabled:bg-[var(--brand-hover)] disabled:cursor-default disabled:opacity-40';

const messageContainerClass = (role: string) => cx(
  messageClass,
  role === 'user' ? 'bg-[var(--bg-hover)]' : 'bg-transparent',
);

const messageAvatarVariantClass = (role: string) => cx(
  messageAvatarClass,
  role === 'user' && 'bg-[var(--bg-hover)] text-[var(--text-secondary)]',
);

const planItemVariantClass = (requiresWrite: boolean) => cx(
  planItemClass,
  !requiresWrite && 'opacity-80',
);

const badgeClass = (variant: 'read' | 'write' | 'command' | 'high' | 'medium' | 'low' | 'neutral' | 'unsupported') => cx(
  'inline-flex h-[18px] items-center rounded-[3px] px-1.5 text-[9px] font-bold uppercase tracking-[0.35px]',
  variant === 'read' && 'bg-[var(--success-dim)] text-[var(--success)]',
  variant === 'write' && 'bg-[var(--warning-dim)] text-[var(--warning)]',
  variant === 'command' && 'bg-[var(--accent-dim)] text-[var(--accent-hover)]',
  variant === 'low' && 'bg-[var(--success-dim)] text-[var(--success)]',
  variant === 'medium' && 'bg-[var(--warning-dim)] text-[var(--warning)]',
  variant === 'high' && 'bg-[var(--danger-dim)] text-[var(--danger)]',
  variant === 'unsupported' && 'bg-[var(--danger-dim)] text-[var(--danger)]',
  variant === 'neutral' && 'bg-[var(--bg-hover)] text-[var(--text-muted)]',
);

function normalizePermission(op: CopilotOperation): 'read' | 'write' | 'command' | 'unsupported' {
  if (op.permission_kind) return op.permission_kind;
  return op.requires_write ? 'write' : 'read';
}

function normalizeRisk(op: CopilotOperation): 'low' | 'medium' | 'high' | 'unsupported' {
  const risk = op.risk_hint;
  if (risk === 'medium' || risk === 'high' || risk === 'unsupported') return risk;
  return op.requires_write ? 'medium' : 'low';
}

function permissionLabel(kind: string): string {
  if (kind === 'read') return 'READ';
  if (kind === 'write') return 'WRITE';
  if (kind === 'command') return 'COMMAND';
  if (kind === 'unsupported') return 'BLOCKED';
  return kind.toUpperCase();
}

function riskLabel(kind: string): string {
  if (kind === 'low') return 'LOW RISK';
  if (kind === 'medium') return 'MED RISK';
  if (kind === 'high') return 'HIGH RISK';
  if (kind === 'unsupported') return 'UNSUPPORTED';
  return kind.toUpperCase();
}

const traceResultClass = (result: string) => cx(
  'text-[var(--text-secondary)]',
  result === 'applied' && 'text-[var(--accent)]',
  result.startsWith('failed') && 'text-[var(--danger)]',
);

// ─── Copilot Message ─────────────────────────────────────────────────────────

function MessageBubble({ role, content }: { role: string; content: string }) {
  return (
    <div className={messageContainerClass(role)}>
      <div className={messageAvatarVariantClass(role)}>
        {role === 'assistant' ? <IconBot /> : <span>U</span>}
      </div>
      <div className={messageContentClass}>
        {role === 'assistant' ? <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown> : content}
      </div>
    </div>
  );
}

function ContextBar({ hasDiagnostics }: { hasDiagnostics: boolean }) {
  const { t } = useTranslation();
  const chips = [
    { label: t('copilot_context_project'), icon: <IconFile /> },
    { label: t('copilot_context_scene'), icon: <IconSparkles /> },
    { label: t('copilot_context_selection'), icon: <IconCode /> },
    ...(hasDiagnostics ? [{ label: t('copilot_context_diagnostics'), icon: <IconAlertCircle /> }] : []),
  ];

  return (
    <div className={contextBarClass}>
      <div className={contextTitleClass}>
        <span>{t('copilot_context_title')}</span>
        <span>{t('copilot_context_edit_hint')}</span>
      </div>
      <div className={contextChipsClass}>
        {chips.map((chip) => (
          <span key={chip.label} className={contextChipClass} title={chip.label}>
            {chip.icon}
            <span>{chip.label}</span>
          </span>
        ))}
      </div>
    </div>
  );
}

function OperationCard({
  op,
  approved,
  onToggle,
}: {
  op: CopilotOperation;
  approved: boolean;
  onToggle: (index: number) => void;
}) {
  const permission = normalizePermission(op);
  const risk = normalizeRisk(op);
  const requiresApproval = op.requires_approval ?? op.requires_write;
  const canApply = permission !== 'unsupported';

  return (
    <label className={planItemVariantClass(op.requires_write)}>
      <span className="pt-0.5">
        {requiresApproval && canApply ? (
          <input
            type="checkbox"
            className={planCheckboxClass}
            checked={approved}
            onChange={() => onToggle(op.index)}
          />
        ) : (
          <span className={copilotPlanBadgeReadClass}><IconCheck /></span>
        )}
      </span>
      <span className="min-w-0">
        <span className={planPreviewClass}>{op.preview}</span>
        <span className={planMetaClass}>
          <span className={badgeClass(permission)}>{permissionLabel(permission)}</span>
          <span className={badgeClass(risk)}>{riskLabel(risk)}</span>
          {op.kind && <span className={badgeClass('neutral')}>{op.kind}</span>}
          {op.undo_hint && <span className={badgeClass('neutral')}>UNDO {op.undo_hint}</span>}
          {op.validation_hint && <span className={badgeClass('neutral')}>VALIDATE</span>}
          {op.target && <span className={planTargetClass} title={op.target}>{op.target}</span>}
        </span>
      </span>
    </label>
  );
}

// ─── Status Badge ────────────────────────────────────────────────────────────

function StatusBadge({ status }: { status: CopilotStatus }) {
  const { t } = useTranslation();
  const config: Record<CopilotStatus, { label: string; variant: Exclude<CopilotStatus, 'idle'> | null }> = {
    idle: { label: '', variant: null },
    planning: { label: t('copilot_status_planning'), variant: 'planning' },
    ready: { label: t('copilot_status_ready'), variant: 'ready' },
    executing: { label: t('copilot_status_executing'), variant: 'executing' },
    complete: { label: t('copilot_status_complete'), variant: 'complete' },
    error: { label: t('copilot_status_error'), variant: 'error' },
  };
  const c = config[status];
  if (!c.label || !c.variant) return null;
  return <span className={copilotStatusBadgeClass(c.variant)}>{c.label}</span>;
}

// ─── Copilot Panel ───────────────────────────────────────────────────────────

export default function CopilotPanel() {
  const { t } = useTranslation();
  const [input, setInput] = useState('');
  const [messages, setMessages] = useState<{ role: string; content: string; id?: string }[]>([]);
  const [status, setStatus] = useState<CopilotStatus>('idle');
  const [plan, setPlan] = useState<CopilotPlan | null>(null);
  const [approved, setApproved] = useState<Set<number>>(new Set());
  const [traceExpanded, setTraceExpanded] = useState(false);
  const [trace, setTrace] = useState<TraceEntry[]>([]);
  const [autoAccept, setAutoAccept] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const nextMsgId = useRef(1);

  // Auto-scroll to bottom
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages, plan, trace]);

  // Focus input on mount
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const executeApprovedRef = useRef<((approvedSet?: Set<number>) => Promise<void>) | null>(null);

  // ── Submit prompt ──

  const submitPrompt = useCallback(async (prompt: string) => {
    if (!prompt.trim() || status === 'planning' || status === 'executing') return;

    setMessages(prev => [...prev, { role: 'user', content: prompt }]);
    setInput('');
    setStatus('planning');
    setPlan(null);
    setApproved(new Set());
    setTrace([]);
    setTraceExpanded(false);
    setErrorMsg(null);

    const streamingId = String(nextMsgId.current++);
    setMessages(prev => [...prev, {
      role: 'assistant',
      content: '',
      id: streamingId,
    }]);

    try {
      let receivedDelta = false;
      const streamHandle = streamCopilotPlan<CopilotPlan>({ prompt }, (delta, kind) => {
        if (kind !== 'text') return;
        setMessages(prev => prev.map(msg => {
          if ((msg as any).id !== streamingId) return msg;
          return { ...msg, content: receivedDelta ? msg.content + delta : delta };
        }));
        receivedDelta = true;
      });
      const result = await streamHandle.promise;

      setPlan(result.operations.length > 0 ? result : null);
      const autoApproved = new Set<number>();
      result.operations.forEach((op) => {
        const permission = normalizePermission(op);
        const requiresApproval = op.requires_approval ?? op.requires_write;
        if (permission !== 'unsupported' && !requiresApproval) autoApproved.add(op.index);
      });
      setApproved(autoApproved);
      setStatus('ready');

      const finalContent = result.message || (result.operations.length > 0
        ? t('copilot_planned_ops').replace('{count}', String(result.operations.length))
        : t('copilot_no_ops'));
      setMessages(prev => prev.map(msg => (msg as any).id === streamingId
        ? { ...msg, content: finalContent }
        : msg));

      // Auto-execute if all ops are read-only and auto-accept is on
      if (autoAccept && result.operations.length > 0 && !result.requires_write) {
        await executeApprovedRef.current?.(autoApproved);
      }
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err.message || 'Unknown error';
      setStatus('error');
      setErrorMsg(msg);
      setMessages(prev => prev.map(m => (m as any).id === streamingId
        ? { ...m, content: `Error: ${msg}` }
        : m));
    }
  }, [status, autoAccept]);

  // ── Execute approved operations ──

  const executeApproved = useCallback(async (approvedSet?: Set<number>) => {
    const indices = Array.from(approvedSet ?? approved);
    if (indices.length === 0) return;

    setStatus('executing');
    setErrorMsg(null);

    try {
      const result = await rpc<ApplyResult>('copilot/apply', {
        approved_indices: indices,
      });

      setTrace(result.trace_entries);
      if (result.trace_entries.length > 0) {
        setTraceExpanded(true);
      }

      setStatus('complete');

      const summary = result.summary
        ? `✅ ${result.summary}`
        : `✅ ${t('copilot_applied_ops').replace('{count}', String(result.operations_performed))}`;
      setMessages(prev => [...prev, { role: 'assistant', content: summary }]);
      setPlan(null);
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err.message || 'Unknown error';
      setStatus('error');
      setErrorMsg(msg);
      setMessages(prev => [...prev, {
        role: 'assistant',
        content: `❌ ${msg}`,
      }]);
    }
  }, [approved]);

  executeApprovedRef.current = executeApproved;

  // ── Toggle approval for an operation ──

  const toggleApproval = useCallback((index: number) => {
    setApproved(prev => {
      const next = new Set(prev);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      return next;
    });
  }, []);

  // ── Keyboard handler ──

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      submitPrompt(input);
    }
  }, [input, submitPrompt]);

  // ── Render ──

  const approvedCount = approved.size;
  const hasPlan = plan && plan.operations.length > 0;
  const writeCount = plan?.operations.filter(op => op.requires_write).length ?? 0;
  const readCount = plan?.operations.filter(op => !op.requires_write).length ?? 0;

  return (
    <div className={panelClass}>
      <ContextBar hasDiagnostics={trace.some(entry => entry.result.startsWith('failed')) || Boolean(errorMsg)} />

      {/* Messages */}
      <div ref={scrollRef} className={messagesClass}>
        {messages.length === 0 && (
          <div className={emptyClass}>
            <IconMessageSquare />
            <p>{t('copilot_empty_hint')}</p>
          </div>
        )}
        {messages.map((msg, i) => (
          <MessageBubble key={msg.id ?? i} role={msg.role} content={msg.content} />
        ))}

        {/* Plan Preview */}
        {hasPlan && status === 'ready' && (
          <div className={planClass}>
            <div className={planHeaderClass}>
              <IconInfo />
              <span className={planHeaderTextClass}>{t('copilot_plan_title')}</span>
              <StatusBadge status={status} />
            </div>
            <div className={planIntroClass}>
              {t('copilot_plan_intro_detailed')
                .replace('{write}', String(writeCount))
                .replace('{read}', String(readCount))}
            </div>
            <div className={planListClass}>
            {plan.operations.map((op) => (
              <OperationCard
                key={op.index}
                op={op}
                approved={approved.has(op.index)}
                onToggle={toggleApproval}
              />
            ))}
            </div>
            <div className={planActionsClass}>
              <span className={planFooterMetaClass}>
                {t('copilot_plan_selected').replace('{count}', String(approvedCount))}
              </span>
              <span className={planActionGroupClass}>
                <button
                  className={buttonClass('primary', 'sm')}
                  disabled={approvedCount === 0}
                  onClick={() => executeApproved()}
                >
                  {t('copilot_apply').replace('{count}', String(approvedCount))}
                </button>
                <button
                  className={buttonClass('ghost', 'sm')}
                  onClick={() => { setPlan(null); setStatus('idle'); }}
                >
                  <IconX />
                  {t('copilot_reject')}
                </button>
              </span>
            </div>
          </div>
        )}

        {/* Executing indicator */}
        {status === 'executing' && (
          <div className={executingClass}>
            <IconLoader className="animate-spin" />
            <span>{t('copilot_executing')}</span>
          </div>
        )}

        {/* Error */}
        {errorMsg && (
          <div className={errorClass}>
            <IconAlertCircle />
            <span>{errorMsg}</span>
          </div>
        )}
      </div>

      {/* Trace (collapsible) */}
      {trace.length > 0 && (
        <div className={traceClass}>
          <button
            className={traceToggleClass}
            onClick={() => setTraceExpanded(!traceExpanded)}
          >
            {traceExpanded ? <IconChevronDown /> : <IconChevronRight />}
            <span>{t('copilot_trace')} ({trace.length})</span>
          </button>
          {traceExpanded && (
            <div className={traceEntriesClass}>
              {trace.map((entry, i) => (
                <div key={i} className={traceEntryClass}>
                  <span className={traceToolClass}>{entry.tool}</span>
                  <span className={traceResultClass(entry.result)}>{entry.result}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Auto-accept toggle */}
      <div className={optionsClass}>
        <label className={autoAcceptClass}>
          <input
            type="checkbox"
            className={autoAcceptCheckboxClass}
            checked={autoAccept}
            onChange={(e) => setAutoAccept(e.target.checked)}
          />
          <span>{t('copilot_auto_accept')}</span>
        </label>
      </div>

      {/* Input */}
      <div className={inputRowClass}>
        <textarea
          ref={inputRef as React.RefObject<HTMLTextAreaElement>}
          className={inputClass}
          placeholder={t('copilot_input_placeholder')}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={status === 'planning' || status === 'executing'}
          rows={2}
        />
        <button
          className={sendButtonClass}
          onClick={() => submitPrompt(input)}
          disabled={!input.trim() || status === 'planning' || status === 'executing'}
          title={t('copilot_send')}
        >
          <IconSend />
        </button>
      </div>
    </div>
  );
}
