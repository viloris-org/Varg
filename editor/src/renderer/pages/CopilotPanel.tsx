import React, { useCallback, useEffect, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { rpc, streamCopilotPlan } from '../api';
import { useTranslation } from '../i18n';
import {
  buttonClass,
  copilotPlanBadgeClass,
  copilotPlanBadgeReadClass,
  copilotStatusBadgeClass,
} from '../uiClasses';
import {
  IconSend, IconBot, IconCheck, IconX, IconAlertCircle,
  IconChevronDown, IconChevronRight, IconInfo, IconLoader,
} from '../icons';

// ─── Types ──────────────────────────────────────────────────────────────────

interface CopilotOperation {
  index: number;
  preview: string;
  requires_write: boolean;
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

const panelClass = 'flex h-full flex-col overflow-hidden text-xs';
const messagesClass = 'flex flex-1 flex-col gap-1.5 overflow-y-auto p-2 [&::-webkit-scrollbar]:w-1 [&::-webkit-scrollbar-thumb]:rounded-sm [&::-webkit-scrollbar-thumb]:bg-[var(--border)]';
const emptyClass = 'flex flex-col items-center gap-2 px-4 py-6 text-center text-[var(--text-muted)] [&_p]:max-w-[200px] [&_p]:text-[11px] [&_p]:leading-normal [&_svg]:h-8 [&_svg]:w-8 [&_svg]:opacity-40';
const messageClass = 'flex gap-2 rounded-[var(--radius-md)] px-2 py-1.5 animate-[fadeIn_150ms_ease]';
const messageAvatarClass = 'flex h-[22px] w-[22px] flex-shrink-0 items-center justify-center rounded-full bg-[var(--accent-dim)] text-[11px] font-bold text-[var(--accent)]';
const messageContentClass = 'min-w-0 flex-1 break-words text-xs leading-normal text-[var(--text-primary)]';
const planClass = 'my-1 overflow-hidden rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-surface)]';
const planHeaderClass = 'flex items-center gap-1.5 border-b border-[var(--border)] bg-[var(--bg-hover)] px-2.5 py-2 text-[11px] font-semibold text-[var(--text-secondary)]';
const planItemClass = 'flex cursor-pointer items-center gap-2 px-2.5 py-1.5 text-xs transition-[background] duration-[var(--transition-fast)] hover:bg-[var(--bg-hover)]';
const planCheckboxClass = 'h-3.5 w-3.5 flex-shrink-0 accent-[var(--accent)]';
const planPreviewClass = 'min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-[var(--text-primary)]';
const planActionsClass = 'flex gap-1.5 border-t border-[var(--border)] px-2.5 py-2';
const executingClass = 'flex items-center gap-2 px-2.5 py-2 text-[11px] font-medium text-[var(--accent)]';
const errorClass = 'flex items-center gap-1.5 rounded-[var(--radius-sm)] border border-[rgba(239,68,68,0.2)] bg-[var(--danger-dim)] px-2.5 py-1.5 text-[11px] text-[var(--danger)]';
const traceClass = 'flex-shrink-0 border-t border-[var(--border)]';
const traceToggleClass = 'flex w-full cursor-pointer items-center gap-1.5 border-0 bg-transparent px-2.5 py-1.5 font-[var(--font-sans)] text-[11px] text-[var(--text-secondary)] transition-[background] duration-[var(--transition-fast)] hover:bg-[var(--bg-hover)]';
const traceEntriesClass = 'max-h-[120px] overflow-y-auto px-2.5 pt-1 pb-2';
const traceEntryClass = 'flex gap-2 py-0.5 font-[var(--font-mono)] text-[10px]';
const traceToolClass = 'min-w-20 text-[var(--text-muted)]';
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
  !requiresWrite && 'opacity-70',
);

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
        if (!op.requires_write) autoApproved.add(op.index);
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

  return (
    <div className={panelClass}>
      {/* Messages */}
      <div ref={scrollRef} className={messagesClass}>
        {messages.length === 0 && (
          <div className={emptyClass}>
            <IconBot />
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
              <span>{t('copilot_plan_title')}</span>
              <StatusBadge status={status} />
            </div>
            {plan.operations.map((op) => (
              <label
                key={op.index}
                className={planItemVariantClass(op.requires_write)}
              >
                {op.requires_write ? (
                  <input
                    type="checkbox"
                    className={planCheckboxClass}
                    checked={approved.has(op.index)}
                    onChange={() => toggleApproval(op.index)}
                  />
                ) : (
                  <span className={copilotPlanBadgeReadClass}><IconCheck /></span>
                )}
                <span className={planPreviewClass}>{op.preview}</span>
                {op.requires_write && (
                  <span className={copilotPlanBadgeClass}>{t('copilot_badge_write')}</span>
                )}
              </label>
            ))}
            <div className={planActionsClass}>
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
                {t('copilot_reject')}
              </button>
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
