import React, { useCallback, useContext, useEffect, useMemo, useRef, useState, createContext } from 'react';
import ReactMarkdown from 'react-markdown';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { vscDarkPlus } from 'react-syntax-highlighter/dist/esm/styles/prism';
import { rpc, streamCopilotPlan } from '../api';
import { useTranslation } from '../i18n';
import {
  IconSend, IconBot, IconCheck, IconX, IconAlertCircle,
  IconChevronDown, IconChevronRight, IconInfo, IconLoader,
  IconSave, IconUndo, IconPlay, IconSettings, IconSparkles, IconRefresh,
  IconBrain,
} from '../icons';

// ─── Types ──────────────────────────────────────────────────────────────────

export interface CopilotOperation {
  index: number;
  preview: string;
  requires_write: boolean;
  permission_kind: 'read' | 'write' | 'command';
  command?: string | null;
  permanently_allowed?: boolean;
}

export interface CopilotPlan {
  message: string;
  operations: CopilotOperation[];
  read_only: boolean;
  requires_write: boolean;
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
  needs_continuation?: boolean;
}

export interface CompletedChangeBundle {
  summary: string;
  operationsPerformed: number;
  traceEntries: TraceEntry[];
}

export type AiStatus = 'idle' | 'thinking' | 'ready' | 'executing' | 'complete' | 'error';
type AiWorkspaceView = 'chat' | 'tasks' | 'changes';
type ThinkingEffort = 'off' | 'low' | 'medium' | 'high';

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
  type: 'plan' | 'trace' | 'graph' | 'entity-list' | 'error';
  data: any;
}

