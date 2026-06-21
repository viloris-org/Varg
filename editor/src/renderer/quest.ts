import { rpc } from './api';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type QuestStatus =
  | 'draft'
  | 'clarifying'
  | 'specified'
  | 'planning'
  | 'prepared'
  | 'running'
  | 'waiting_for_user'
  | 'validating'
  | 'repairing'
  | 'ready_for_review'
  | 'applying'
  | 'completed'
  | 'blocked'
  | 'canceled'
  | 'archived';

export type QuestMode = 'solo' | 'extra';

export type QuestProvider =
  | 'inherit'
  | 'stub'
  | 'anthropic'
  | 'openai'
  | 'codex_oauth'
  | 'gemini'
  | 'ollama'
  | 'custom'
  | 'mimo'
  | 'deepseek'
  | 'glm';

export type QuestThinkingEffort = 'off' | 'low' | 'medium' | 'high';

export interface QuestModelConfig {
  provider: QuestProvider | string;
  model: string;
  api_endpoint?: string | null;
  max_tokens: number;
  thinking_effort: QuestThinkingEffort | string;
}

export interface QuestAutonomyPolicy {
  workspace_writes_automatic: boolean;
  active_project_apply_requires_approval: boolean;
  allowlisted_commands_automatic: boolean;
  high_risk_requires_confirmation: boolean;
}

export interface QuestNextAction {
  label: string;
  reason: string;
}

export interface QuestRecord {
  schema_version: number;
  id: string;
  title: string;
  goal: string;
  status: QuestStatus;
  project: {
    name: string;
    path: string;
  };
  mode: QuestMode;
  model_config: QuestModelConfig;
  autonomy: QuestAutonomyPolicy;
  created_at_ms: number;
  updated_at_ms: number;
  workspace_id: string | null;
  intent_path: string;
  spec_path: string | null;
  trace_path: string;
  artifact_links: Array<{
    kind: string;
    label: string;
    path: string;
  }>;
  attached_knowledge_ids: string[];
  branch_of: string | null;
  branch_ids: string[];
  decisions: Array<{
    kind: string;
    summary: string;
    files: string[];
    timestamp_ms: number;
    rollback_id: string | null;
  }>;
  checkpoints: Array<{
    id: string;
    label: string;
    summary: string;
    timestamp_ms: number;
    workspace_id?: string | null;
    artifact_path?: string | null;
    project_fingerprint?: string | null;
  }>;
  next_action: QuestNextAction;
  review: QuestReview | null;
}

export interface QuestEvent {
  id: string;
  quest_id: string;
  timestamp_ms: number;
  kind: string;
  summary: string;
  details: unknown;
}

export interface QuestReview {
  summary: string;
  changed_files: Array<{
    path: string;
    additions: number;
    deletions: number;
    status: string;
    diff: string;
  }>;
  transaction_groups: Array<{
    id: string;
    label: string;
    summary: string;
    files: string[];
    risk: string;
  }>;
  exploration_attempts: Array<{
    id: string;
    label: string;
    summary: string;
    outcome: string;
    artifact_path: string;
    selected: boolean;
  }>;
  findings: Array<{
    id: string;
    title: string;
    summary: string;
    severity: string;
    artifact_path?: string | null;
    source?: string | null;
  }>;
  validations: Array<{
    name: string;
    status: string;
    summary: string;
    command_id?: string | null;
    command?: string | null;
    policy_approved?: boolean;
    log?: string;
  }>;
  unresolved_issues: string[];
  next_actions: QuestReviewAction[];
  metrics?: {
    intent_to_first_action_ms?: number | null;
    tool_call_latency_ms?: number | null;
    validator_turnaround_ms?: number | null;
    context_relevance_score?: number | null;
    failed_action_recovery_rate?: number | null;
    review_evidence_quality_score?: number | null;
    isolated_attempt_count?: number;
    validation_count?: number;
    validation_failure_count?: number;
    baseline_changed_file_count?: number;
    notes?: string[];
  };
  risk: string;
}

export interface QuestReviewAction {
  id: string;
  label: string;
  kind: string;
  target?: string | null;
}

