import React, { useEffect, useState, useCallback } from 'react';
import HubPage from './pages/HubPage';
import EditorPage from './pages/EditorPage';
import QuestPage from './pages/QuestPage';
import { rpc } from './api';
import { I18nProvider, useTranslation } from './i18n';
import { buttonClass } from './uiClasses';

interface DesktopIntegration {
  desktop_environment: string;
  prefers_native_chrome: boolean;
  window_background: string;
  window_backend?: string;
}

interface HubState {
  page: string;
  theme: string;
  locale: string;
  recent_projects: Array<{
    name: string;
    path: string;
    last_touched: string;
    toolchain_version: string;
  }>;
  installs: Array<{
    version: string;
    path: string;
    editor_available: boolean;
    runtime_available: boolean;
  }>;
  open_project: string | null;
  desktop_integration?: DesktopIntegration;
}

type Screen = 'loading' | 'hub' | 'editor' | 'quest';

export interface QuestEditorArtifact {
  questId: string;
  questTitle: string;
  kind: 'intent' | 'spec' | 'trace' | 'changed_file' | 'validation' | 'review_finding' | 'exploration' | 'checkpoint';
  label: string;
  path?: string;
}

function AppFrame({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-full min-h-0 w-full flex-col bg-[var(--bg-base)]">
      <div className="min-h-0 flex-1 overflow-hidden">{children}</div>
    </div>
  );
}

function LoadingScreen() {
  const { t } = useTranslation();
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 text-[var(--text-secondary)]">
      <div className="h-6 w-6 animate-spin rounded-full border-2 border-[var(--border-light)] border-t-[var(--accent)]" />
      <span>{t('loading')}</span>
    </div>
  );
}

function StartupErrorScreen({ message, onRetry }: { message: string; onRetry: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="grid h-full place-items-center p-6 text-[var(--text-secondary)]">
      <div className="flex w-[min(420px,100%)] flex-col gap-3 rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-surface)] p-5">
        <h1 className="text-base font-semibold text-[var(--text-primary)]">{t('startup_error_title')}</h1>
        <p className="break-anywhere leading-[1.5]">{message}</p>
        <button type="button" className={buttonClass('primary')} onClick={onRetry}>{t('btn_retry')}</button>
      </div>
    </div>
  );
}

