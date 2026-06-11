import React, { useEffect, useState, useCallback } from 'react';
import HubPage from './pages/HubPage';
import EditorPage from './pages/EditorPage';
import GameView from './pages/GameView';
import { rpc } from './api';
import { I18nProvider } from './i18n';

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

type Screen = 'loading' | 'hub' | 'editor' | 'game-view';

function AppFrame({ children }: { children: React.ReactNode }) {
  return (
    <div className="app-frame">
      <div className="app-frame-content">{children}</div>
    </div>
  );
}

export default function App() {
  const [screen, setScreen] = useState<Screen>('loading');
  const [hubState, setHubState] = useState<HubState | null>(null);

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

  // ── Init ──
  useEffect(() => {
    // Check hash-based routing first (Game View opens via Tauri new window)
    if (window.location.hash === '#/game-view') {
      rpc<DesktopIntegration>('app/get_desktop_integration')
        .then(applyDesktopIntegration)
        .catch(() => {});
      setScreen('game-view');
      return;
    }

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
      setScreen('hub');
    });
  }, [applyDesktopIntegration]);

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

  const locale = hubState?.locale ?? 'en';

  if (screen === 'game-view') {
    return <GameView />;
  }

  if (screen === 'loading') {
    return (
      <I18nProvider locale={locale}>
        <AppFrame>
          <div className="loading-screen">
            <div className="spinner" />
            <span>Loading...</span>
          </div>
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
          />
        </AppFrame>
      </I18nProvider>
    );
  }

  return (
    <I18nProvider locale={locale}>
      <EditorPage onCloseProject={handleCloseProject} onOpenSettings={handleOpenSettings} />
    </I18nProvider>
  );
}