export interface QuestDetail extends QuestRecord {
  intent: string;
  spec: string;
  attached_knowledge: KnowledgeEntry[];
  events: QuestEvent[];
}

export interface KnowledgeEntry {
  id: string;
  status: 'pending' | 'approved' | 'rejected';
  category: string;
  content: string;
  source: string;
  reference_status: 'unchecked' | 'unverified' | 'valid' | 'missing' | 'invalid' | string;
  reference_summary: string;
  created_at_ms: number;
  updated_at_ms: number;
}

export function listQuests(): Promise<{ quests: QuestRecord[] }> {
  return rpc('quest/list');
}

export function getQuest(id: string): Promise<QuestDetail> {
  return rpc('quest/get', { id });
}

interface QuestAiStreamEvent {
  request_id: string;
  kind: 'text' | 'thinking' | 'tool_call';
  delta: string;
}

export interface QuestAiStreamHandle<T> {
  promise: Promise<T>;
  cancel: () => void;
}

let nextQuestAiRequestId = 1;

function streamQuestAiRequest<T>(
  kind: 'create' | 'rewrite',
  params: Record<string, unknown>,
  onDelta?: (delta: string, kind: QuestAiStreamEvent['kind']) => void,
): QuestAiStreamHandle<T> {
  const requestId = `${Date.now()}-${nextQuestAiRequestId++}`;
  let rejectCancelled: ((reason?: unknown) => void) | undefined;
  const cancelled = new Promise<never>((_, reject) => {
    rejectCancelled = reject;
  });
  let resolveComplete: (() => void) | undefined;
  const completed = new Promise<void>((resolve) => {
    resolveComplete = resolve;
  });

  const setupPromise = (async () => {
    const unlistenDelta = await listen<QuestAiStreamEvent>('quest-ai-stream', event => {
      if (event.payload.request_id === requestId) {
        onDelta?.(event.payload.delta, event.payload.kind ?? 'text');
      }
    });
    const unlistenComplete = await listen<{ request_id: string }>('quest-ai-stream-complete', event => {
      if (event.payload.request_id === requestId) {
        resolveComplete?.();
      }
    });

    try {
      await invoke('start_quest_ai_request', { requestId, kind, params });
      await completed;
      return await invoke<T>('finish_quest_ai_request', { requestId });
    } finally {
      unlistenDelta();
      unlistenComplete();
    }
  })();

  return {
    promise: Promise.race([setupPromise, cancelled]),
    cancel: () => {
      rejectCancelled?.(new Error('cancelled'));
      rejectCancelled = undefined;
      invoke('cancel_quest_ai_request', { requestId }).catch(() => {});
    },
  };
}

export function createQuest(
  title: string,
  goal: string,
  options?: { mode?: QuestMode; model_config?: QuestModelConfig },
): Promise<QuestDetail> {
  return streamQuestAiRequest<QuestDetail>('create', { title, goal, ...options }).promise;
}

export interface OpenAIRealtimeTranscriptionSession {
  session: {
    client_secret?: {
      value?: string;
    };
    [key: string]: unknown;
  };
  model: 'gpt-realtime-whisper' | string;
  endpoint: string;
  realtime_url: string;
}

export function createOpenAIRealtimeTranscriptionSession(): Promise<OpenAIRealtimeTranscriptionSession> {
  return invoke('create_openai_realtime_transcription_session');
}

export function rewriteQuestPrompt(
  prompt: string,
  modelConfig?: QuestModelConfig,
  onDelta?: (delta: string, kind: QuestAiStreamEvent['kind']) => void,
): QuestAiStreamHandle<{ prompt: string }> {
  return streamQuestAiRequest('rewrite', { prompt, model_config: modelConfig }, onDelta);
}

export function promoteQuest(prompt: string, context: string): Promise<QuestDetail> {
  return rpc('quest/promote', { prompt, context });
}

export function updateQuestSpec(id: string, spec: string): Promise<QuestDetail> {
  return rpc('quest/update_spec', { id, spec });
}

export function updateQuestIntent(id: string, intent: string): Promise<QuestDetail> {
  return rpc('quest/update_intent', { id, intent });
}

