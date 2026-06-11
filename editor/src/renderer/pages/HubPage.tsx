import React, { useState, useCallback, useEffect, useMemo } from 'react';
import { rpc, selectProjectLocation } from '../api';
import { useTranslation } from '../i18n';
import {
  IconProjects, IconInstalls, IconSettings, IconFolder, IconPlus, IconTrash, IconPlay,
  IconSun, IconMoon, IconMonitor, IconPackage, IconAlertTriangle, IconX, IconEmpty,
  AsterLogo,
} from '../icons';

// ─── Types ──────────────────────────────────────────────────────────────────

interface ProjectMeta {
  name: string;
  path: string;
  last_touched: string;
  toolchain_version: string;
}

interface InstallInfo {
  version: string;
  path: string;
  editor_available: boolean;
  runtime_available: boolean;
}

interface HubState {
  page: string;
  theme: string;
  locale: string;
  recent_projects: ProjectMeta[];
  installs: InstallInfo[];
  open_project: string | null;
}

interface Props {
  state: HubState;
  onOpenProject: (path: string) => void;
  onNavigate: (page: string) => void;
  onSetTheme: (theme: string) => void;
  onSetLocale: (locale: string) => void;
  onRefresh: () => Promise<void>;
}

// ─── Avatar helper ──────────────────────────────────────────────────────────

const AVATAR_COLORS = [
  'avatar-blue', 'avatar-green', 'avatar-purple', 'avatar-orange',
  'avatar-cyan', 'avatar-pink', 'avatar-red', 'avatar-teal',
];

function getAvatarClass(name: string): string {
  const hash = name.split('').reduce((a, c) => a + c.charCodeAt(0), 0);
  return AVATAR_COLORS[hash % AVATAR_COLORS.length];
}

function getInitials(name: string): string {
  return name
    .split(/\s+/)
    .slice(0, 2)
    .map(w => w.charAt(0).toUpperCase())
    .join('')
    .slice(0, 2) || '?';
}

// ─── Sidebar ────────────────────────────────────────────────────────────────

function Sidebar({
  page,
  theme,
  onNavigate,
  onSetTheme,
}: {
  page: string;
  theme: string;
  onNavigate: (p: string) => void;
  onSetTheme: (t: string) => void;
}) {
  const { t } = useTranslation();
  const navItems = [
    { id: 'projects', label: t('sidebar_projects'), icon: <IconProjects /> },
    { id: 'installs', label: t('sidebar_installs'), icon: <IconInstalls /> },
    { id: 'settings', label: t('sidebar_settings'), icon: <IconSettings /> },
  ];

  const themeOptions = [
    { id: 'dark', icon: <IconMoon /> },
    { id: 'light', icon: <IconSun /> },
    { id: 'system', icon: <IconMonitor /> },
  ];

  return (
    <aside className="hub-sidebar">
      {/* Logo */}
      <div className="hub-logo">
        <AsterLogo />
        <div>
          <h1>Aster</h1>
          <span>{t('app_tagline')}</span>
        </div>
      </div>

      {/* Navigation */}
      <nav className="hub-nav">
        {navItems.map(item => (
          <button
            key={item.id}
            className={`hub-nav-item ${page === item.id ? 'active' : ''}`}
            onClick={() => onNavigate(item.id)}
          >
            {item.icon}
            {item.label}
          </button>
        ))}
      </nav>

      {/* Theme Toggle */}
      <div className="hub-sidebar-footer">
        <span className="theme-toggle-label">{t('sidebar_theme')}</span>
        <div className="theme-toggle-group">
          {themeOptions.map(opt => (
            <button
              key={opt.id}
              className={`theme-toggle-btn ${theme === opt.id ? 'active' : ''}`}
              onClick={() => onSetTheme(opt.id)}
              title={opt.id.charAt(0).toUpperCase() + opt.id.slice(1)}
            >
              {opt.icon}
            </button>
          ))}
        </div>
      </div>
    </aside>
  );
}

// ─── New Project Dialog ─────────────────────────────────────────────────────

interface NewProjectDialogProps {
  installs: InstallInfo[];
  onClose: () => void;
  onCreate: (req: {
    name: string;
    location: string;
    template_id: string;
    toolchain_version: string;
  }) => Promise<void>;
}

