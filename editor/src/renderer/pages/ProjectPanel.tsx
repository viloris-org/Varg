import React, { useCallback, useEffect, useRef, useState } from 'react';
import { rpc } from '../api';
import { useTranslation } from '../i18n';
import { IconFolder, IconFile, IconPlus, IconRefresh, IconEdit, IconCopy, IconTrash, assetIcon } from '../icons';
import {
  contextMenuClass,
  contextMenuDangerItemClass,
  contextMenuItemClass,
  contextMenuSeparatorClass,
  projectPanelIconButtonClass,
  projectPanelSearchInputClass,
  projectTreeRenameInputClass,
} from '../uiClasses';

// ─── Types ──────────────────────────────────────────────────────────────────

interface AssetEntry {
  guid: string;
  path: string;
  kind: string;
}

interface AssetMeta {
  guid: string;
  source_path: string;
  kind: string;
  importer: string;
}

interface ContextMenuState {
  x: number;
  y: number;
  asset: AssetMeta;
  deleteConfirm?: boolean;
}

interface ProjectAssets {
  entries: AssetEntry[];
  assets: AssetMeta[];
}

interface TreeNode {
  name: string;
  path: string;
  isDir: boolean;
  children: TreeNode[];
  meta?: AssetMeta;
}

const projectPanelClass = 'flex h-full flex-col overflow-hidden text-xs';
const projectToolbarClass = 'flex items-center justify-between border-b border-[var(--border)] px-1.5 py-[3px]';
const projectToolbarLeftClass = 'flex items-center gap-0.5';
const projectCountClass = 'px-1 text-[10px] text-[var(--text-muted)]';
const projectSearchRowClass = 'border-b border-[var(--border)] px-1.5 py-1';
const projectScrollClass = 'flex-1 overflow-y-auto py-0.5 [&::-webkit-scrollbar]:w-1 [&::-webkit-scrollbar-thumb]:rounded-sm [&::-webkit-scrollbar-thumb]:bg-[var(--border)]';
const projectTreeItemClass = 'flex cursor-pointer items-center gap-1 overflow-hidden text-ellipsis whitespace-nowrap px-2 py-[3px] transition-[background] duration-[var(--transition-fast)] hover:bg-[var(--bg-hover)]';
const projectTreeCaretClass = 'w-3 flex-shrink-0 text-center text-[10px] text-[var(--text-muted)]';
const projectTreeIconClass = 'flex h-3.5 w-3.5 flex-shrink-0 items-center justify-center text-[var(--text-muted)]';
const projectTreeNameClass = 'min-w-0 flex-1 overflow-hidden text-ellipsis text-xs text-[var(--text-primary)]';
const projectTreeDimmedClass = 'opacity-40';
const projectTreeKindClass = 'flex-shrink-0 px-1 text-[9px] text-[var(--text-muted)]';
const projectTreeEmptyClass = 'px-2 py-1.5 text-[11px] italic text-[var(--text-muted)]';
const projectScriptInputClass = 'min-w-[100px] flex-1 rounded-[2px] border border-[var(--accent)] bg-[var(--bg-base)] px-1.5 py-[3px] font-[var(--font-sans)] text-[11px] text-[var(--text-primary)] outline-none';
const projectScriptSelectClass = 'cursor-pointer rounded-[2px] border border-[var(--border)] bg-[var(--bg-base)] px-1 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--text-primary)] outline-none';
const projectPanelEmptyClass = 'p-4 text-center text-xs italic text-[var(--text-secondary)]';

// ─── Build tree from flat asset list ────────────────────────────────────────

function buildTree(assets: AssetMeta[]): { tree: TreeNode[]; folderCount: number } {
  const root: TreeNode[] = [];
  const map = new Map<string, TreeNode>();

  // Sort by path to ensure parent dirs come before children
  const sorted = [...assets].sort((a, b) => a.source_path.localeCompare(b.source_path));

  let folderCount = 0;

  for (const meta of sorted) {
    const parts = meta.source_path.split('/');
    let current = root;
    let currentPath = '';

    for (let i = 0; i < parts.length; i++) {
      const part = parts[i];
      const isLast = i === parts.length - 1;
      const fullPath = currentPath ? `${currentPath}/${part}` : part;
      const key = isLast ? fullPath : `dir:${fullPath}`;

      let node = map.get(key);
      if (!node) {
        node = {
          name: part,
          path: fullPath,
          isDir: !isLast,
          children: [],
          meta: isLast ? meta : undefined,
        };
        map.set(key, node);
        current.push(node);
        if (!isLast) folderCount++;
      }
      current = node.children;
      currentPath = fullPath;
    }
  }

  return { tree: root, folderCount };
}