export default function App() {
  const [screen, setScreen] = useState<Screen>('loading');
  const [hubState, setHubState] = useState<HubState | null>(null);
  const [startupError, setStartupError] = useState<string | null>(null);
  const [questArtifact, setQuestArtifact] = useState<QuestEditorArtifact | null>(null);
  const [initialQuestId, setInitialQuestId] = useState<string | null>(null);

  // ── Refresh hub state from backend ──
  const refreshHubState = useCallback(async () => {
    try {
      const state = await rpc<HubState>('hub/get_state');
      setHubState(state);
      return state;
    } catch {
      return null;
    }
  }, []);

  // ── Apply desktop integration styles ──
  const applyDesktopIntegration = useCallback((integration: DesktopIntegration) => {
    document.body.dataset.desktopEnvironment = integration.desktop_environment;
    document.body.dataset.nativeChrome = String(integration.prefers_native_chrome);
    document.documentElement.style.background = integration.window_background;
    document.body.style.background = integration.window_background;
  }, []);

  const initialize = useCallback(() => {
    setStartupError(null);
    setScreen('loading');

    // Normal editor startup
    Promise.all([
      rpc<DesktopIntegration>('app/get_desktop_integration'),
      rpc<HubState>('hub/get_state'),
    ]).then(([integration, state]) => {
      applyDesktopIntegration(integration);
      // Apply theme from saved state
      if (state.theme === 'system') {
        const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
        document.documentElement.dataset.theme = prefersDark ? 'dark' : 'light';
      } else {
        document.documentElement.dataset.theme = state.theme;
      }
      setHubState(state);
      setScreen(state.open_project ? 'editor' : 'hub');
    }).catch((err) => {
      console.error('Failed to connect to host:', err);
      const message = err instanceof Error ? err.message : String(err);
      setStartupError(message || 'The editor backend did not respond.');
      setScreen('loading');
    });
  }, [applyDesktopIntegration]);

  // ── Init ──
  useEffect(() => {
    initialize();
  }, [initialize]);

  // ── Handlers ──

  const handleOpenProject = useCallback(async (path: string) => {
    await rpc('hub/open_project', { path });
    const state = await refreshHubState();
    if (state) setScreen('editor');
  }, [refreshHubState]);

  const handleCloseProject = useCallback(async () => {
    await rpc('shell/close_project');
    await refreshHubState();
    // Synced hub state should reflect no open project
    setScreen('hub');
  }, [refreshHubState]);

  const handleOpenSettings = useCallback(async () => {
    await rpc('shell/close_project');
    await rpc('hub/set_page', { page: 'settings' });
    await refreshHubState();
    setScreen('hub');
  }, [refreshHubState]);

  const handleOpenQuest = useCallback(() => {
    setInitialQuestId(null);
    setScreen('quest');
  }, []);

  const handleOpenEditor = useCallback(async (projectPath: string, artifact?: QuestEditorArtifact) => {
    if (hubState?.open_project !== projectPath) {
      await rpc('hub/open_project', { path: projectPath });
      await refreshHubState();
    }
    setQuestArtifact(artifact ?? null);
    setScreen('editor');
  }, [hubState?.open_project, refreshHubState]);

  const handleNavigate = useCallback(async (page: string) => {
    await rpc('hub/set_page', { page });
    setHubState(prev => prev ? { ...prev, page } : prev);
  }, []);

  const handleSetTheme = useCallback(async (theme: string) => {
    await rpc('hub/set_theme', { theme });
    // Apply theme to DOM immediately so CSS reacts
    if (theme === 'system') {
      const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
      document.documentElement.dataset.theme = prefersDark ? 'dark' : 'light';
    } else {
      document.documentElement.dataset.theme = theme;
    }
    setHubState(prev => prev ? { ...prev, theme } : prev);
  }, []);

  const handleSetLocale = useCallback(async (locale: string) => {
    await rpc('hub/set_locale', { locale });
    setHubState(prev => prev ? { ...prev, locale } : prev);
  }, []);

  // ── Render ──

  const locale = hubState?.locale ?? 'zh';

  if (screen === 'loading') {
    return (
      <I18nProvider locale={locale}>
        <AppFrame>
          {startupError ? (
            <StartupErrorScreen message={startupError} onRetry={initialize} />
          ) : (
            <LoadingScreen />
          )}
        </AppFrame>
      </I18nProvider>
    );
  }

  if (screen === 'hub' && hubState) {
    return (
      <I18nProvider locale={locale}>
        <AppFrame>
          <HubPage
            state={hubState}
            onOpenProject={handleOpenProject}
            onNavigate={handleNavigate}
            onSetTheme={handleSetTheme}
            onSetLocale={handleSetLocale}
            onRefresh={async () => { await refreshHubState(); }}
            onOpenQuests={handleOpenQuest}
          />
        </AppFrame>
      </I18nProvider>
    );
  }

  if (screen === 'quest' && hubState) {
    return (
      <I18nProvider locale={locale}>
        <QuestPage
          currentProjectPath={hubState.open_project}
          initialQuestId={initialQuestId}
          onOpenEditor={handleOpenEditor}
          onCloseProject={handleCloseProject}
        />
      </I18nProvider>
    );
  }

  return (
    <I18nProvider locale={locale}>
      <EditorPage
        onCloseProject={handleCloseProject}
        onOpenSettings={handleOpenSettings}
        onOpenQuest={handleOpenQuest}
        questArtifact={questArtifact}
        onDismissQuestArtifact={() => setQuestArtifact(null)}
      />
    </I18nProvider>
  );
}
