import React, { useCallback, useEffect, useRef, useState } from 'react';
import { rpc } from '../api';
import { useTranslation } from '../i18n';
import {
  IconSave, IconUndo, IconRedo, IconPlay, IconMove, IconRotate, IconScale, IconView,
  IconX, IconChevronDown, IconChevronRight, IconPlus, IconMenu,
} from '../icons';

// ─── Shared dropdown hook ──────────────────────────────────────────────────

function useDropdown() {
  const [openMenu, setOpenMenu] = useState<string | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!openMenu) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpenMenu(null);
      }
    };
    window.addEventListener('mousedown', handler);
    return () => window.removeEventListener('mousedown', handler);
  }, [openMenu]);

  return { openMenu, setOpenMenu, ref };
}

// ─── Menu Item types ───────────────────────────────────────────────────────

interface MenuItem {
  label?: string;
  shortcut?: string;
  disabled?: boolean;
  action?: () => void;
  divider?: boolean;
  submenu?: MenuItem[];
}

interface MenuDef {
  label: string;
  items: MenuItem[];
}

// ─── MenuBar Component ─────────────────────────────────────────────────────

interface MenuBarProps {
  menus: MenuDef[];
  onCloseProject: () => void;
}

export function MenuBar({ menus, onCloseProject }: MenuBarProps) {
  const { openMenu, setOpenMenu, ref } = useDropdown();

  return (
    <div className="menubar" ref={ref}>
      {menus.map((menu) => (
        <div key={menu.label} className="menubar-menu">
          <button
            className={`menubar-trigger ${openMenu === menu.label ? 'active' : ''}`}
            onClick={() => setOpenMenu(openMenu === menu.label ? null : menu.label)}
            onMouseEnter={() => openMenu && setOpenMenu(menu.label)}
          >
            {menu.label}
          </button>
          {openMenu === menu.label && (
            <div className="menubar-dropdown">
              {menu.items.map((item, i) => {
                if (item.divider) {
                  return <div key={i} className="menubar-divider" />;
                }
                if (item.submenu) {
                  return (
                    <SubmenuItem key={i} item={item} depth={0} onClose={() => setOpenMenu(null)} />
                  );
                }
                return (
                  <button
                    key={i}
                    className={`menubar-item ${item.disabled ? 'disabled' : ''}`}
                    disabled={item.disabled}
                    onClick={() => {
                      item.action?.();
                      setOpenMenu(null);
                    }}
                  >
                    <span className="menubar-item-label">{item.label}</span>
                    {item.shortcut && <span className="menubar-item-shortcut">{item.shortcut}</span>}
                  </button>
                );
              })}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

// ─── Submenu item with nested dropdown ─────────────────────────────────────

function SubmenuItem({ item, depth, onClose }: { item: MenuItem; depth: number; onClose: () => void }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    window.addEventListener('mousedown', handler);
    return () => window.removeEventListener('mousedown', handler);
  }, [open]);

  return (
    <div
      ref={ref}
      className="menubar-item menubar-submenu"
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
      onClick={() => setOpen(!open)}
    >
      <span className="menubar-item-label">{item.label}</span>
      <IconChevronRight size={12} />
      {open && item.submenu && (
        <div className="menubar-dropdown menubar-submenu-dropdown" style={{ left: '100%', top: 0 }}>
          {item.submenu.map((sub, i) => (
            sub.divider ? (
              <div key={i} className="menubar-divider" />
            ) : (
              <button
                key={i}
                className={`menubar-item ${sub.disabled ? 'disabled' : ''}`}
                disabled={sub.disabled}
                onClick={() => {
                  sub.action?.();
                  onClose();
                }}
              >
                <span className="menubar-item-label">{sub.label}</span>
                {sub.shortcut && <span className="menubar-item-shortcut">{sub.shortcut}</span>}
              </button>
            )
          ))}
        </div>
      )}
    </div>
  );
}

// ─── Toolbar Extras ────────────────────────────────────────────────────────

export type TransformTool = 'view' | 'move' | 'rotate' | 'scale';

interface ToolbarExtrasProps {
  activeTool: TransformTool;
  onToolChange: (tool: TransformTool) => void;
  space: 'global' | 'local';
  onSpaceChange: (space: 'global' | 'local') => void;
  moveSnap: number;
  onMoveSnapChange: (snap: number) => void;
  angleSnap: number;
  onAngleSnapChange: (snap: number) => void;
}

export function ToolbarExtras({
  activeTool,
  onToolChange,
  space,
  onSpaceChange,
  moveSnap,
  onMoveSnapChange,
  angleSnap,
  onAngleSnapChange,
}: ToolbarExtrasProps) {
  const { t } = useTranslation();
  const [showSnap, setShowSnap] = useState(false);

  const tools: { key: TransformTool; icon: React.ReactNode; label: string; shortcut: string }[] = [
    { key: 'view',   icon: <IconView size={16} />,   label: t('tool_view'),   shortcut: 'Q' },
    { key: 'move',   icon: <IconMove size={16} />,   label: t('tool_move'),   shortcut: 'W' },
    { key: 'rotate', icon: <IconRotate size={16} />,  label: t('tool_rotate'), shortcut: 'E' },
    { key: 'scale',  icon: <IconScale size={16} />,  label: t('tool_scale'),  shortcut: 'R' },
  ];

  return (
    <div className="toolbar-extras">
      {/* Transform tools */}
      <div className="toolbar-group">
        {tools.map((tool) => (
          <button
            key={tool.key}
            className={`tool-btn tool-btn-icon ${activeTool === tool.key ? 'active' : ''}`}
            onClick={() => onToolChange(tool.key)}
            title={`${tool.label} (${tool.shortcut})`}
          >
            {tool.icon}
          </button>
        ))}
      </div>

      <div className="toolbar-sep" />

      {/* Transform space */}
      <div className="toolbar-group">
        <button
          className={`tool-btn tool-btn-sm ${space === 'global' ? 'active' : ''}`}
          onClick={() => onSpaceChange(space === 'global' ? 'local' : 'global')}
          title={t('tool_toggle_space')}
        >
          {space === 'global' ? t('tool_global') : t('tool_local')}
        </button>
      </div>

      <div className="toolbar-sep" />

      {/* Snap */}
      <div className="toolbar-group" style={{ position: 'relative' }}>
        <button
          className={`tool-btn tool-btn-sm ${showSnap ? 'active' : ''}`}
          onClick={() => setShowSnap(!showSnap)}
          title={t('tool_snap')}
        >
          <IconChevronDown size={10} /> Snap
        </button>
        {showSnap && (
          <div className="context-menu" style={{ position: 'absolute', top: '100%', left: 0, zIndex: 100, width: 180 }}>
            <div className="context-menu-item" style={{ display: 'flex', gap: 8, alignItems: 'center', padding: '4px 8px' }}>
              <span style={{ fontSize: 11, minWidth: 60 }}>Move</span>
              <select value={moveSnap} onChange={e => onMoveSnapChange(Number(e.target.value))} className="toolbar-select">
                <option value={0.1}>0.1</option>
                <option value={0.25}>0.25</option>
                <option value={0.5}>0.5</option>
                <option value={1}>1</option>
              </select>
            </div>
            <div className="context-menu-item" style={{ display: 'flex', gap: 8, alignItems: 'center', padding: '4px 8px' }}>
              <span style={{ fontSize: 11, minWidth: 60 }}>Angle</span>
              <select value={angleSnap} onChange={e => onAngleSnapChange(Number(e.target.value))} className="toolbar-select">
                <option value={5}>5°</option>
                <option value={15}>15°</option>
                <option value={45}>45°</option>
              </select>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// ─── Build menu structure from shell state ─────────────────────────────────

export interface EditorMenuActions {
  handleUndo: () => void;
  handleRedo: () => void;
  handleSaveScene: () => void;
  handleSaveSceneAs: () => void;
  handleOpenScene: () => void;
  handleCloseProject: () => void;
  handleCreateEmpty: () => void;
  handleCreateCamera: () => void;
  handleCreateLight: () => void;
  addComponent: (type: string) => void;
  handleImportAsset?: () => void;
  handleReimportAll?: () => void;
  handleProjectSettings?: () => void;
  handleToggleHierarchy?: () => void;
  handleToggleInspector?: () => void;
  handleToggleConsole?: () => void;
  handleToggleProject?: () => void;
  handleKeyboardShortcuts?: () => void;
  handleDocumentation?: () => void;
  handleReportIssue?: () => void;
  handleAbout?: () => void;
}

export interface EditorPanelStates {
  leftCollapsed?: boolean;
  rightCollapsed?: boolean;
  bottomCollapsed?: boolean;
}

export function buildEditorMenus(
  t: (key: string) => string,
  shellState: { can_undo: boolean; can_redo: boolean; scene_dirty: boolean; has_project: boolean } | null,
  actions: EditorMenuActions,
  panelStates?: EditorPanelStates,
): MenuDef[] {
  const hp = !!shellState?.has_project;
  const dirty = !!shellState?.scene_dirty;

  return [
    {
      label: t('menu_file'),
      items: [
        { label: t('menu_open_scene'),   shortcut: 'Ctrl+O',  disabled: !hp, action: actions.handleOpenScene },
        { label: t('menu_save'),          shortcut: 'Ctrl+S',  disabled: !dirty, action: actions.handleSaveScene },
        { label: t('menu_save_as'),       shortcut: 'Ctrl+Shift+S', disabled: !hp, action: actions.handleSaveSceneAs },
        { divider: true },
        { label: t('menu_close_project'), shortcut: '',        disabled: !hp, action: actions.handleCloseProject },
      ],
    },
    {
      label: t('menu_edit'),
      items: [
        { label: t('menu_undo'), shortcut: 'Ctrl+Z', disabled: !shellState?.can_undo, action: actions.handleUndo },
        { label: t('menu_redo'), shortcut: 'Ctrl+Y', disabled: !shellState?.can_redo, action: actions.handleRedo },
      ],
    },
    {
      label: t('menu_gameobject'),
      items: [
        { label: t('menu_create_empty'),  shortcut: 'Ctrl+Shift+N', disabled: !hp, action: actions.handleCreateEmpty },
        { divider: true },
        { label: t('menu_create_camera'), shortcut: '', disabled: !hp, action: actions.handleCreateCamera },
        { label: t('menu_create_light'),  shortcut: '', disabled: !hp, action: actions.handleCreateLight },
      ],
    },
    {
      label: t('menu_component'),
      items: [
        ...['Camera', 'MeshRenderer', 'Light', 'Rigidbody', 'Collider', 'AudioSource', 'Script'].map((comp) => ({
          label: comp,
          disabled: !hp,
          action: () => actions.addComponent(comp),
        })),
      ],
    },
    {
      label: t('menu_assets'),
      items: [
        { label: t('menu_import_asset'), shortcut: '', disabled: !hp, action: actions.handleImportAsset },
        { label: t('menu_reimport_all'), shortcut: '', disabled: !hp, action: actions.handleReimportAll },
        { divider: true },
        { label: t('menu_project_settings'), shortcut: '', disabled: !hp, action: actions.handleProjectSettings },
      ],
    },
    {
      label: t('menu_window'),
      items: [
        { label: t('menu_toggle_hierarchy'), shortcut: '', disabled: !hp, action: actions.handleToggleHierarchy },
        { label: t('menu_toggle_inspector'), shortcut: '', disabled: !hp, action: actions.handleToggleInspector },
        { divider: true },
        { label: t('menu_toggle_console'), shortcut: '', disabled: !hp, action: actions.handleToggleConsole },
        { label: t('menu_toggle_project'), shortcut: '', disabled: !hp, action: actions.handleToggleProject },
      ],
    },
    {
      label: t('menu_help'),
      items: [
        { label: t('menu_about'), shortcut: '', action: actions.handleAbout },
        { divider: true },
        { label: t('menu_keyboard_shortcuts'), shortcut: 'Ctrl+Shift+K', action: actions.handleKeyboardShortcuts },
        { divider: true },
        { label: t('menu_documentation'), shortcut: '', action: actions.handleDocumentation },
        { label: t('menu_report_issue'), shortcut: '', action: actions.handleReportIssue },
      ],
    },
  ];
}
