import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
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
        title={discoveryError ?? 'Available models from the configured provider'}
        onChange={(e) => {
          const val = e.target.value;
          if (val === '__refresh__') { loadModels(); return; }
          handleChange(val === '__custom__' ? currentModel : val);
        }}
      >
        {models.length === 0 && !currentModel && <option value="">No models</option>}
        {models.map(m => (
          <option key={m.id} value={m.id}>{m.display_name}</option>
        ))}
        {!known && currentModel && <option value="__custom__">{currentModel}</option>}
        {discoveryError && <option value="__discovery_error__" disabled>Provider discovery failed; showing fallback models</option>}
        <option value="__refresh__">↻ Refresh list</option>
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
  return (
    <div className="ai-context-bar">
      {projectName && <span className="ai-context-tag">{projectName}</span>}
      <span className="ai-context-tag">{sceneObjectCount} objects</span>
      {selectedEntity && (
        <span className="ai-context-tag ai-context-selected">@ {selectedEntity}</span>
      )}
      {conversationTurns > 0 && (
        <span className="ai-context-tag ai-context-turns">{conversationTurns} turn{conversationTurns !== 1 ? 's' : ''}</span>
      )}
      <div className="ai-context-actions">
        <button className="ai-context-settings-btn" onClick={onNewChat} title="New Chat">
          <IconRefresh />
        </button>
        <button className="ai-context-settings-btn" onClick={onSettingsClick} title="AI Settings">
          <IconSettings />
        </button>
      </div>
    </div>
  );
}

// ─── Entity Context Card ───────────────────────────────────────────────────