// ─── Tree Node Component (with context menu and rename) ──────────────────────

function TreeNodeItem({
  node,
  depth,
  search,
  onContextMenu,
  onRename,
  renaming,
  renameText,
  setRenameText,
  onRenameSubmit,
  onRenameCancel,
  onOpenScript,
}: {
  node: TreeNode;
  depth: number;
  search: string;
  onContextMenu?: (e: React.MouseEvent, asset: AssetMeta) => void;
  onRename?: (asset: AssetMeta) => void;
  renaming?: string | null;
  renameText?: string;
  setRenameText?: (v: string) => void;
  onRenameSubmit?: () => void;
  onRenameCancel?: () => void;
  onOpenScript?: (path: string, language: 'rhai' | 'python') => void;
}) {
  const [expanded, setExpanded] = useState(true);
  const hasChildren = node.children.length > 0;

  const matchesSearch = search === '' || node.name.toLowerCase().includes(search.toLowerCase());
  const childrenMatch = search !== '' && node.children.some(
    c => c.name.toLowerCase().includes(search.toLowerCase())
  );

  if (search && !matchesSearch && !childrenMatch && !node.isDir) return null;
  if (search && !matchesSearch && !childrenMatch && node.isDir && !hasChildren) return null;

  const isRenaming = renaming === node.path;
  const meta = node.meta;

  return (
    <>
      <div
        className={projectTreeItemClass}
        style={{ paddingLeft: 8 + depth * 16 }}
        onClick={() => { if (hasChildren) setExpanded(!expanded); }}
        onDoubleClick={() => {
          if (!node.isDir && onOpenScript) {
            const ext = node.name.split('.').pop()?.toLowerCase();
            if (ext === 'aster' || ext === 'rhai' || ext === 'py') {
              onOpenScript(node.path, ext === 'py' ? 'python' : 'rhai');
            }
          }
        }}
        onContextMenu={(e) => {
          if (meta && onContextMenu) {
            e.preventDefault();
            onContextMenu(e, meta);
          }
        }}
        title={node.path}
      >
        {node.isDir ? (
          <span className={projectTreeCaretClass}>{expanded ? '▼' : '▶'}</span>
        ) : (
          <span className={projectTreeIconClass}>
            {meta ? assetIcon(meta.kind) : <IconFile />}
          </span>
        )}
        {isRenaming ? (
          <input
            className={projectTreeRenameInputClass}
            value={renameText || ''}
            onChange={(e) => setRenameText?.(e.target.value)}
            onBlur={onRenameSubmit}
            onKeyDown={(e) => {
              if (e.key === 'Enter') onRenameSubmit?.();
              if (e.key === 'Escape') onRenameCancel?.();
              e.stopPropagation();
            }}
            autoFocus
            onClick={(e) => e.stopPropagation()}
          />
        ) : (
          <>
            <span className={`${projectTreeNameClass} ${!matchesSearch && childrenMatch ? projectTreeDimmedClass : ''}`}>
              {node.name}
            </span>
            {meta && (
              <span className={projectTreeKindClass}>{meta.kind}</span>
            )}
          </>
        )}
      </div>
      {node.isDir && expanded && hasChildren && (
        <>
          {node.children.map((child, i) => (
            <TreeNodeItem
              key={child.path + i} node={child} depth={depth + 1} search={search}
              onContextMenu={onContextMenu}
              onRename={onRename}
              renaming={renaming} renameText={renameText}
              setRenameText={setRenameText} onRenameSubmit={onRenameSubmit} onRenameCancel={onRenameCancel}
              onOpenScript={onOpenScript}
            />
          ))}
          {search && node.children.length === 0 && (
            <div className={projectTreeEmptyClass} style={{ paddingLeft: 24 + depth * 16 }}>
              {node.name + '/'}
            </div>
          )}
        </>
      )}
    </>
  );
}

