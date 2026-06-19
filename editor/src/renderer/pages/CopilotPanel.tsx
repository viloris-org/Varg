import React, { useCallback, useEffect, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import { rpc, streamCopilotPlan } from '../api';
import { useTranslation } from '../i18n';
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

// ─── Copilot Message ─────────────────────────────────────────────────────────

function MessageBubble({ role, content }: { role: string; content: string }) {
  return (
    <div className={`copilot-message copilot-message-${role}`}>
      <div className="copilot-message-avatar">
        {role === 'assistant' ? <IconBot /> : <span>U</span>}
      </div>
      <div className="copilot-message-content">
        {role === 'assistant' ? <ReactMarkdown>{content}</ReactMarkdown> : content}
      </div>
    </div>
  );
}

// ─── Status Badge ────────────────────────────────────────────────────────────

function StatusBadge({ status }: { status: CopilotStatus }) {
  const { t } = useTranslation();
  const config: Record<CopilotStatus, { label: string; className: string }> = {
    idle: { label: '', className: '' },
    planning: { label: t('copilot_status_planning'), className: 'badge-copilot-planning' },
    ready: { label: t('copilot_status_ready'), className: 'badge-copilot-ready' },
    executing: { label: t('copilot_status_executing'), className: 'badge-copilot-executing' },
    complete: { label: t('copilot_status_complete'), className: 'badge-copilot-complete' },
    error: { label: t('copilot_status_error'), className: 'badge-copilot-error' },
  };
  const c = config[status];
  if (!c.label) return null;
  return <span className={`badge-copilot ${c.className}`}>{c.label}</span>;
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
    <div className="copilot-panel">
      {/* Messages */}
      <div ref={scrollRef} className="copilot-messages">
        {messages.length === 0 && (
          <div className="copilot-empty">
            <IconBot />
            <p>{t('copilot_empty_hint')}</p>
          </div>
        )}
        {messages.map((msg, i) => (
          <MessageBubble key={msg.id ?? i} role={msg.role} content={msg.content} />
        ))}

        {/* Plan Preview */}
        {hasPlan && status === 'ready' && (
          <div className="copilot-plan">
            <div className="copilot-plan-header">
              <IconInfo />
              <span>{t('copilot_plan_title')}</span>
              <StatusBadge status={status} />
            </div>
            {plan.operations.map((op) => (
              <label
                key={op.index}
                className={`copilot-plan-item ${!op.requires_write ? 'auto-approved' : ''}`}
              >
                {op.requires_write ? (
                  <input
                    type="checkbox"
                    checked={approved.has(op.index)}
                    onChange={() => toggleApproval(op.index)}
                  />
                ) : (
                  <span className="copilot-plan-badge-read"><IconCheck /></span>
                )}
                <span className="copilot-plan-preview">{op.preview}</span>
                {op.requires_write && (
                  <span className="copilot-plan-badge">{t('copilot_badge_write')}</span>
                )}
              </label>
            ))}
            <div className="copilot-plan-actions">
              <button
                className="btn btn-primary btn-sm"
                disabled={approvedCount === 0}
                onClick={() => executeApproved()}
              >
                {t('copilot_apply').replace('{count}', String(approvedCount))}
              </button>
              <button
                className="btn btn-ghost btn-sm"
                onClick={() => { setPlan(null); setStatus('idle'); }}
              >
                {t('copilot_reject')}
              </button>
            </div>
          </div>
        )}

        {/* Executing indicator */}
        {status === 'executing' && (
          <div className="copilot-executing">
            <IconLoader className="spin-icon" />
            <span>{t('copilot_executing')}</span>
          </div>
        )}

        {/* Error */}
        {errorMsg && (
          <div className="copilot-error">
            <IconAlertCircle />
            <span>{errorMsg}</span>
          </div>
        )}
      </div>

      {/* Trace (collapsible) */}
      {trace.length > 0 && (
        <div className="copilot-trace">
          <button
            className="copilot-trace-toggle"
            onClick={() => setTraceExpanded(!traceExpanded)}
          >
            {traceExpanded ? <IconChevronDown /> : <IconChevronRight />}
            <span>{t('copilot_trace')} ({trace.length})</span>
          </button>
          {traceExpanded && (
            <div className="copilot-trace-entries">
              {trace.map((entry, i) => (
                <div key={i} className={`copilot-trace-entry copilot-trace-${entry.result === 'applied' ? 'success' : entry.result.startsWith('failed') ? 'fail' : ''}`}>
                  <span className="copilot-trace-tool">{entry.tool}</span>
                  <span className="copilot-trace-result">{entry.result}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Auto-accept toggle */}
      <div className="copilot-options">
        <label className="copilot-auto-accept">
          <input
            type="checkbox"
            checked={autoAccept}
            onChange={(e) => setAutoAccept(e.target.checked)}
          />
          <span>{t('copilot_auto_accept')}</span>
        </label>
      </div>

      {/* Input */}
      <div className="copilot-input-row">
        <textarea
          ref={inputRef as React.RefObject<HTMLTextAreaElement>}
          className="copilot-input"
          placeholder={t('copilot_input_placeholder')}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={status === 'planning' || status === 'executing'}
          rows={2}
        />
        <button
          className="copilot-send-btn"
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
