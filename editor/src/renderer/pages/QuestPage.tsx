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
    <div className="quest-shell">
      <header className="quest-global-header">
        <div className="quest-brand">
          <span>Aster</span>
          <strong>Quests</strong>
        </div>
        <nav>
          <button className="active">Quests</button>
        </nav>
        <div className="quest-global-actions">
          <button
            onClick={() => selected && onOpenEditor(
              selected.project.path,
              artifactFor('intent', 'Quest intent', selected.intent_path),
            )}
            disabled={!selected}
          >
            <IconCode /> Open Editor
          </button>
          <button onClick={onCloseProject} title="Close project"><IconX /></button>
        </div>
      </header>

      <div className="quest-layout">
        <aside className="quest-sidebar">
          <div className="quest-sidebar-heading">
            <button className="quest-new-button" onClick={() => setSelected(null)} disabled={!currentProjectPath}>
              <IconPlus /> New Quest <kbd>Ctrl N</kbd>
            </button>
          </div>

          <QuestGroup label="Quests" quests={visibleQuests.active} selectedId={selected?.id} onSelect={selectQuest} />
          <QuestGroup label="Completed" quests={visibleQuests.history} selectedId={selected?.id} onSelect={selectQuest} />
          <div className="quest-sidebar-footer">
            <button onClick={() => setPanel('knowledge')}>Knowledge <b>{knowledge.filter(entry => entry.status === 'pending').length}</b></button>
            <button disabled>Marketplace</button>
          </div>
        </aside>

        {!selected ? (
          <main className="quest-home">
            <div className="quest-orb"><IconSparkles size={28} /></div>
            <h1>Quest on, hands off</h1>
            <div className="quest-start-line">
              <span>Start in</span>
              <b>{currentProjectPath ? 'Aster' : 'No project'}</b>
              <span>Local</span>
              <span>main</span>
            </div>
            <div className="quest-prompt-box">
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
                <button onClick={create} disabled={busy || !goal.trim() || !currentProjectPath} title="Create Quest">
                  {busy ? <IconLoader className="spin-icon" /> : <IconSend />}
                </button>
              </footer>
            </div>
            <div className="quest-intro-card">
              <IconSparkles />
              <div>
                <strong>Meet Quest Mode</strong>
                <p>Describe the outcome. Aster drafts a named spec, tracks execution, and keeps review separate from active project changes.</p>
              </div>
            </div>
          </main>
        ) : (
          <main className="quest-workspace">
            <header className="quest-header">
              <div>
                <div className="quest-project-line">
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
                  <div className="quest-title-edit">
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
              <div className="quest-header-actions">
                <span className={`quest-status status-${selected.status}`}>{selected.status}</span>
                {!renaming && <button onClick={() => setRenaming(true)} disabled={busy}><IconEdit /> Rename</button>}
                <button onClick={() => onOpenEditor(
                  selected.project.path,
                  artifactFor('intent', 'Quest intent', selected.intent_path),
                )}><IconCode /> Open Editor</button>
                <button onClick={branchSelectedQuest} disabled={busy}><IconSparkles /> Branch</button>
                <button onClick={exportSelectedQuest} disabled={busy}>Export</button>
                {action && (
                  <button
                    onClick={() => action.label === 'Resume'
                      ? continueSelectedQuest('Resume Quest from current evidence')
                      : transition(action.next)}
                    disabled={busy}
                  >
                    {action.label}
                  </button>
                )}
                {selected.status === 'ready_for_review' && (
                  <button onClick={rejectSelectedQuest} disabled={busy}>Reject</button>
                )}
                {!['archived', 'canceled', 'completed', 'running'].includes(selected.status) && (
                  <button onClick={cancelSelectedQuest} disabled={busy}>Cancel</button>
                )}
                {!['archived', 'canceled', 'running'].includes(selected.status) && (
                  <button onClick={() => transition('archived')} disabled={busy}>Archive</button>
                )}
                {['archived', 'canceled', 'completed'].includes(selected.status) && (
                  <button onClick={reopenSelectedQuest} disabled={busy}>Reopen</button>
                )}
                {selected.status === 'archived' && (
                  <button className="danger" onClick={remove} disabled={busy}><IconTrash /> Delete</button>
                )}
              </div>
            </header>

            <section className="quest-cockpit">
              <div className="quest-run-stream">
                <div className="quest-stream-prompt">{selected.goal}</div>
                <div className="quest-stream-list">
                  <article className="quest-stream-entry user-entry">
                    <span className="timeline-kind kind-created" />
                    <div>
                      <header><strong>Quest goal accepted</strong><time>{formatTime(selected.created_at_ms)}</time></header>
                      <small>user prompt</small>
                    </div>
                  </article>
                  {selected.events.map(event => (
                    <article key={event.id} className="quest-stream-entry">
                      <span className={`timeline-kind kind-${event.kind}`} />
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
                  <article className="quest-stream-entry next-action">
                    <span className={`timeline-kind kind-${selected.status}`} />
                    <div>
                      <header><strong>{selected.next_action.label}</strong><time>next</time></header>
                      <small>{selected.next_action.reason}</small>
                    </div>
                  </article>
                  {['draft', 'specified'].includes(selected.status) && (
                    <article className="quest-stream-entry next-action">
                      <span className="timeline-kind kind-ready" />
                      <div>
                        <header><strong>Spec is ready for review</strong><time>next</time></header>
                        <small>edit the spec in the right panel, then approve and execute</small>
                      </div>
                    </article>
                  )}
                  {executingQuestId === selected.id && (
                    <article className="quest-stream-entry live">
                      <span className="timeline-kind kind-running" />
                      <div>
                        <header><strong>Quest execution is running</strong><time>live</time></header>
                        <small>waiting for workspace events and review bundle</small>
                      </div>
                    </article>
                  )}
                  {selected.review && (
                    <button className="quest-review-chip" onClick={() => setPanel('review')}>
                      <IconCheck /> Review +{selected.review.changed_files.reduce((sum, file) => sum + file.additions, 0)}
                      <span>-{selected.review.changed_files.reduce((sum, file) => sum + file.deletions, 0)}</span>
                    </button>
                  )}
                </div>
                <div className="quest-steer-bar">
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
                  <button onClick={submitQuestInput} disabled={busy || !questInput.trim()}>
                    <IconSend />
                  </button>
                </div>
              </div>

              <aside className="quest-right-panel">
                <div className="quest-panel-tabs">
                  <button className={panel === 'overview' ? 'active' : ''} onClick={() => setPanel('overview')}><IconRefresh /> Overview</button>
                  <button className={panel === 'intent' ? 'active' : ''} onClick={() => setPanel('intent')}><IconFile /> Intent</button>
                  <button className={panel === 'spec' ? 'active' : ''} onClick={() => setPanel('spec')}><IconFile /> Spec</button>
                  <button className={panel === 'review' ? 'active' : ''} onClick={() => setPanel('review')}><IconCheck /> Review</button>
                  <button className={panel === 'knowledge' ? 'active' : ''} onClick={() => setPanel('knowledge')}><IconSparkles /> Knowledge</button>
                </div>

                {panel === 'overview' && (
                  <div className="quest-overview">
                    <section>
                      <h2>Progress</h2>
                      <ol className="quest-progress">
                        {progressItems(selected).map((item, index) => (
                          <li key={`${item.title}-${index}`} className={item.status}>
                            <span>{item.status === 'done' ? <IconCheck /> : null}</span>
                            <p>{item.title}</p>
                          </li>
                        ))}
                      </ol>
                    </section>

                    <section>
                      <h2>Artifacts</h2>
                      <button className="quest-artifact-row" onClick={() => { setArtifact(null); setPanel('intent'); }}>
                        <IconFile /><span><strong>Quest intent</strong><small>{selected.intent_path}</small></span>
                      </button>
                      <button className="quest-artifact-row" onClick={() => { setArtifact(null); setPanel('spec'); }}>
                        <IconFile /><span><strong>Quest spec</strong><small>{selected.spec_path ?? 'not generated'}</small></span>
                      </button>
                      <button className="quest-artifact-row" onClick={() => openArtifact({ kind: 'trace', label: 'Timeline trace', path: selected.trace_path })}>
                        <IconFile /><span><strong>Timeline trace</strong><small>{selected.trace_path}</small></span>
                      </button>
                      {selected.checkpoints.map(checkpoint => (
                        <button
                          className="quest-artifact-row"
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
                          className="quest-artifact-row"
                          key={attempt.id}
                          onClick={() => openArtifact({ kind: 'exploration', label: attempt.label, path: attempt.artifact_path })}
                        >
                          <IconSparkles /><span><strong>{attempt.label}</strong><small>{attempt.outcome}{attempt.selected ? ' · selected' : ''}</small></span>
                        </button>
                      ))}
                      <button className="quest-artifact-row" onClick={() => setPanel('review')} disabled={!selected.review}>
                        <IconCheck /><span><strong>Review bundle</strong><small>{selected.review ? `${selected.review.changed_files.length} changed files` : 'not ready'}</small></span>
                      </button>
                    </section>

                    <section>
                      <h2>Changed files <b>{selected.review?.changed_files.length ?? 0}</b></h2>
                      {selected.review?.changed_files.map(file => (
                        <button
                          className="quest-file-row"
                          key={file.path}
                          onClick={() => openArtifact({ kind: 'changed_file', label: file.path, path: file.path })}
                        >
                          <IconFile /><span><strong>{file.path}</strong><small>{file.status}</small></span>
                          <b>+{file.additions} <i>-{file.deletions}</i></b>
                        </button>
                      )) ?? <p className="quest-muted">No file changes yet</p>}
                    </section>

                    <section>
                      <h2>Validation</h2>
                      {selected.review?.validations.map(validation => (
                        <button
                          className="quest-validation quest-validation-action"
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
                      )) ?? <p className="quest-muted">No validation evidence yet</p>}
                    </section>

                    <section>
                      <h2>References</h2>
                      {selected.attached_knowledge.length === 0 ? (
                        <p className="quest-muted">No Knowledge attached</p>
                      ) : selected.attached_knowledge.map(entry => (
                        <button className="quest-artifact-row" key={entry.id} onClick={() => setPanel('knowledge')}>
                          <IconSparkles /><span><strong>{entry.category}</strong><small>{entry.content}</small></span>
                        </button>
                      ))}
                    </section>
                  </div>
                )}

                {panel === 'intent' && (
                  <div className="quest-spec quest-panel-document">
                    <div className="quest-section-heading">
                      <div><span>DURABLE INTENT</span><strong>Edit the task brief before execution or during revision</strong></div>
                      <div>
                        <button
                          onClick={() => onOpenEditor(selected.project.path, artifactFor('intent', 'Quest intent', selected.intent_path))}
                        >
                          <IconCode /> Open Editor
                        </button>
                        <button onClick={saveIntent} disabled={busy || intentDraft === selected.intent}><IconEdit /> Save</button>
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
                  <div className="quest-spec quest-panel-document">
                    <div className="quest-section-heading">
                      <div><span>AI TOOL SPEC</span><strong>Review the model-created artifact before execution</strong></div>
                      <div>
                        <button
                          onClick={() => onOpenEditor(selected.project.path, artifactFor('spec', 'Quest spec', selected.spec_path ?? 'spec.md'))}
                        >
                          <IconCode /> Open Editor
                        </button>
                        <button onClick={saveSpec} disabled={busy || specDraft === selected.spec}><IconEdit /> Save</button>
                        <button className="primary" onClick={execute} disabled={busy || selected.status === 'archived'}>
                          {busy ? <IconLoader className="spin-icon" /> : <IconPlay />} Approve &amp; execute
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
                  <div className="quest-artifact-viewer">
                    <header>
                      <button onClick={() => setPanel('overview')}><IconChevronRight /> Overview</button>
                      <button onClick={() => onOpenEditor(selected.project.path, artifactFor(artifact.kind, artifact.label, artifact.path))}><IconCode /> Open Editor</button>
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
                  <div className="quest-knowledge">
                    <section>
                      <h2>
                        Pending knowledge <b>{knowledge.filter(entry => entry.status === 'pending').length}</b>
                        <button onClick={revalidateKnowledgeEntries} disabled={busy}>Revalidate</button>
                      </h2>
                      {knowledge.filter(entry => entry.status === 'pending').length === 0 ? (
                        <p className="quest-muted">No pending memory proposals</p>
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
                        <p className="quest-muted">No approved project knowledge yet</p>
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
                  <div className="quest-review">
                    {!selected.review ? (
                      <div className="quest-review-empty">
                        <IconCheck size={24} />
                        <strong>No review bundle yet</strong>
                        <span>Approve the spec and execute the Quest first.</span>
                      </div>
                    ) : (
                      <>
                        <div className="quest-review-summary">
                          <div><span>RISK</span><strong>{selected.review.risk}</strong></div>
                          <p>{selected.review.summary}</p>
                        </div>
                        <section>
                          <h2>Capability metrics</h2>
                          <div className="quest-review-metrics">
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
                            <p className="quest-metric-note">{selected.review.metrics?.notes?.join(' ')}</p>
                          )}
                        </section>
                        <section>
                          <h2>Unresolved issues</h2>
                          {(selected.review.findings ?? []).length > 0 ? (
                            (selected.review.findings ?? []).map(finding => (
                              <div className="quest-issue" key={finding.id}>
                                <button
                                  className="quest-issue-open"
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
                            ? <div className="quest-issue clear"><IconCheck /> No unresolved issues</div>
                            : selected.review.unresolved_issues.map(issue => (
                              <div className="quest-issue" key={issue}>
                                <button
                                  className="quest-issue-open"
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
                            <p className="quest-muted">No guided next action is attached to this review.</p>
                          ) : (
                            <div className="quest-review-actions">
                              {(selected.review.next_actions ?? []).map(action => {
                                const needsSelection = action.kind === 'apply_selected' || action.kind === 'discard_selected';
                                const selectedActionDisabled = needsSelection
                                  && (selected.review?.transaction_groups.length
                                    ? selectedReviewGroups.size === 0
                                    : selectedReviewFiles.size === 0);
                                return (
                                  <button
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
                            <p className="quest-muted">No alternative attempts were preserved for this result.</p>
                          ) : selected.review.exploration_attempts.map(attempt => (
                            <button
                              className="quest-artifact-row quest-exploration-row"
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
                                <label className="quest-file-row quest-transaction-row" key={group.id}>
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
                            <p className="quest-muted">No changed files can be applied.</p>
                          ) : selected.review.changed_files.map(file => (
                            <label className="quest-file-row" key={file.path}>
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
                          <div className="quest-decision-row">
                            <button
                              className="primary"
                              onClick={() => selected.review?.transaction_groups.length
                                ? applySelectedQuest(undefined, Array.from(selectedReviewGroups))
                                : applySelectedQuest(Array.from(selectedReviewFiles))
                              }
                              disabled={busy || selected.status !== 'ready_for_review' || (selected.review.transaction_groups.length ? selectedReviewGroups.size === 0 : selectedReviewFiles.size === 0)}
                            >
                              <IconCheck /> Apply selected
                            </button>
                            <button
                              onClick={() => applySelectedQuest()}
                              disabled={busy || selected.status !== 'ready_for_review' || selected.review.changed_files.length === 0}
                            >
                              Apply all
                            </button>
                            <button
                              onClick={() => selected.review?.transaction_groups.length
                                ? discardSelectedQuest(undefined, Array.from(selectedReviewGroups))
                                : discardSelectedQuest(Array.from(selectedReviewFiles))
                              }
                              disabled={busy || selected.status !== 'ready_for_review' || (selected.review.transaction_groups.length ? selectedReviewGroups.size === 0 : selectedReviewFiles.size === 0)}
                            >
                              <IconX /> Discard selected
                            </button>
                            <button onClick={rejectSelectedQuest} disabled={busy || selected.status !== 'ready_for_review'}><IconX /> Reject result</button>
                            <button onClick={reviseSelectedQuest} disabled={busy || !['ready_for_review', 'blocked', 'waiting_for_user'].includes(selected.status)}><IconRefresh /> Request revision</button>
                          </div>
                          {selected.decisions.length > 0 && (
                            <div className="quest-decision-history">
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
      <footer className="quest-footer">
        <span>Registry: Aster user data · cross-project</span>
        <span>{currentProjectPath ? `Current project: ${currentProjectPath}` : 'No project open'}</span>
      </footer>
      {error && (
        <button className="quest-error-toast" onClick={() => setErrorOpen(true)} title="View error details">
          <IconAlertCircle />
          <span>
            <strong>Quest failed</strong>
            <small>{error}</small>
          </span>
          <IconChevronRight />
        </button>
      )}
      {errorOpen && error && (
        <div className="quest-error-modal" role="dialog" aria-modal="true" aria-label="Quest error details">
          <div>
            <header>
              <span><IconAlertCircle /> Quest error</span>
              <button onClick={() => setErrorOpen(false)} title="Close"><IconX /></button>
            </header>
            <pre>{error}</pre>
            <footer>
              <button onClick={() => setError(null)}>Dismiss</button>
              <button className="primary" onClick={() => setErrorOpen(false)}>Close</button>
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
    <section className="quest-group">
      <header><span>{label}</span><b>{quests.length}</b></header>
      {quests.map(quest => (
        <button key={quest.id} className={selectedId === quest.id ? 'active' : ''} onClick={() => onSelect(quest)}>
          <span className={`quest-list-status status-${quest.status}`} />
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
    <article className={`quest-knowledge-row status-${entry.status}`}>
      <header>
        <span>{entry.category}</span>
        <b>{entry.status}</b>
      </header>
      <p>{entry.content}</p>
      <small>{entry.source}</small>
      <small className={`quest-knowledge-reference status-${entry.reference_status}`}>
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