// ─── Project Panel ──────────────────────────────────────────────────────────

interface ProjectPanelProps {
  onOpenScript?: (path: string, language: 'rhai' | 'python') => void;
}

export default function ProjectPanel({ onOpenScript }: ProjectPanelProps = {}) {
  const { t } = useTranslation();
  const [assets, setAssets] = useState<AssetMeta[]>([]);
  const [tree, setTree] = useState<TreeNode[]>([]);
  const [search, setSearch] = useState('');
  const [loading, setLoading] = useState(true);
  const [createMenuOpen, setCreateMenuOpen] = useState(false);
  const [scriptName, setScriptName] = useState('');
  const [scriptBackend, setScriptBackend] = useState<'rhai' | 'python'>('rhai');
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameText, setRenameText] = useState('');
  // delete confirm is now scoped inside contextMenu state

  const loadAssets = useCallback(async () => {
    setLoading(true);
    try {
      const data = await rpc<ProjectAssets>('project/list_assets');
      setAssets(data.assets);
      const { tree: t } = buildTree(data.assets);
      setTree(t);
    } catch {
      // no project open
    }
    setLoading(false);
  }, []);

  useEffect(() => { loadAssets(); }, [loadAssets]);

  // Close context menu on outside click
  useEffect(() => {
    if (!contextMenu) return;
    const handler = () => setContextMenu(null);
    window.addEventListener('click', handler);
    return () => window.removeEventListener('click', handler);
  }, [contextMenu]);

  const handleCreateScript = useCallback(async () => {
    if (!scriptName.trim()) return;
    try {
      await rpc('project/create_script', {
        name: scriptName.trim(),
        backend: scriptBackend,
      });
      setScriptName('');
      setCreateMenuOpen(false);
      await loadAssets();
    } catch (err) {
      console.error('Failed to create script:', err);
    }
  }, [scriptName, scriptBackend, loadAssets]);

  // ── Context menu handlers ──

  const handleAssetContextMenu = useCallback((e: React.MouseEvent, asset: AssetMeta) => {
    setContextMenu({ x: e.clientX, y: e.clientY, asset, deleteConfirm: false });
  }, []);

  const handleDeleteAsset = useCallback(async (path: string) => {
    setContextMenu((prev) => {
      if (!prev) return prev;
      if (prev.asset.source_path !== path) return prev; // safety: only act on the scoped asset
      if (prev.deleteConfirm) {
        // Actually delete
        rpc('project/delete_asset', { path })
          .then(() => loadAssets())
          .catch((err) => console.error('Failed to delete asset:', err));
        return null; // close context menu
      }
      // First click: ask for confirmation
      return { ...prev, deleteConfirm: true };
    });
  }, [loadAssets]);

  const handleRenameStart = useCallback((asset: AssetMeta) => {
    setRenaming(asset.source_path);
    // Extract filename without extension
    const name = asset.source_path.split('/').pop() || asset.source_path;
    const dotIdx = name.lastIndexOf('.');
    setRenameText(dotIdx > 0 ? name.substring(0, dotIdx) : name);
    setContextMenu(null);
  }, []);

  const handleRenameSubmit = useCallback(async () => {
    if (!renaming || !renameText.trim()) {
      setRenaming(null);
      return;
    }
    try {
      await rpc('project/rename_asset', { old_path: renaming, new_name: renameText.trim() });
      setRenaming(null);
      await loadAssets();
    } catch (err) {
      console.error('Failed to rename asset:', err);
      setRenaming(null);
    }
  }, [renaming, renameText, loadAssets]);

  const handleRenameCancel = useCallback(() => {
    setRenaming(null);
  }, []);

  const handleCopyGuid = useCallback((guid: string) => {
    navigator.clipboard.writeText(guid).catch(console.error);
    setContextMenu(null);
  }, []);

  const handleOpenInFileManager = useCallback(async (path: string) => {
    try {
      await rpc('app/open_folder', { path });
    } catch {
      // not supported
    }
    setContextMenu(null);
  }, []);

  const handleReimport = useCallback(async (path: string) => {
    try {
      await rpc('project/reimport_asset', { path });
      setContextMenu(null);
      await loadAssets();
    } catch (err) {
      console.error('Failed to reimport asset:', err);
    }
  }, [loadAssets]);

  return (
    <div className={projectPanelClass}>
      {/* Toolbar */}
      <div className={projectToolbarClass}>
        <div className={projectToolbarLeftClass}>
          <button
            className={projectPanelIconButtonClass}
            onClick={loadAssets}
            title={t('project_refresh')}
          >
            <IconRefresh />
          </button>
          <div style={{ position: 'relative' }}>
            <button
              className={projectPanelIconButtonClass}
              onClick={() => setCreateMenuOpen(!createMenuOpen)}
              title={t('project_create')}
            >
              <IconPlus />
            </button>
            {createMenuOpen && (
              <div className={`${contextMenuClass} absolute left-0 top-full z-[100]`}>
                <div className={`${contextMenuItemClass} gap-1 px-2 py-1`}>
                  <input
                    className={projectScriptInputClass}
                    type="text"
                    placeholder={t('project_script_name')}
                    value={scriptName}
                    onChange={(e) => setScriptName(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') handleCreateScript();
                      if (e.key === 'Escape') setCreateMenuOpen(false);
                      e.stopPropagation();
                    }}
                    autoFocus
                    onClick={(e) => e.stopPropagation()}
                  />
                  <select
                    className={projectScriptSelectClass}
                    value={scriptBackend}
                    onChange={(e) => setScriptBackend(e.target.value as 'rhai' | 'python')}
                    onClick={(e) => e.stopPropagation()}
                  >
                    <option value="rhai">.aster</option>
                    <option value="python">.py</option>
                  </select>
                </div>
              </div>
            )}
          </div>
        </div>
        {assets.length > 0 && (
          <span className={projectCountClass}>{assets.length}</span>
        )}
      </div>

      {/* Search */}
      <div className={projectSearchRowClass}>
        <input
          className={projectPanelSearchInputClass}
          type="text"
          placeholder={t('project_search')}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

      {/* Tree */}
      <div className={projectScrollClass}>
        {loading ? (
          <p className={projectPanelEmptyClass}>{t('loading')}</p>
        ) : tree.length === 0 ? (
          <p className={projectPanelEmptyClass}>{t('project_empty')}</p>
        ) : (
          tree.map((node, i) => (
            <TreeNodeItem
              key={node.path + i} node={node} depth={0} search={search}
              onContextMenu={handleAssetContextMenu}
              onRename={handleRenameStart}
              renaming={renaming} renameText={renameText}
              setRenameText={setRenameText}
              onRenameSubmit={handleRenameSubmit}
              onRenameCancel={handleRenameCancel}
              onOpenScript={onOpenScript}
            />
          ))
        )}
      </div>

      {/* Context Menu */}
      {contextMenu && (
        <div
          className={`${contextMenuClass} fixed z-[1000]`}
          style={{ position: 'fixed', left: contextMenu.x, top: contextMenu.y, zIndex: 1000 }}
          onClick={(e) => e.stopPropagation()}
        >
          <button className={contextMenuItemClass} onClick={() => handleRenameStart(contextMenu.asset)}>
            <IconEdit size={12} /> {t('action_rename')}
          </button>
          <button className={contextMenuItemClass} onClick={() => handleCopyGuid(contextMenu.asset.guid)}>
            <IconCopy size={12} /> {t('action_copy_guid')}
          </button>
          <button className={contextMenuItemClass} onClick={() => handleReimport(contextMenu.asset.source_path)}>
            <IconRefresh size={12} /> {t('action_reimport')}
          </button>
          <button className={contextMenuItemClass} onClick={() => handleOpenInFileManager(contextMenu.asset.source_path)}>
            <IconFolder size={12} /> {t('action_show_in_fm')}
          </button>
          <div className={contextMenuSeparatorClass} />
          <button
            className={contextMenuDangerItemClass}
            onClick={() => handleDeleteAsset(contextMenu.asset.source_path)}
          >
            <IconTrash size={12} /> {contextMenu.deleteConfirm ? t('action_confirm_delete') : t('action_delete')}
          </button>
        </div>
      )}
    </div>
  );
}