interface ActiveToolCall {
  id: string;
  name: string;
  argumentsPreview: string;
  complete: boolean;
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

function ModelSelector() {
  const { t } = useTranslation();
  const [currentModel, setCurrentModel] = useState<string>('');
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [discoveryError, setDiscoveryError] = useState<string | null>(null);

  const loadModels = useCallback(async () => {
    setLoading(true);
    setDiscoveryError(null);
    try {
      const settings = await rpc<CopilotSettingsFull>('app/get_copilot_settings').catch(() => null);
      if (!settings) { setLoading(false); return; }
      setCurrentModel(settings.model);

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

  const handleChange = useCallback(async (modelId: string) => {
    setCurrentModel(modelId);
    const settings = await rpc<CopilotSettingsFull>('app/get_copilot_settings').catch(() => null);
    if (settings) {
      await rpc('app/update_copilot_settings', { ...settings, model: modelId });
    }
  }, []);

  if (loading) return <span className="ai-model-selector-loading"><IconLoader className="spin-icon" /></span>;

  const known = models.some(m => m.id === currentModel);

  return (
    <div className="ai-model-selector">
      <select
        className="ai-model-select"
        value={known ? currentModel : currentModel ? '__custom__' : ''}
        title={discoveryError ?? t('model_available_title')}
        onChange={(e) => {
          const val = e.target.value;
          if (val === '__refresh__') { loadModels(); return; }
          handleChange(val === '__custom__' ? currentModel : val);
        }}
      >
        {models.length === 0 && !currentModel && <option value="">{t('model_none')}</option>}
        {models.map(m => (
          <option key={m.id} value={m.id}>{m.display_name}</option>
        ))}
        {!known && currentModel && <option value="__custom__">{currentModel}</option>}
        {discoveryError && <option value="__discovery_error__" disabled>{t('model_discovery_failed')}</option>}
        <option value="__refresh__">↻ {t('model_refresh')}</option>
      </select>
    </div>
  );
}

// ─── Context Bar ────────────────────────────────────────────────────────────

function ContextBar({ projectName, selectedEntity, sceneObjectCount, onSettingsClick, onNewChat, conversationTurns }: {
  projectName?: string;
  selectedEntity?: string | null;
  sceneObjectCount: number;
  onSettingsClick?: () => void;
  onNewChat?: () => void;
  conversationTurns: number;
}) {
  const { t } = useTranslation();
  return (
    <div className="ai-context-bar">
      {projectName && <span className="ai-context-tag">{projectName}</span>}
      <span className="ai-context-tag">{sceneObjectCount} {t('label_objects')}</span>
      {selectedEntity && (
        <span className="ai-context-tag ai-context-selected">@ {selectedEntity}</span>
      )}
      {conversationTurns > 0 && (
        <span className="ai-context-tag ai-context-turns">{conversationTurns} {conversationTurns !== 1 ? t('label_turns') : t('label_turn')}</span>
      )}
      <div className="ai-context-actions">
        <button className="ai-context-settings-btn" onClick={onNewChat} title={t('ai_new_chat')} aria-label={t('ai_new_chat')}>
          <IconRefresh />
        </button>
        <button className="ai-context-settings-btn" onClick={onSettingsClick} title={t('ai_settings')} aria-label={t('ai_settings')}>
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
    <div className="ai-entity-context-card">
      <button className="ai-entity-context-header" onClick={() => setExpanded(!expanded)}>
        {expanded ? <IconChevronDown /> : <IconChevronRight />}
        <span className="ai-entity-context-name">{entity.name}</span>
        <span className="ai-entity-context-tag">{entity.tag || t('entity_untagged')}</span>
        <span className="ai-entity-context-components">{comps.length} {comps.length !== 1 ? t('label_comps') : t('label_comp')}</span>
      </button>
      {expanded && (
        <div className="ai-entity-context-body">
          <div className="ai-entity-context-row">
            <span className="ai-entity-context-label">{t('prop_position')}</span>
            <span className="ai-entity-context-value">
              {pos[0].toFixed(2)}, {pos[1].toFixed(2)}, {pos[2].toFixed(2)}
            </span>
          </div>
          {comps.map((c, i) => (
            <div key={i} className="ai-entity-context-row">
              <span className="ai-entity-context-comp-badge">{c.type}</span>
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
  return (
    <div className="ai-tool-calls">
      {toolCalls.map((tc, i) => (
        <div key={i} className={`ai-tool-call ${tc.complete ? 'complete' : 'pending'}`}>
          <span className="ai-tool-call-icon">{tc.complete ? '✓' : '⟳'}</span>
          <span className="ai-tool-call-name">{tc.name}</span>
          {!tc.complete && <span className="ai-tool-call-status">{t('tool_calling')}</span>}
          {tc.complete && tc.argumentsPreview && (
            <span className="ai-tool-call-args">
              {(() => {
                try {
                  const args = JSON.parse(tc.argumentsPreview);
                  const keys = Object.keys(args);
                  if (keys.length === 0) return null;
                  const fullJson = JSON.stringify(args, null, 2);
                  const preview = keys.map(k => {
                    const v = args[k];
                    const display = typeof v === 'string' ? `"${v.slice(0, 30)}${v.length > 30 ? '...' : ''}"` : JSON.stringify(v);
                    return `${k}: ${display}`;
                  }).join(', ');
                  return <span className="ai-tool-call-args-text" title={fullJson}>({preview})</span>;
                } catch {
                  return <span className="ai-tool-call-args-text">({t('tool_parsing')})</span>;
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
    <div className={`ai-message ai-message-${msg.role}`}>
      <div className="ai-message-avatar">
        {msg.role === 'assistant' ? <IconBot /> : <span>U</span>}
      </div>
      <div className="ai-message-body">
        {msg.thinking && (
          <div className="ai-thinking-block">
            <button
              className="ai-thinking-header"
              onClick={() => setThinkingExpanded(!thinkingExpanded)}
            >
              {thinkingExpanded ? <IconChevronDown size={12} /> : <IconChevronRight size={12} />}
              <IconBrain size={12} />
               <span>{t('ai_thinking_process')}</span>
            </button>
            {thinkingExpanded && (
              <div className="ai-thinking-content">{msg.thinking}</div>
            )}
          </div>
        )}
        {msg.activeToolCalls && msg.activeToolCalls.length > 0 && (
          <ToolCallIndicator toolCalls={msg.activeToolCalls} />
        )}
        <div className="ai-message-content">
          {msg.role === 'assistant' ? (
            <ReactMarkdown
              components={{
                code({ className, children, ...props }) {
                  const match = /language-(\w+)/.exec(className || '');
                  const codeString = String(children).replace(/\n$/, '');
                  if (match) {
                    return (
                      <div className="ai-code-block">
                        <button
                          className="ai-code-copy-btn"
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
        {msg.queued && <div className="ai-message-state queued">{t('msg_queued')}</div>}
        {msg.interrupted && <div className="ai-message-state interrupted">{t('msg_interrupted')}</div>}
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

// ─── Inline Cards ───────────────────────────────────────────────────────────

function InlineCard({ card }: { card: AiCard }) {
  const [expanded, setExpanded] = useState(card.type === 'plan');

  return (
    <div className={`ai-card ai-card-${card.type}`}>
      <button className="ai-card-header" onClick={() => setExpanded(!expanded)}>
        {expanded ? <IconChevronDown /> : <IconChevronRight />}
        <span className="ai-card-type">{card.type}</span>
      </button>
      {expanded && (
        <div className="ai-card-body">
          {card.type === 'plan' && <PlanCard data={card.data} />}
          {card.type === 'trace' && <TraceCard data={card.data} />}
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
    <div className="ai-plan-card">
      {data.operations.map((op) => {
        const isRead = op.permission_kind === 'read';
        const isApproved = ctx?.approved.has(op.index);
        const isDenied = ctx?.denied.has(op.index);
        const showControls = ctx?.active && !isRead;

        return (
          <div key={op.index} className="ai-plan-item">
            <span className={`ai-plan-badge ${op.requires_write ? 'write' : 'read'}`}>
              {op.permission_kind === 'read' ? 'R' : op.permission_kind === 'command' ? 'CMD' : 'W'}
            </span>
            <span className="ai-plan-item-preview">{op.preview}</span>
            {showControls && (
              <div className="ai-plan-item-actions">
                {isApproved ? (
                  <span className="ai-plan-item-state allowed">{t('op_allowed_icon')}</span>
                ) : isDenied ? (
                  <span className="ai-plan-item-state denied">{t('op_denied_icon')}</span>
                ) : (
                  <>
                    <button
                      className="ai-plan-item-btn allow"
                      onClick={() => ctx.decideOperation(op, 'once')}
                      title="Allow this operation once"
                    >
                      {t('btn_allow')}
                    </button>
                    <button
                      className="ai-plan-item-btn deny"
                      onClick={() => ctx.decideOperation(op, 'deny')}
                      title="Deny this operation"
                    >
                      {t('btn_deny')}
                    </button>
                  </>
                )}
              </div>
            )}
            {isRead && (
              <span className="ai-plan-item-state auto">{t('op_auto')}</span>
            )}
          </div>
        );
      })}
    </div>
  );
}

function TraceCard({ data }: { data: TraceEntry[] }) {
  return (
    <div className="ai-trace-card">
      {data.map((entry, i) => (
        <div key={i} className={`ai-trace-item ${entry.result === 'applied' ? 'success' : 'fail'}`}>
          <span className="ai-trace-tool">{entry.tool}</span>
          <span className="ai-trace-result">{entry.result}</span>
        </div>
      ))}
    </div>
  );
}

function ErrorCard({ data }: { data: string }) {
  return (
    <div className="ai-error-card">
      <IconAlertCircle />
      <span>{data}</span>
    </div>
  );
}

function EntityListCard({ data }: { data: Array<{ id: string; name: string }> }) {
  return (
    <div className="ai-entity-list-card">
      {data.map((e) => (
        <div key={e.id} className="ai-entity-item">
          <span className="ai-entity-id">{e.id}</span>
          <span className="ai-entity-name">{e.name}</span>
        </div>
      ))}
    </div>
  );
}

// ─── Quick Actions ──────────────────────────────────────────────────────────

function QuickActions({ onAction }: { onAction: (action: string) => void }) {
  const { t } = useTranslation();
  return (
    <div className="ai-quick-actions">
      <span className="ai-quick-actions-label">{t('ai_workspace_label')}</span>
      <button className="ai-quick-btn" onClick={() => onAction('save')} title={t('command_save')} aria-label={t('command_save')}>
        <IconSave />
      </button>
      <button className="ai-quick-btn" onClick={() => onAction('undo')} title={t('command_undo')} aria-label={t('command_undo')}>
        <IconUndo />
      </button>
      <button className="ai-quick-btn" onClick={() => onAction('play')} title={t('command_play')} aria-label={t('command_play')}>
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
}: AiPanelProps) {
  const { t } = useTranslation();
  const [input, setInput] = useState('');
  const [messages, setMessages] = useState<AiMessage[]>(() => {
    try {
      const saved = localStorage.getItem('aster.aiMessages');
      return saved ? JSON.parse(saved) : [];
    } catch { return []; }
  });
  const [status, setStatus] = useState<AiStatus>('idle');
  const [plan, setPlan] = useState<CopilotPlan | null>(null);
  const [approved, setApproved] = useState<Set<number>>(new Set());
  const [denied, setDenied] = useState<Set<number>>(new Set());
  const [sessionWritesAllowed, setSessionWritesAllowed] = useState(false);
  const [entityDetails, setEntityDetails] = useState<EntityDetails | null>(null);
  const [mentionQuery, setMentionQuery] = useState<string | null>(null);
  const [mentionIndex, setMentionIndex] = useState(0);
  const [conversationTurns, setConversationTurns] = useState(0);
  const [workspaceView, setWorkspaceView] = useState<AiWorkspaceView>('chat');
  const [completedBundle, setCompletedBundle] = useState<CompletedChangeBundle | null>(null);
  const [pendingContinuation, setPendingContinuation] = useState(false);
  const [queuedPrompts, setQueuedPrompts] = useState<QueuedPrompt[]>([]);
  const [interruptRequested, setInterruptRequested] = useState(false);
  const [requestActive, setRequestActive] = useState(false);
  const [thinkingEffort, setThinkingEffort] = useState<ThinkingEffort>('medium');
  const continuationDepthRef = useRef(0);
  const activeRequestRef = useRef(false);
  const interruptRequestedRef = useRef(false);
  const queuedPromptsRef = useRef<QueuedPrompt[]>([]);
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
      localStorage.setItem('aster.aiMessages', JSON.stringify(messages));
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
    if (messages.length > 0 && !window.confirm(t('confirm_new_chat'))) return;
    try {
      await rpc('copilot/clear_conversation');
    } catch { /* ignore */ }
    setMessages([]);
    localStorage.removeItem('aster.aiMessages');
    setPlan(null);
    setApproved(new Set());
    setDenied(new Set());
    setStatus('idle');
    setConversationTurns(0);
    setCompletedBundle(null);
    updateQueuedPrompts([]);
    activeRequestRef.current = false;
    interruptRequestedRef.current = false;
    setInterruptRequested(false);
    setRequestActive(false);
    setWorkspaceView('chat');
    inputRef.current?.focus();
  }, [updateQueuedPrompts, messages.length, t]);

  // ── Submit ──

  const submitPrompt = useCallback(async (
    prompt: string,
    continuation = false,
    existingMessageId?: string,
  ) => {
    if (!prompt.trim() || activeRequestRef.current) return;

    if (!continuation) continuationDepthRef.current = 0;
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
      const planParams: Record<string, unknown> = { prompt };
      if (entityDetails) {
        planParams.selected_entity = entityDetails;
      }
      if (thinkingEffort !== 'off') {
        planParams.thinking_effort = thinkingEffort;
      }

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
            // Parse the tool call delta
            try {
              const tcDelta = JSON.parse(delta);
              const toolCalls = [...(message.activeToolCalls ?? [])];
              if (tcDelta.name) {
                // New tool call starting
                toolCalls.push({
                  id: tcDelta.id || `tc-${toolCalls.length}`,
                  name: tcDelta.name,
                  argumentsPreview: '',
                  complete: false,
                });
              }
              if (tcDelta.arguments_delta && toolCalls.length > 0) {
                // Accumulating arguments
                const last = toolCalls[toolCalls.length - 1];
                last.argumentsPreview += tcDelta.arguments_delta;
              }
              return { ...message, activeToolCalls: toolCalls };
            } catch {
              // Partial JSON fragments are expected during streaming; skip silently
              return message;
            }
          }
          return {
            ...message,
            content: message.content + delta,
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

      const autoApproved = new Set<number>();
      result.operations.forEach((op) => {
        if (op.permission_kind === 'read'
          || (op.permission_kind === 'write' && sessionWritesAllowed)
          || (op.permission_kind === 'command' && op.permanently_allowed)) {
          autoApproved.add(op.index);
        }
      });
      setApproved(autoApproved);
      setDenied(new Set());
      setStatus(result.operations.length > 0 ? 'ready' : 'complete');
      setConversationTurns(t => t + 1);

      const finalContent = result.message || (result.operations.length > 0
          ? `${t('ai_propose_ops').replace('{count}', String(result.operations.length))}`
          : t('ai_no_changes'));
      setMessages(prev => prev.map(message => message.id === streamingMessageId
        ? {
            ...message,
            content: message.content || finalContent,
            activeToolCalls: message.activeToolCalls?.map(tc => ({ ...tc, complete: true })),
            cards: result.operations.length > 0
              ? [{ type: 'plan', data: result }]
              : undefined,
          }
        : message));
      if (result.operations.length > 0) setWorkspaceView('changes');

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
  }, [entityDetails, addMessage, sessionWritesAllowed, thinkingEffort]);

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
    setPendingContinuation(false);
  }, [status, submitPrompt, updateQueuedPrompts]);

  const requestInterrupt = useCallback(() => {
    if (!activeRequestRef.current) return;
    interruptRequestedRef.current = true;
    setInterruptRequested(true);
    setPendingContinuation(false);
    cancelRef.current?.();
    cancelRef.current = null;
  }, []);

  useEffect(() => {
    if (!contextualRequest) return;
    queueOrSubmitPrompt(contextualRequest.prompt);
    onContextualRequestConsumed?.(contextualRequest.id);
  }, [contextualRequest, onContextualRequestConsumed, queueOrSubmitPrompt]);

  // ── Execute ──

  const executeApproved = useCallback(async (approvedSet?: Set<number>) => {
    const indices = Array.from(approvedSet ?? approved);
    if (indices.length === 0) return;

    setStatus('executing');
    setWorkspaceView('tasks');

    try {
      const result = await rpc<ApplyResult>('copilot/apply', {
        approved_indices: indices,
      });

      setStatus('complete');
      setPlan(null);

      const summary = result.summary || `${t('ai_applied_ops').replace('{count}', String(result.operations_performed))}`;
      setCompletedBundle({
        summary,
        operationsPerformed: result.operations_performed,
        traceEntries: result.trace_entries,
      });
      const cards: AiCard[] = [];
      if (result.trace_entries.length > 0) {
        cards.push({ type: 'trace', data: result.trace_entries });
      }
      addMessage('assistant', `✅ ${summary}`, cards.length > 0 ? cards : undefined);
      if (result.needs_continuation && continuationDepthRef.current < 4) {
        continuationDepthRef.current += 1;
        setPendingContinuation(true);
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
  }, [approved, addMessage, onSceneChanged]);

  const decideOperation = useCallback(async (
    operation: CopilotOperation,
    decision: 'once' | 'session' | 'always' | 'deny',
  ) => {
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
      setSessionWritesAllowed(true);
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

    if (decision === 'always' && operation.command) {
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
    setPendingContinuation(false);
    submitPrompt(
      'Continue the original task using the tool results now present in the conversation. Do not repeat completed inspection. Propose the concrete remaining operations, or explicitly state that the task is complete.',
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
    <div className="ai-panel">
      <ContextBar
        projectName={projectName}
        selectedEntity={selectedEntityName}
        sceneObjectCount={sceneObjectCount}
        onSettingsClick={onOpenSettings}
        onNewChat={handleNewChat}
        conversationTurns={conversationTurns}
      />

      {!chatOnly && <div className="ai-workspace-tabs" role="tablist" aria-label="AI workspace">
        {(['chat', 'tasks', 'changes'] as AiWorkspaceView[]).map(view => (
          <button
            key={view}
            className={workspaceView === view ? 'active' : ''}
            onClick={() => setWorkspaceView(view)}
            role="tab"
            aria-selected={workspaceView === view}
          >
            {view === 'chat' ? t('tab_chat') : view === 'tasks' ? t('tab_tasks') : t('tab_changes')}
            {view === 'changes' && plan && plan.operations.length > 0 && (
              <span>{plan.operations.length}</span>
            )}
          </button>
        ))}
      </div>}

      {/* Entity context card — shown when an entity is selected */}
      {entityDetails && <EntityContextCard entity={entityDetails} />}

      {/* Messages */}
      <PlanApprovalContext.Provider value={planApprovalCtx}>
      <div
        ref={scrollRef}
        className={`ai-messages ${!chatOnly && workspaceView !== 'chat' ? 'ai-workspace-detail' : ''}`}
        aria-live="polite"
      >
        {!chatOnly && workspaceView === 'tasks' && (
          <div className="ai-task-workspace">
            <div className={`ai-task-state state-${status}`}>
              <span className="ai-task-state-dot" />
              <div>
                <strong>{statusLabel[status]}</strong>
                <span>{projectName || t('ai_current_project')} · {sceneObjectCount} scene objects</span>
              </div>
            </div>
            <div className="ai-task-section">
              <div className="ai-task-section-title">{t('ai_current_workflow')}</div>
              <ol className="ai-task-timeline">
                <li className={status !== 'idle' ? 'complete' : 'active'}>{t('workflow_describe')}</li>
                <li className={status === 'thinking' ? 'active' : ['ready', 'executing', 'complete'].includes(status) ? 'complete' : ''}>{t('workflow_inspect')}</li>
                <li className={status === 'ready' ? 'active' : ['executing', 'complete'].includes(status) ? 'complete' : ''}>{t('workflow_review')}</li>
                <li className={status === 'executing' ? 'active' : status === 'complete' ? 'complete' : ''}>{t('workflow_apply')}</li>
              </ol>
            </div>
            <div className="ai-task-section">
              <div className="ai-task-section-title">{t('ai_context_scope')}</div>
              <div className="ai-scope-list">
                <span>{t('scope_scene_snapshot')} <strong>{sceneObjectCount} objects</strong></span>
                <span>Selection <strong>{selectedEntityName || t('scope_no_entity')}</strong></span>
                <span>Write policy <strong>{sessionWritesAllowed ? t('policy_session_allow') : t('policy_ask_write')}</strong></span>
              </div>
            </div>
          </div>
        )}
        {!chatOnly && workspaceView === 'changes' && (
          <div className="ai-changes-workspace">
            <div className="ai-task-section-title">{t('changes_bundle_title')}</div>
            {!plan || plan.operations.length === 0 ? (
              completedBundle ? (
                <div className="ai-completed-bundle">
                  <div className="ai-completed-heading">
                    <IconCheck />
                    <div>
                      <strong>{t('changes_applied')}</strong>
                      <span>{completedBundle.summary}</span>
                    </div>
                  </div>
                  <div className="ai-completed-stats">
                    <span><strong>{completedBundle.operationsPerformed}</strong> {t('label_operations')}</span>
                    <span><strong>{completedBundle.traceEntries.length}</strong> {t('label_trace_entries')}</span>
                  </div>
                  {completedBundle.traceEntries.length > 0 && <TraceCard data={completedBundle.traceEntries} />}
                </div>
              ) : (
                <div className="ai-workspace-empty">
                  <strong>{t('changes_empty')}</strong>
                  <span>{t('changes_empty_desc')}</span>
                </div>
              )
            ) : <>
              <div className="ai-change-summary">
                <span>{t('changes_decision_hint')}</span>
              </div>
              {plan.operations.map(operation => (
              <div key={operation.index} className="ai-change-row">
                <span className={`ai-change-kind ${operation.permission_kind}`}>
                  {operation.permission_kind.toUpperCase()}
                </span>
                <span className="ai-change-description" title={operation.preview}>{operation.preview}</span>
                <div className="ai-permission-actions">
                  {operation.permission_kind === 'read' ? (
                    <span className="ai-permission-state allowed">{t('op_allowed_auto')}</span>
                  ) : approved.has(operation.index) ? (
                    <span className="ai-permission-state allowed">
                      {operation.permission_kind === 'command' && operation.permanently_allowed ? t('op_always_allowed') : t('op_allowed')}
                    </span>
                  ) : denied.has(operation.index) ? (
                    <span className="ai-permission-state denied">{t('op_denied_once')}</span>
                  ) : operation.permission_kind === 'write' ? <>
                    <button onClick={() => decideOperation(operation, 'once')}>{t('btn_allow_once')}</button>
                    <button onClick={() => decideOperation(operation, 'session')}>{t('btn_allow_session')}</button>
                    <button onClick={() => decideOperation(operation, 'deny')}>{t('btn_deny_once')}</button>
                  </> : <>
                    <button onClick={() => decideOperation(operation, 'once')}>{t('btn_allow_once')}</button>
                    <button onClick={() => decideOperation(operation, 'always')}>{t('btn_allow_always')}</button>
                    <button onClick={() => decideOperation(operation, 'deny')}>{t('btn_deny_once')}</button>
                  </>}
                </div>
              </div>
              ))}
            </>}
          </div>
        )}
        {(chatOnly || workspaceView === 'chat') && <>
        {messages.length === 0 && (
          <div className="ai-empty">
            <div className="ai-empty-mark"><IconSparkles size={24} /></div>
            <span className="ai-empty-eyebrow">{t('ai_workspace_eyebrow')}</span>
            <p className="ai-empty-title">{t('ai_empty_title')}</p>
            <p className="ai-empty-description">
              {t('ai_empty_desc')}
            </p>
            <div className="ai-workflow" aria-label="AI editing workflow">
              <span className="active">{t('workflow_step_describe')}</span>
              <span>{t('workflow_step_review')}</span>
              <span>{t('workflow_step_apply')}</span>
              <span>{t('workflow_step_verify')}</span>
            </div>
            <div className="ai-empty-prompts">
              <button className="ai-prompt-chip" onClick={() => submitPrompt('Create a playable third-person character with a following camera and basic movement controls')}>
                <strong>{t('prompt_playable_char')}</strong>
                <span>{t('prompt_playable_char_desc')}</span>
              </button>
              <button className="ai-prompt-chip" onClick={() => submitPrompt('Improve the lighting and atmosphere of this scene while preserving the current composition')}>
                <strong>{t('prompt_improve_scene')}</strong>
                <span>{t('prompt_improve_scene_desc')}</span>
              </button>
              <button className="ai-prompt-chip" onClick={() => submitPrompt('Inspect the current project and recommend the highest-impact next improvement')}>
                <strong>{t('prompt_inspect')}</strong>
                <span>{t('prompt_inspect_desc')}</span>
              </button>
            </div>
            <button className="ai-empty-configure" onClick={onOpenSettings}>
              <IconSettings size={12} />
              <span>{t('ai_model_settings')}</span>
              <IconChevronRight size={12} />
            </button>
          </div>
        )}
        {messages.map((msg) => (
          <MessageBubble key={msg.id} msg={msg} />
        ))}

        {/* Executing indicator */}
        {status === 'executing' && (
          <div className="ai-executing">
            <IconLoader className="spin-icon" />
            <span>{t('status_executing')}</span>
          </div>
        )}
        {status === 'thinking' && (
          <div className="ai-executing">
            <IconLoader className="spin-icon" />
            <span>{t('status_thinking')}</span>
          </div>
        )}
        {status === 'error' && lastPromptRef.current && (
          <div className="ai-retry-bar">
            <button
              className="btn btn-secondary btn-sm"
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
        const pendingOps = plan!.operations.filter(
          op => !approved.has(op.index) && !denied.has(op.index) && op.permission_kind !== 'read'
        );
        const approveAll = () => {
          const count = plan!.operations.filter(op => op.permission_kind !== 'read').length;
          if (!window.confirm(t('confirm_approve_all').replace('{count}', String(count)))) return;
          setApproved(current => {
            const next = new Set(current);
            plan!.operations
              .filter(op => op.permission_kind !== 'read')
              .forEach(op => next.add(op.index));
            return next;
          });
          setDenied(current => {
            const next = new Set(current);
            plan!.operations
              .filter(op => op.permission_kind !== 'read')
              .forEach(op => next.delete(op.index));
            return next;
          });
        };
        return (
          <div className="ai-plan-bar">
            <button
              className="btn btn-primary btn-sm"
              onClick={() => executeApproved()}
              disabled={approved.size === 0}
              title={approved.size === 0 ? 'Approve at least one operation below, or click \"Approve all\" to continue' : undefined}
            >
              {t('btn_continue_allowed').replace('{count}', String(approved.size))}
            </button>
            {pendingOps.length > 0 && (
              <button
                className="btn btn-secondary btn-sm"
                onClick={approveAll}
                title={`Approve all ${pendingOps.length} pending write/command operation${pendingOps.length === 1 ? '' : 's'}`}
              >
                {t('btn_approve_all').replace('{count}', String(pendingOps.length))}
              </button>
            )}
            <button
              className="btn btn-ghost btn-sm"
              onClick={discardProposal}
            >
              {t('btn_discard')}
            </button>
            {approvedWriteCount > 0 && <span className="ai-write-warning">{approvedWriteCount} {approvedWriteCount === 1 ? t('label_write') : t('label_writes')}</span>}
          </div>
        );
      })()}

      {/* Quick Actions + Input */}
      <div className="ai-input-area">
        <QuickActions onAction={onQuickAction} />

        {(requestActive || status === 'executing' || queuedPrompts.length > 0) && (
          <div className="ai-queue-status" role="status">
            <div>
              <IconLoader className={requestActive || status === 'executing' ? 'spin-icon' : undefined} />
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
            {requestActive && status !== 'executing' && (
              <button onClick={requestInterrupt} disabled={interruptRequested}>
                {interruptRequested ? t('btn_stopping') : t('btn_stop_response')}
              </button>
            )}
          </div>
        )}

        {/* @ Mention autocomplete dropdown */}
        {mentionQuery !== null && mentionMatches.length > 0 && (
          <div className="ai-mention-dropdown" role="listbox" aria-label={t('mention_suggestions')}>
            {mentionMatches.map((obj, i) => (
              <button
                key={obj.id}
                role="option"
                aria-selected={i === mentionIndex}
                className={`ai-mention-item ${i === mentionIndex ? 'active' : ''}`}
                onMouseDown={(e) => { e.preventDefault(); insertMention(obj); }}
              >
                <span className="ai-mention-icon">⬡</span>
                <span className="ai-mention-name">{obj.name}</span>
              </button>
            ))}
          </div>
        )}

        <div className="ai-input-heading">
          <span>{requestActive || status === 'executing' ? t('input_queue_next') : t('input_describe')}</span>
          <span>{requestActive || status === 'executing' ? t('input_queue_hint') : t('input_send_hint')}</span>
        </div>
        <div className="ai-input-row">
          <textarea
            ref={inputRef as React.RefObject<HTMLTextAreaElement>}
            className="ai-input"
            placeholder={t('ai_input_placeholder')}
            value={input}
            onChange={handleInputChange}
            onKeyDown={handleKeyDown}
            rows={2}
          />
          <button
            className="ai-send-btn"
            onClick={() => queueOrSubmitPrompt(input)}
            disabled={!input.trim()}
            aria-label={t('btn_send')}
          >
            <IconSend />
          </button>
        </div>
        <div className="ai-input-controls">
          <ModelSelector />
          <div className="ai-thinking-selector">
            <IconBrain size={12} />
            <select
              className="ai-thinking-select"
              value={thinkingEffort}
              onChange={(e) => setThinkingEffort(e.target.value as ThinkingEffort)}
              title={t('thinking_effort_title')}
            >
              <option value="off">{t('thinking_off')}</option>
              <option value="low">{t('thinking_low')}</option>
              <option value="medium">{t('thinking_medium')}</option>
              <option value="high">{t('thinking_high')}</option>
            </select>
          </div>
        </div>
      </div>
    </div>
  );
}