function EntityContextCard({ entity }: { entity: EntityDetails }) {
  const [expanded, setExpanded] = useState(false);
  const pos = entity.transform.position;
  const comps = entity.components;

  return (
    <div className="ai-entity-context-card">
      <button className="ai-entity-context-header" onClick={() => setExpanded(!expanded)}>
        {expanded ? <IconChevronDown /> : <IconChevronRight />}
        <span className="ai-entity-context-name">{entity.name}</span>
        <span className="ai-entity-context-tag">{entity.tag || 'Untagged'}</span>
        <span className="ai-entity-context-components">{comps.length} comp{comps.length !== 1 ? 's' : ''}</span>
      </button>
      {expanded && (
        <div className="ai-entity-context-body">
          <div className="ai-entity-context-row">
            <span className="ai-entity-context-label">Position</span>
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

function MessageBubble({ msg }: { msg: AiMessage }) {
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
              <span>Thinking process</span>
            </button>
            {thinkingExpanded && (
              <div className="ai-thinking-content">{msg.thinking}</div>
            )}
          </div>
        )}
        <div className="ai-message-content">{msg.content}</div>
        {msg.queued && <div className="ai-message-state queued">Queued for the next turn</div>}
        {msg.interrupted && <div className="ai-message-state interrupted">Interrupted</div>}
        {msg.cards && msg.cards.map((card, i) => (
          <InlineCard key={i} card={card} />
        ))}
      </div>
    </div>
  );
}

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
  return (
    <div className="ai-plan-card">
      {data.operations.map((op) => (
        <div key={op.index} className="ai-plan-item">
          <span className={`ai-plan-badge ${op.requires_write ? 'write' : 'read'}`}>
            {op.requires_write ? 'W' : 'R'}
          </span>
          <span>{op.preview}</span>
        </div>
      ))}
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
      <span className="ai-quick-actions-label">Workspace</span>
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

let nextMsgId = 1;

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
  const [messages, setMessages] = useState<AiMessage[]>([]);
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

  // Auto-scroll
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages]);

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
      id: String(nextMsgId++),
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
  }, [updateQueuedPrompts]);

  // ── Submit ──

  const submitPrompt = useCallback(async (
    prompt: string,
    continuation = false,
    existingMessageId?: string,
  ) => {
    if (!prompt.trim() || activeRequestRef.current) return;

    if (!continuation) continuationDepthRef.current = 0;

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
    const streamingMessageId = String(nextMsgId++);
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

      const result = await streamCopilotPlan<CopilotPlan>(planParams, (delta, kind) => {
        if (interruptRequestedRef.current) return;
        setMessages(prev => prev.map(message => {
          if (message.id !== streamingMessageId) return message;
          if (kind === 'thinking') {
            return {
              ...message,
              thinking: (message.thinking ?? '') + delta,
            };
          }
          return {
            ...message,
            content: message.content + delta,
          };
        }));
      });
      if (interruptRequestedRef.current) {
        setMessages(prev => prev.map(message => message.id === streamingMessageId
          ? { ...message, content: message.content || 'Response interrupted.', interrupted: true }
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
          ? `I propose ${result.operations.length} operation(s).`
          : 'No project changes are needed.');
      setMessages(prev => prev.map(message => message.id === streamingMessageId
        ? {
            ...message,
            content: finalContent,
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
          ? { ...message, content: message.content || 'Response interrupted.', interrupted: true }
          : message));
        setStatus('idle');
        return;
      }
      const msg = typeof err === 'string' ? err : err.message || 'Unknown error';
      setStatus('error');
      setMessages(prev => prev.map(message => message.id === streamingMessageId
        ? { ...message, content: 'Something went wrong.', cards: [{ type: 'error', data: msg }] }
        : message));
    } finally {
      activeRequestRef.current = false;
      setRequestActive(false);
      interruptRequestedRef.current = false;
      setInterruptRequested(false);
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

    const id = String(nextMsgId++);
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

      const summary = result.summary || `Applied ${result.operations_performed} operation(s).`;
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
        addMessage('system', 'Inspection complete. Continuing with the build plan…');
      } else {
        setWorkspaceView('changes');
      }

      // Immediately refresh viewport and scene tree
      onSceneChanged?.();
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err.message || 'Unknown error';
      setStatus('error');
      addMessage('assistant', '❌ Execution failed.', [{ type: 'error', data: msg }]);
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

    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      queueOrSubmitPrompt(input);
    }
  }, [input, queueOrSubmitPrompt, mentionQuery, mentionMatches, mentionIndex, insertMention]);

  // ── Render ──

  const hasPlan = plan && plan.operations.length > 0 && status === 'ready';
  const approvedWriteCount = plan?.operations.filter(operation => (
    operation.requires_write && approved.has(operation.index)
  )).length ?? 0;
  const statusLabel: Record<AiStatus, string> = {
    idle: 'Ready for an outcome',
    thinking: 'Planning the task',
    ready: 'Waiting for review',
    executing: 'Applying approved work',
    complete: 'Task complete',
    error: 'Action required',
  };

  const discardProposal = useCallback(() => {
    setPlan(null);
    setApproved(new Set());
    setDenied(new Set());
    setStatus('idle');
    setWorkspaceView('chat');
  }, []);

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
            {view === 'chat' ? 'Chat' : view === 'tasks' ? 'Tasks' : 'Changes'}
            {view === 'changes' && plan && plan.operations.length > 0 && (
              <span>{plan.operations.length}</span>
            )}
          </button>
        ))}
      </div>}

      {/* Entity context card — shown when an entity is selected */}
      {entityDetails && <EntityContextCard entity={entityDetails} />}

      {/* Messages */}
      <div
        ref={scrollRef}
        className={`ai-messages ${!chatOnly && workspaceView !== 'chat' ? 'ai-workspace-detail' : ''}`}
        aria-live={status === 'thinking' || status === 'executing' ? 'polite' : undefined}
      >
        {!chatOnly && workspaceView === 'tasks' && (
          <div className="ai-task-workspace">
            <div className={`ai-task-state state-${status}`}>
              <span className="ai-task-state-dot" />
              <div>
                <strong>{statusLabel[status]}</strong>
                <span>{projectName || 'Current project'} · {sceneObjectCount} scene objects</span>
              </div>
            </div>
            <div className="ai-task-section">
              <div className="ai-task-section-title">Current workflow</div>
              <ol className="ai-task-timeline">
                <li className={status !== 'idle' ? 'complete' : 'active'}>Describe the outcome</li>
                <li className={status === 'thinking' ? 'active' : ['ready', 'executing', 'complete'].includes(status) ? 'complete' : ''}>Build and inspect the plan</li>
                <li className={status === 'ready' ? 'active' : ['executing', 'complete'].includes(status) ? 'complete' : ''}>Review proposed changes</li>
                <li className={status === 'executing' ? 'active' : status === 'complete' ? 'complete' : ''}>Apply and verify</li>
              </ol>
            </div>
            <div className="ai-task-section">
              <div className="ai-task-section-title">Context scope</div>
              <div className="ai-scope-list">
                <span>Scene snapshot <strong>{sceneObjectCount} objects</strong></span>
                <span>Selection <strong>{selectedEntityName || 'No entity pinned'}</strong></span>
                <span>Write policy <strong>{sessionWritesAllowed ? 'Allowed for this session' : 'Ask before writing'}</strong></span>
              </div>
            </div>
          </div>
        )}
        {!chatOnly && workspaceView === 'changes' && (
          <div className="ai-changes-workspace">
            <div className="ai-task-section-title">Proposed change bundle</div>
            {!plan || plan.operations.length === 0 ? (
              completedBundle ? (
                <div className="ai-completed-bundle">
                  <div className="ai-completed-heading">
                    <IconCheck />
                    <div>
                      <strong>Applied and verified</strong>
                      <span>{completedBundle.summary}</span>
                    </div>
                  </div>
                  <div className="ai-completed-stats">
                    <span><strong>{completedBundle.operationsPerformed}</strong> operations</span>
                    <span><strong>{completedBundle.traceEntries.length}</strong> trace entries</span>
                  </div>
                  {completedBundle.traceEntries.length > 0 && <TraceCard data={completedBundle.traceEntries} />}
                </div>
              ) : (
                <div className="ai-workspace-empty">
                  <strong>No pending changes</strong>
                  <span>Ask Aster to build or modify something. Proposed operations will appear here before they touch the project.</span>
                </div>
              )
            ) : <>
              <div className="ai-change-summary">
                <span>Reads run automatically. Writes and commands require a decision.</span>
              </div>
              {plan.operations.map(operation => (
              <div key={operation.index} className="ai-change-row">
                <span className={`ai-change-kind ${operation.permission_kind}`}>
                  {operation.permission_kind.toUpperCase()}
                </span>
                <span className="ai-change-description">{operation.preview}</span>
                <div className="ai-permission-actions">
                  {operation.permission_kind === 'read' ? (
                    <span className="ai-permission-state allowed">Allowed automatically</span>
                  ) : approved.has(operation.index) ? (
                    <span className="ai-permission-state allowed">
                      {operation.permission_kind === 'command' && operation.permanently_allowed ? 'Always allowed' : 'Allowed'}
                    </span>
                  ) : denied.has(operation.index) ? (
                    <span className="ai-permission-state denied">Denied this time</span>
                  ) : operation.permission_kind === 'write' ? <>
                    <button onClick={() => decideOperation(operation, 'once')}>Allow once</button>
                    <button onClick={() => decideOperation(operation, 'session')}>Allow for session</button>
                    <button onClick={() => decideOperation(operation, 'deny')}>Deny once</button>
                  </> : <>
                    <button onClick={() => decideOperation(operation, 'once')}>Allow once</button>
                    <button onClick={() => decideOperation(operation, 'always')}>Always allow command</button>
                    <button onClick={() => decideOperation(operation, 'deny')}>Deny once</button>
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
            <span className="ai-empty-eyebrow">AI workspace</span>
            <p className="ai-empty-title">What should Aster build?</p>
            <p className="ai-empty-description">
              Describe an outcome. Aster will inspect the scene, propose a plan, and ask before applying changes.
            </p>
            <div className="ai-workflow" aria-label="AI editing workflow">
              <span className="active">Describe</span>
              <span>Review plan</span>
              <span>Apply</span>
              <span>Verify</span>
            </div>
            <div className="ai-empty-prompts">
              <button className="ai-prompt-chip" onClick={() => submitPrompt('Create a playable third-person character with a following camera and basic movement controls')}>
                <strong>Playable character</strong>
                <span>Create movement, camera, and input setup</span>
              </button>
              <button className="ai-prompt-chip" onClick={() => submitPrompt('Improve the lighting and atmosphere of this scene while preserving the current composition')}>
                <strong>Improve the scene</strong>
                <span>Refine lighting and atmosphere</span>
              </button>
              <button className="ai-prompt-chip" onClick={() => submitPrompt('Inspect the current project and recommend the highest-impact next improvement')}>
                <strong>Inspect project</strong>
                <span>Find the best next improvement</span>
              </button>
            </div>
            <button className="ai-empty-configure" onClick={onOpenSettings}>
              <IconSettings size={12} />
              <span>Model and provider settings</span>
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
            <span>Executing...</span>
          </div>
        )}
        {status === 'thinking' && (
          <div className="ai-executing">
            <IconLoader className="spin-icon" />
            <span>Thinking...</span>
          </div>
        )}
        </>}
      </div>

      {/* Plan approval bar */}
      {hasPlan && (
        <div className="ai-plan-bar">
          <button
            className="btn btn-primary btn-sm"
            onClick={() => executeApproved()}
            disabled={approved.size === 0}
          >
            Continue with allowed ({approved.size})
          </button>
          <button
            className="btn btn-ghost btn-sm"
            onClick={discardProposal}
          >
            Discard proposal
          </button>
          {approvedWriteCount > 0 && <span className="ai-write-warning">{approvedWriteCount} write operation{approvedWriteCount === 1 ? '' : 's'}</span>}
        </div>
      )}

      {/* Quick Actions + Input */}
      <div className="ai-input-area">
        <QuickActions onAction={onQuickAction} />

        {(requestActive || status === 'executing' || queuedPrompts.length > 0) && (
          <div className="ai-queue-status" role="status">
            <div>
              <IconLoader className={requestActive || status === 'executing' ? 'spin-icon' : undefined} />
              <span>
                {interruptRequested
                  ? 'Stopping after the current provider response…'
                  : queuedPrompts.length > 0
                    ? `${queuedPrompts.length} message${queuedPrompts.length === 1 ? '' : 's'} queued for the next turn`
                    : status === 'executing'
                      ? 'Applying changes. New messages will run next.'
                      : 'Aster is responding. You can queue another message or stop this response.'}
              </span>
            </div>
            {requestActive && status !== 'executing' && (
              <button onClick={requestInterrupt} disabled={interruptRequested}>
                {interruptRequested ? 'Stopping…' : 'Stop response'}
              </button>
            )}
          </div>
        )}

        {/* @ Mention autocomplete dropdown */}
        {mentionQuery !== null && mentionMatches.length > 0 && (
          <div className="ai-mention-dropdown">
            {mentionMatches.map((obj, i) => (
              <button
                key={obj.id}
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
          <span>{requestActive || status === 'executing' ? 'Queue the next instruction' : 'Describe the outcome'}</span>
          <span>{requestActive || status === 'executing' ? 'It will run after the current step' : 'Enter to send · Shift+Enter for a new line'}</span>
        </div>
        <div className="ai-input-row">
          <textarea
            ref={inputRef as React.RefObject<HTMLTextAreaElement>}
            className="ai-input"
            placeholder="Build, change, inspect, or fix something... Use @ to reference an entity."
            value={input}
            onChange={handleInputChange}
            onKeyDown={handleKeyDown}
            rows={2}
          />
          <button
            className="ai-send-btn"
            onClick={() => queueOrSubmitPrompt(input)}
            disabled={!input.trim()}
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
              title="Thinking/reasoning effort level"
            >
              <option value="off">No thinking</option>
              <option value="low">Light thinking</option>
              <option value="medium">Balanced</option>
              <option value="high">Deep thinking</option>
            </select>
          </div>
        </div>
      </div>
    </div>
  );
}