export function updateQuestExecutionConfig(
  id: string,
  mode: QuestMode,
  modelConfig: QuestModelConfig,
  autonomy?: QuestAutonomyPolicy,
): Promise<QuestDetail> {
  return rpc('quest/update_execution_config', { id, mode, model_config: modelConfig, autonomy });
}

export function updateQuestKnowledgeContext(id: string, knowledgeIds: string[]): Promise<QuestDetail> {
  return rpc('quest/update_knowledge_context', { id, knowledge_ids: knowledgeIds });
}

export function addQuestNote(id: string, kind: string, message: string): Promise<QuestDetail> {
  return rpc('quest/add_note', { id, kind, message });
}

export function requestQuestQuickFix(id: string, issue: string): Promise<QuestDetail> {
  return rpc('quest/request_quick_fix', { id, issue });
}

export function renameQuest(id: string, title: string): Promise<QuestDetail> {
  return rpc('quest/rename', { id, title });
}

export function branchQuest(id: string, title?: string): Promise<QuestDetail> {
  return rpc('quest/branch', title ? { id, title } : { id });
}

export function transitionQuest(id: string, status: QuestStatus): Promise<QuestDetail> {
  return rpc('quest/transition', { id, status });
}

export function deleteQuest(id: string): Promise<{ deleted: boolean }> {
  return rpc('quest/delete', { id });
}

export function executeQuest(id: string): Promise<QuestDetail> {
  return rpc('quest/execute', { id });
}

export function applyQuest(id: string, files?: string[]): Promise<QuestDetail> {
  return rpc('quest/apply', files ? { id, files } : { id });
}

export function applyQuestTransactionGroups(id: string, transactionGroupIds: string[]): Promise<QuestDetail> {
  return rpc('quest/apply', { id, transaction_group_ids: transactionGroupIds });
}

export function discardQuest(id: string, files?: string[]): Promise<QuestDetail> {
  return rpc('quest/discard', files ? { id, files } : { id });
}

export function discardQuestTransactionGroups(id: string, transactionGroupIds: string[]): Promise<QuestDetail> {
  return rpc('quest/discard', { id, transaction_group_ids: transactionGroupIds });
}

export function rollbackQuest(id: string, rollbackId: string): Promise<QuestDetail> {
  return rpc('quest/rollback', { id, rollback_id: rollbackId });
}

export function exportQuest(id: string): Promise<QuestDetail> {
  return rpc('quest/export', { id });
}

export function cancelQuest(id: string, reason?: string): Promise<QuestDetail> {
  return rpc('quest/cancel', { id, reason });
}

export function reopenQuest(id: string, reason?: string): Promise<QuestDetail> {
  return rpc('quest/reopen', { id, reason });
}

export function continueQuest(id: string, reason?: string): Promise<QuestDetail> {
  return rpc('quest/continue', { id, reason });
}

export function rejectQuest(id: string, reason?: string): Promise<QuestDetail> {
  return rpc('quest/reject', { id, reason });
}

export function requestQuestRevision(id: string, reason?: string): Promise<QuestDetail> {
  return rpc('quest/request_revision', { id, reason });
}

export function mockExecuteQuest(id: string): Promise<QuestDetail> {
  return rpc('quest/mock_execute', { id });
}

export function listKnowledge(): Promise<{ entries: KnowledgeEntry[] }> {
  return rpc('knowledge/list');
}

export function proposeKnowledge(category: string, content: string, source: string): Promise<{ entries: KnowledgeEntry[] }> {
  return rpc('knowledge/propose', { category, content, source });
}

export function approveKnowledge(id: string): Promise<{ entries: KnowledgeEntry[] }> {
  return rpc('knowledge/approve', { id });
}

export function rejectKnowledge(id: string): Promise<{ entries: KnowledgeEntry[] }> {
  return rpc('knowledge/reject', { id });
}

export function revalidateKnowledge(): Promise<{ entries: KnowledgeEntry[] }> {
  return rpc('knowledge/revalidate');
}

export function removeKnowledge(id: string): Promise<{ entries: KnowledgeEntry[] }> {
  return rpc('knowledge/remove', { id });
}