function NewProjectDialog({ installs, onClose, onCreate }: NewProjectDialogProps) {
  const { t } = useTranslation();
  const [name, setName] = useState('');
  const [location, setLocation] = useState('');
  const [templateIdx, setTemplateIdx] = useState(0);
  const [versionIdx, setVersionIdx] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const templates = [
    { id: 'three_d', title: '3D', desc: 'Full 3D scene with camera, light, and a default cube' },
    { id: 'two_d', title: '2D', desc: 'Orthographic 2D scene with sprite renderer set up' },
  ];

  const handleCreate = useCallback(async () => {
    if (!name.trim()) { setError(t('error_project_name_required')); return; }
    if (!location.trim()) { setError(t('error_project_location_required')); return; }
    setError(null);
    setCreating(true);
    try {
      await onCreate({
        name: name.trim(),
        location: location.trim(),
        template_id: templates[templateIdx].id,
        toolchain_version: installs[versionIdx]?.version || '0.1.0',
      });
    } catch (e: unknown) {
      setError(typeof e === 'string' ? e : (e instanceof Error ? e.message : t('dialog_new_project')));
      setCreating(false);
    }
  }, [name, location, templateIdx, versionIdx, installs, onCreate]);

  const handleOverlayClick = useCallback((e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onClose();
  }, [onClose]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Escape') onClose();
    if (e.key === 'Enter' && !creating) handleCreate();
  }, [onClose, handleCreate, creating]);

  return (
    <div className="modal-overlay" onClick={handleOverlayClick} onKeyDown={handleKeyDown}>
      <div className="modal">
        <div className="modal-header">
          <h3>{t('dialog_new_project')}</h3>
          <button className="modal-close-btn" onClick={onClose}><IconX /></button>
        </div>
        <div className="modal-body">
          {/* Template */}
          <div className="form-group">
            <label className="form-label">{t('dialog_template')}</label>
            <div className="template-grid">
              {templates.map((tmpl, i) => (
                <div
                  key={tmpl.id}
                  className={`template-card ${templateIdx === i ? 'selected' : ''}`}
                  onClick={() => setTemplateIdx(i)}
                >
                  <span className="template-card-icon">
                    {i === 0 ? (
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" width="24" height="24">
                        <path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z" />
                        <polyline points="3.27 6.96 12 12.01 20.73 6.96" />
                        <line x1="12" y1="22.08" x2="12" y2="12" />
                      </svg>
                    ) : (
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" width="24" height="24">
                        <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
                        <circle cx="8.5" cy="8.5" r="1.5" />
                        <polyline points="21 15 16 10 5 21" />
                      </svg>
                    )}
                  </span>
                  <div className="template-card-title">{t('template_' + tmpl.id)}</div>
                  <div className="template-card-desc">{t('template_' + tmpl.id + '_desc')}</div>
                </div>
              ))}
            </div>
          </div>

          {/* Project Name */}
          <div className="form-group">
            <label className="form-label">{t('dialog_project_name')}</label>
            <input
              className="form-input"
              type="text"
              placeholder={t('dialog_name_hint')}
              value={name}
              onChange={e => setName(e.target.value)}
              autoFocus
            />
          </div>

          {/* Location */}
          <div className="form-group">
            <label className="form-label">{t('dialog_location')}</label>
            <div className="location-input-row">
              <input
                className="form-input"
                type="text"
                placeholder={t('dialog_location_placeholder')}
                value={location}
                onChange={e => setLocation(e.target.value)}
              />
              <button
                className="btn btn-secondary btn-sm btn-browse"
                onClick={async () => {
                  setError(null);
                  try {
                    const selected = await selectProjectLocation();
                    if (selected) setLocation(selected);
                  } catch (err) {
                    setError(err instanceof Error ? err.message : String(err));
                  }
                }}
                type="button"
              >
                {t('dialog_browse')}
              </button>
            </div>
          </div>

          {/* Toolchain Version */}
          {installs.length > 0 && (
            <div className="form-group">
              <label className="form-label">{t('dialog_engine_version')}</label>
              <select
                className="form-select"
                value={versionIdx}
                onChange={e => setVersionIdx(Number(e.target.value))}
              >
                {installs.map((inst, i) => (
                  <option key={i} value={i}>{inst.version}</option>
                ))}
              </select>
            </div>
          )}

          {/* Error */}
          {error && <p className="form-error">{error}</p>}
        </div>
        <div className="modal-footer">
          <button className="btn btn-secondary" onClick={onClose}>{t('dialog_cancel')}</button>
          <button
            className="btn btn-primary"
            onClick={handleCreate}
            disabled={creating}
          >
            {creating ? t('dialog_creating') : t('dialog_create_project')}
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── Confirm Delete Dialog ─────────────────────────────────────────────────

interface ConfirmDeleteProps {
  path: string;
  onClose: () => void;
  onRemoveRecent: () => void;
}

function ConfirmDeleteDialog({ path, onClose, onRemoveRecent }: ConfirmDeleteProps) {
  const { t, t_fmt } = useTranslation();
  const handleOverlayClick = useCallback((e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onClose();
  }, [onClose]);

  return (
    <div className="modal-overlay" onClick={handleOverlayClick}>
      <div className="modal" style={{ width: 440 }}>
        <div className="modal-header">
          <h3>{t('dialog_confirm_delete')}</h3>
          <button className="modal-close-btn" onClick={onClose}><IconX /></button>
        </div>
        <div className="modal-body">
          <div className="delete-warning">
            <IconAlertTriangle />
            <div className="delete-warning-text">
              {t_fmt('dialog_confirm_message', { path })}
            </div>
          </div>
        </div>
        <div className="modal-footer">
          <button className="btn btn-secondary" onClick={onClose}>{t('dialog_cancel')}</button>
          <button className="btn btn-danger" onClick={onRemoveRecent}>
            {t('dialog_remove_recents')}
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── Projects Page ──────────────────────────────────────────────────────────

function ProjectsPage({
  projects,
  selectedPath,
  onSelect,
  onOpen,
  onDeleteRequest,
  onNewProject,
}: {
  projects: ProjectMeta[];
  selectedPath: string | null;
  onSelect: (path: string | null) => void;
  onOpen: (path: string) => void;
  onDeleteRequest: (path: string) => void;
  onNewProject: () => void;
}) {
  const { t } = useTranslation();
  const [search, setSearch] = useState('');

  const filtered = projects.filter(p =>
    p.name.toLowerCase().includes(search.toLowerCase())
  );

  const handleCardClick = useCallback((path: string) => {
    if (selectedPath === path) {
      onSelect(null);
    } else {
      onSelect(path);
    }
  }, [selectedPath, onSelect]);

  const handleCardDoubleClick = useCallback((path: string) => {
    onOpen(path);
  }, [onOpen]);

  const handleOpenFolder = useCallback(async (e: React.MouseEvent, path: string) => {
    e.stopPropagation();
    try {
      await rpc('app/open_folder', { path });
    } catch {
      // folder open not supported on this platform
    }
  }, []);

  const selectedProject = projects.find(p => p.path === selectedPath);

  return (
    <>
      {/* Header */}
      <div className="hub-page-header">
        <h2>{t('hub_projects_title')}</h2>
        <div className="hub-page-actions">
          <button className="btn btn-primary btn-sm" onClick={onNewProject}>
            <IconPlus /> {t('hub_new_project')}
          </button>
        </div>
      </div>

      {/* Search */}
      <div className="hub-search-bar">
        <input
          className="hub-search"
          type="text"
          placeholder={t('hub_search')}
          value={search}
          onChange={e => setSearch(e.target.value)}
        />
      </div>

      {/* Action bar (shown when a project is selected) */}
      <div className={`hub-action-bar ${selectedProject ? 'visible' : ''}`}>
        {selectedProject && (
          <>
            <span className="hub-action-bar-label">
              {selectedProject.name}
            </span>
            <button className="btn btn-sm btn-primary" onClick={() => onOpen(selectedProject.path)}>
              <IconPlay /> {t('hub_launch')}
            </button>
            <button className="btn btn-sm btn-danger" onClick={() => onDeleteRequest(selectedProject.path)}>
              <IconTrash /> {t('hub_delete')}
            </button>
          </>
        )}
      </div>

      {/* Project Cards */}
      <div className="hub-scroll">
        {filtered.length === 0 ? (
          <div className="hub-empty">
            <div className="hub-empty-icon"><IconEmpty /></div>
            {search ? (
              <>
                <h3>{t('hub_search_no_matches')}</h3>
                <p>{t('hub_search_no_matches_desc')}</p>
              </>
            ) : (
              <>
                <h3>{t('hub_no_projects')}</h3>
                <p>{t('hub_no_projects_desc')}</p>
              </>
            )}
          </div>
        ) : (
          <div className="hub-grid">
            {filtered.map(project => (
              <div
                key={project.path}
                className={`project-card ${selectedPath === project.path ? 'selected' : ''}`}
                onClick={() => handleCardClick(project.path)}
                onDoubleClick={() => handleCardDoubleClick(project.path)}
              >
                <div className={`project-avatar ${getAvatarClass(project.name)}`}>
                  {getInitials(project.name)}
                </div>
                <div className="project-info">
                  <div className="project-name">{project.name}</div>
                  <div className="project-path">{project.path}</div>
                  <div className="project-meta">
                    <span>{project.toolchain_version}</span>
                    <span className="project-meta-dot" />
                    <span>{project.last_touched.slice(0, 10)}</span>
                  </div>
                </div>
                <button
                  className="project-folder-btn"
                  onClick={e => handleOpenFolder(e, project.path)}
                  title={t('hub_open_folder')}
                >
                  <IconFolder />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </>
  );
}

// ─── Installs Page ──────────────────────────────────────────────────────────

function InstallsPage({ installs }: { installs: InstallInfo[] }) {
  const { t } = useTranslation();
  return (
    <>
      <div className="hub-page-header">
        <h2>{t('hub_installs_title')}</h2>
      </div>
      <div className="hub-scroll">
        {installs.length === 0 ? (
          <div className="hub-empty">
            <div className="hub-empty-icon"><IconPackage /></div>
            <h3>{t('hub_installs_empty')}</h3>
            <p>{t('hub_installs_empty_desc')}</p>
          </div>
        ) : (
          <div className="install-list">
            {installs.map((inst, i) => (
              <div key={i} className="install-card">
                <div className="install-icon"><IconPackage /></div>
                <div className="install-info">
                  <div className="install-version">{inst.version}</div>
                  <div className="install-path">{inst.path}</div>
                </div>
                <div className="install-badges">
                  {inst.editor_available && <span className="badge badge-green">{t('hub_installs_badge_editor')}</span>}
                  {!inst.editor_available && <span className="badge badge-gray">{t('hub_installs_badge_no_editor')}</span>}
                  {inst.runtime_available && <span className="badge badge-green">{t('hub_installs_badge_runtime')}</span>}
                  {!inst.runtime_available && <span className="badge badge-gray">{t('hub_installs_badge_no_runtime')}</span>}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </>
  );
}

// ─── Copilot Settings ────────────────────────────────────────────────────────

interface ModelInfo {
  id: string;
  display_name: string;
  provider: string;
  context_window: number;
  default_max_tokens: number;
  capabilities: {
    can_reason: boolean;
    supports_vision: boolean;
    supports_tools: boolean;
  };
}

interface ProviderMeta {
  provider: string;
  display_name: string;
  requires_api_key: boolean;
  requires_endpoint: boolean;
  default_endpoint: string | null;
  models: ModelInfo[];
}

interface MimoConfig {
  billing: 'subscription' | 'api';
  region: 'china' | 'singapore' | 'europe';
}

interface GlmConfig {
  billing: 'subscription' | 'api';
  region: 'bigmodel' | 'zai';
}

interface CopilotSettingsData {
  provider: 'stub' | 'anthropic' | 'openai' | 'codex_oauth' | 'gemini' | 'ollama' | 'custom' | 'mimo' | 'deepseek' | 'glm';
  model: string;
  api_endpoint: string | null;
  api_key: string | null;
  has_api_key?: boolean;
  max_tokens: number;
  mimo_config?: MimoConfig;
  glm_config?: GlmConfig;
}

const PROVIDER_OPTIONS: Array<{ value: CopilotSettingsData['provider']; label: string }> = [
  { value: 'anthropic', label: 'Anthropic (Claude)' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'codex_oauth', label: 'Codex OAuth (ChatGPT)' },
  { value: 'gemini', label: 'Google Gemini' },
  { value: 'deepseek', label: 'DeepSeek' },
  { value: 'mimo', label: 'Xiaomi MiMo' },
  { value: 'glm', label: 'GLM/Zhipu AI' },
  { value: 'ollama', label: 'Ollama (Local)' },
  { value: 'custom', label: 'Custom (OpenAI-Compatible)' },
  { value: 'stub', label: 'None (Disabled)' },
];

function CopilotSettingsSection() {
  const [settings, setSettings] = useState<CopilotSettingsData>({
    provider: 'stub',
    model: '',
    api_endpoint: null,
    api_key: null,
    max_tokens: 4096,
  });
  const [saving, setSaving] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [providerMetas, setProviderMetas] = useState<ProviderMeta[]>([]);
  const [codexConnected, setCodexConnected] = useState(false);
  const [codexCode, setCodexCode] = useState<string | null>(null);
  const [codexAuthBusy, setCodexAuthBusy] = useState(false);
  const [codexAuthError, setCodexAuthError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [keyChanged, setKeyChanged] = useState(false);

  useEffect(() => {
    Promise.all([
      rpc<CopilotSettingsData>('app/get_copilot_settings').catch(() => null),
      rpc<{ providers: ProviderMeta[] }>('app/get_model_registry').catch(() => ({ providers: [] })),
    ]).then(([s, reg]) => {
      if (s) {
        const providerMap: Record<string, CopilotSettingsData['provider']> = { open_a_i: 'openai' };
        const normalized = providerMap[s.provider] ?? s.provider;
        setSettings({ ...s, provider: normalized as CopilotSettingsData['provider'] });
      }
      setProviderMetas(reg.providers);
      setLoaded(true);
    });
    rpc<{ connected: boolean }>('app/codex_oauth_status')
      .then(status => setCodexConnected(status.connected))
      .catch(() => setCodexConnected(false));
  }, []);

  const currentMeta = useMemo(
    () => providerMetas.find(p => p.provider === settings.provider),
    [providerMetas, settings.provider]
  );

  const handleProviderChange = useCallback((provider: CopilotSettingsData['provider']) => {
    setSettings(s => ({ ...s, provider, api_endpoint: null }));
  }, []);

  const handleSave = useCallback(async () => {
    setSaving(true);
    setSaved(false);
    try {
      const payload = { ...settings };
      if (!keyChanged) delete (payload as any).api_key;
      await rpc('app/update_copilot_settings', payload);
      setSaved(true);
      setKeyChanged(false);
      setTimeout(() => setSaved(false), 2000);
    } catch { /* ignore */ }
    setSaving(false);
  }, [settings, keyChanged]);

  const handleCodexLogin = useCallback(async () => {
    setCodexAuthBusy(true);
    setCodexAuthError(null);
    try {
      const auth = await rpc<{ url: string; user_code: string; interval_seconds: number }>(
        'app/codex_oauth_start',
      );
      setCodexCode(auth.user_code);
      await rpc('app/open_folder', { path: auth.url });
      for (let attempt = 0; attempt < 100; attempt += 1) {
        await new Promise(resolve => setTimeout(resolve, (auth.interval_seconds + 3) * 1000));
        const result = await rpc<{ status: 'pending' | 'connected' }>('app/codex_oauth_poll');
        if (result.status === 'connected') {
          setCodexConnected(true);
          setCodexCode(null);
          setSettings(current => ({ ...current, provider: 'codex_oauth' }));
          return;
        }
      }
      throw new Error('Codex authorization timed out');
    } catch (err: any) {
      setCodexAuthError(typeof err === 'string' ? err : err.message || 'Authorization failed');
    } finally {
      setCodexAuthBusy(false);
    }
  }, []);

  const handleCodexLogout = useCallback(async () => {
    await rpc('app/codex_oauth_logout');
    setCodexConnected(false);
    setCodexCode(null);
  }, []);

  const showApiKey = currentMeta?.requires_api_key ?? (settings.provider !== 'ollama' && settings.provider !== 'stub');
  // MiMo and GLM have auto-determined endpoints based on region/billing config
  const endpointAutoDetermined = settings.provider === 'mimo' || settings.provider === 'glm';
  const showEndpoint = settings.provider !== 'stub' && !endpointAutoDetermined;
  const endpointRequired = settings.provider === 'custom';

  if (!loaded) return null;

  return (
    <div className="settings-section">
      <div className="settings-section-title">AI Provider</div>

      {/* Provider */}
      <div className="settings-row">
        <div>
          <div className="settings-label">Provider</div>
          <div className="settings-desc">Select your AI model provider</div>
        </div>
        <div className="settings-control">
          <select
            value={settings.provider}
            onChange={(e) => handleProviderChange(e.target.value as CopilotSettingsData['provider'])}
            style={{ minWidth: 200 }}
          >
            {PROVIDER_OPTIONS.map(opt => (
              <option key={opt.value} value={opt.value}>{opt.label}</option>
            ))}
          </select>
        </div>
      </div>

      {/* API Key */}
      {showApiKey && (
        <div className="settings-row">
          <div>
            <div className="settings-label">API Key</div>
            <div className="settings-desc">Authentication credential for the provider</div>
          </div>
          <div className="settings-control">
            <input
              type="password"
              value={settings.api_key ?? ''}
              placeholder={settings.has_api_key ? '••••••••••••' : 'sk-...'}
              onChange={(e) => {
                setSettings(s => ({ ...s, api_key: e.target.value || null }));
                setKeyChanged(true);
              }}
              style={{ minWidth: 200 }}
            />
          </div>
        </div>
      )}

      {/* Codex OAuth */}
      {settings.provider === 'codex_oauth' && (
        <div className="settings-row">
          <div>
            <div className="settings-label">ChatGPT Account</div>
            <div className="settings-desc">Sign in with your ChatGPT subscription</div>
          </div>
          <div className="settings-control" style={{ flexDirection: 'column', alignItems: 'flex-end', gap: 4 }}>
            <button
              className="btn btn-primary btn-sm"
              onClick={codexConnected ? handleCodexLogout : handleCodexLogin}
              disabled={codexAuthBusy}
            >
              {codexAuthBusy ? 'Waiting for authorization...' : codexConnected ? 'Sign out' : 'Sign in with ChatGPT'}
            </button>
            {codexConnected && <small style={{ color: 'var(--success)' }}>Connected</small>}
            {codexCode && <small>Enter code <strong>{codexCode}</strong> in the browser.</small>}
            {codexAuthError && <small style={{ color: 'var(--error)' }}>{codexAuthError}</small>}
          </div>
        </div>
      )}

      {/* Endpoint */}
      {showEndpoint && (
        <div className="settings-row">
          <div>
            <div className="settings-label">
              Endpoint {endpointRequired ? '' : <small style={{ opacity: 0.6 }}>(optional override)</small>}
            </div>
            <div className="settings-desc">API base URL for the provider</div>
          </div>
          <div className="settings-control">
            <input
              type="text"
              value={settings.api_endpoint ?? ''}
              placeholder={currentMeta?.default_endpoint ?? 'https://api.example.com/v1'}
              onChange={(e) => setSettings(s => ({ ...s, api_endpoint: e.target.value || null }))}
              style={{ minWidth: 200 }}
            />
          </div>
        </div>
      )}

      {/* MiMo Configuration */}
      {settings.provider === 'mimo' && (
        <>
          <div className="settings-row">
            <div>
              <div className="settings-label">Billing Mode</div>
              <div className="settings-desc">Token Plan subscription or pay-as-you-go API</div>
            </div>
            <div className="settings-control">
              <select
                value={settings.mimo_config?.billing ?? 'subscription'}
                onChange={(e) => setSettings(s => ({
                  ...s,
                  mimo_config: {
                    ...s.mimo_config,
                    billing: e.target.value as 'subscription' | 'api',
                    region: s.mimo_config?.region ?? 'china',
                  }
                }))}
                style={{ minWidth: 150 }}
              >
                <option value="subscription">Token Plan</option>
                <option value="api">Pay-as-you-go</option>
              </select>
            </div>
          </div>
          <div className="settings-row">
            <div>
              <div className="settings-label">Region</div>
              <div className="settings-desc">Regional cluster for Token Plan</div>
            </div>
            <div className="settings-control">
              <select
                value={settings.mimo_config?.region ?? 'china'}
                onChange={(e) => setSettings(s => ({
                  ...s,
                  mimo_config: {
                    ...s.mimo_config,
                    billing: s.mimo_config?.billing ?? 'subscription',
                    region: e.target.value as 'china' | 'singapore' | 'europe',
                  }
                }))}
                style={{ minWidth: 150 }}
              >
                <option value="china">China</option>
                <option value="singapore">Singapore</option>
                <option value="europe">Europe</option>
              </select>
            </div>
          </div>
        </>
      )}

      {/* GLM Configuration */}
      {settings.provider === 'glm' && (
        <>
          <div className="settings-row">
            <div>
              <div className="settings-label">Billing Mode</div>
              <div className="settings-desc">Subscription or pay-as-you-go API</div>
            </div>
            <div className="settings-control">
              <select
                value={settings.glm_config?.billing ?? 'subscription'}
                onChange={(e) => setSettings(s => ({
                  ...s,
                  glm_config: {
                    ...s.glm_config,
                    billing: e.target.value as 'subscription' | 'api',
                    region: s.glm_config?.region ?? 'bigmodel',
                  }
                }))}
                style={{ minWidth: 150 }}
              >
                <option value="subscription">Subscription</option>
                <option value="api">Pay-as-you-go</option>
              </select>
            </div>
          </div>
          <div className="settings-row">
            <div>
              <div className="settings-label">Region</div>
              <div className="settings-desc">Bigmodel (China) or ZAI (International)</div>
            </div>
            <div className="settings-control">
              <select
                value={settings.glm_config?.region ?? 'bigmodel'}
                onChange={(e) => setSettings(s => ({
                  ...s,
                  glm_config: {
                    ...s.glm_config,
                    billing: s.glm_config?.billing ?? 'subscription',
                    region: e.target.value as 'bigmodel' | 'zai',
                  }
                }))}
                style={{ minWidth: 150 }}
              >
                <option value="bigmodel">Bigmodel (China)</option>
                <option value="zai">ZAI (International)</option>
              </select>
            </div>
          </div>
        </>
      )}

      {/* Max Tokens */}
      {settings.provider !== 'stub' && (
        <div className="settings-row">
          <div>
            <div className="settings-label">Max Tokens</div>
            <div className="settings-desc">Maximum response length</div>
          </div>
          <div className="settings-control">
            <input
              type="number"
              value={settings.max_tokens}
              min={256}
              max={128000}
              onChange={(e) => setSettings(s => ({ ...s, max_tokens: parseInt(e.target.value) || 4096 }))}
              style={{ width: 100 }}
            />
          </div>
        </div>
      )}

      {/* Save button */}
      <div className="settings-row">
        <div />
        <div className="settings-control">
          <button className="btn btn-primary btn-sm" onClick={handleSave} disabled={saving}>
            {saving ? 'Saving...' : saved ? 'Saved!' : 'Save AI Settings'}
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── Settings Page ──────────────────────────────────────────────────────────

function SettingsPage({
  theme,
  locale,
  onSetTheme,
  onSetLocale,
}: {
  theme: string;
  locale: string;
  onSetTheme: (t: string) => void;
  onSetLocale: (l: string) => void;
}) {
  const { t, t_fmt } = useTranslation();
  return (
    <>
      <div className="hub-page-header">
        <h2>{t('hub_settings_title')}</h2>
      </div>
      <div className="hub-scroll" style={{ maxWidth: 520 }}>
        {/* Theme */}
        <div className="settings-section">
          <div className="settings-section-title">{t('settings_appearance')}</div>
          <div className="settings-row">
            <div>
              <div className="settings-label">{t('settings_theme')}</div>
              <div className="settings-desc">{t('settings_theme_desc')}</div>
            </div>
            <div className="settings-control">
              <div className="theme-selector">
                {[
                  { id: 'dark', label: t('settings_theme_dark') },
                  { id: 'light', label: t('settings_theme_light') },
                  { id: 'system', label: t('settings_theme_system') },
                ].map(opt => (
                  <button
                    key={opt.id}
                    className={`theme-option ${theme === opt.id ? 'active' : ''}`}
                    onClick={() => onSetTheme(opt.id)}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </div>

        {/* Language */}
        <div className="settings-section">
          <div className="settings-section-title">{t('settings_language')}</div>
          <div className="settings-row">
            <div>
              <div className="settings-label">{t('settings_editor_language')}</div>
              <div className="settings-desc">{t('settings_language_desc')}</div>
            </div>
            <div className="settings-control" style={{ display: 'flex', gap: 4 }}>
              {[
                { id: 'en', label: t('settings_language_en') },
                { id: 'zh', label: t('settings_language_zh') },
              ].map(opt => (
                <button
                  key={opt.id}
                  className={`lang-btn ${locale === opt.id ? 'active' : ''}`}
                  onClick={() => onSetLocale(opt.id)}
                >
                  {opt.label}
                </button>
              ))}
            </div>
          </div>
        </div>

        {/* AI Provider */}
        <CopilotSettingsSection />

        {/* About */}
        <div className="settings-section">
          <div className="settings-section-title">{t('settings_about')}</div>
          <div className="settings-row">
            <div>
              <div className="settings-label">{t('settings_about_name')}</div>
              <div className="settings-desc">{t_fmt('settings_about_version', { version: '0.1.0' })}</div>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

// ─── HubPage (Root) ─────────────────────────────────────────────────────────

export default function HubPage({ state, onOpenProject, onNavigate, onSetTheme, onSetLocale, onRefresh }: Props) {
  const [selectedProject, setSelectedProject] = useState<string | null>(null);
  const [showNewDialog, setShowNewDialog] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);

  // Reset selection when projects change
  useEffect(() => {
    setSelectedProject(prev => {
      if (!prev) return null;
      return state.recent_projects.some(p => p.path === prev) ? prev : null;
    });
  }, [state.recent_projects]);

  const handleNewProjectCreate = useCallback(async (req: {
    name: string;
    location: string;
    template_id: string;
    toolchain_version: string;
  }) => {
    await rpc('hub/create_project', {
      name: req.name,
      location: req.location,
      template_id: req.template_id,
      toolchain_version: req.toolchain_version,
    });
    setShowNewDialog(false);
    // Open the newly created project — use native path separator
    const sep = req.location.includes('\\') ? '\\' : '/';
    const createdPath = `${req.location}${sep}${req.name}`;
    await onOpenProject(createdPath);
  }, [onOpenProject]);

  const handleDeleteRecent = useCallback(async () => {
    if (!deleteTarget) return;
    try {
      await rpc('hub/delete_project', { path: deleteTarget, confirmed: true });
      await onRefresh();
    } catch {
      // Backend may refuse if project is open — silent
    }
    setDeleteTarget(null);
  }, [deleteTarget, onRefresh]);

  // Render the active page
  const renderPage = () => {
    switch (state.page) {
      case 'installs':
        return <InstallsPage installs={state.installs} />;
      case 'settings':
        return (
          <SettingsPage
            theme={state.theme}
            locale={state.locale}
            onSetTheme={onSetTheme}
            onSetLocale={onSetLocale}
          />
        );
      default:
        return (
          <ProjectsPage
            projects={state.recent_projects}
            selectedPath={selectedProject}
            onSelect={setSelectedProject}
            onOpen={onOpenProject}
            onDeleteRequest={setDeleteTarget}
            onNewProject={() => setShowNewDialog(true)}
          />
        );
    }
  };

  return (
    <div className="hub">
      <Sidebar
        page={state.page}
        theme={state.theme}
        onNavigate={onNavigate}
        onSetTheme={onSetTheme}
      />

      <main className="hub-main">
        {renderPage()}
      </main>

      {/* New Project Dialog */}
      {showNewDialog && (
        <NewProjectDialog
          installs={state.installs}
          onClose={() => setShowNewDialog(false)}
          onCreate={handleNewProjectCreate}
        />
      )}

      {/* Delete Confirmation */}
      {deleteTarget && (
        <ConfirmDeleteDialog
          path={deleteTarget}
          onClose={() => setDeleteTarget(null)}
          onRemoveRecent={handleDeleteRecent}
        />
      )}
    </div>
  );
}
