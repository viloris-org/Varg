import React, { useCallback, useEffect, useRef, useState } from 'react';
import { rpc } from '../api';
import { useTranslation } from '../i18n';
import { IconFolder, IconFile, IconPlus, IconRefresh, IconEdit, IconCopy, IconTrash, assetIcon } from '../icons';
import { projectPanelIconButtonClass, projectPanelSearchInputClass } from '../uiClasses';

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
        className={`project-tree-item ${node.isDir ? 'project-tree-dir' : 'project-tree-file'}`}
        style={{ paddingLeft: 8 + depth * 16 }}
        onClick={() => { if (hasChildren) setExpanded(!expanded); }}
        onDoubleClick={() => {
          if (!node.isDir && onOpenScript) {
            const ext = node.name.split('.').pop()?.toLowerCase();
            if (ext === 'rhai' || ext === 'py') {
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
          <span className="project-tree-caret">{expanded ? '▼' : '▶'}</span>
        ) : (
          <span className="project-tree-icon">
            {meta ? assetIcon(meta.kind) : <IconFile />}
          </span>
        )}
        {isRenaming ? (
          <input
            className="project-tree-rename-input"
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
            <span className={`project-tree-name ${!matchesSearch && childrenMatch ? 'project-tree-dimmed' : ''}`}>
              {node.name}
            </span>
            {meta && (
              <span className="project-tree-kind">{meta.kind}</span>
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
            <div className="project-tree-empty" style={{ paddingLeft: 24 + depth * 16 }}>
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
    <div className="project-panel">
      {/* Toolbar */}
      <div className="project-toolbar">
        <div className="project-toolbar-left">
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
              <div className="context-menu" style={{ position: 'absolute', top: '100%', left: 0, zIndex: 100 }}>
                <div className="context-menu-item" style={{ display: 'flex', gap: 4, alignItems: 'center', padding: '4px 8px' }}>
                  <input
                    className="project-script-input"
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
                    className="project-script-select"
                    value={scriptBackend}
                    onChange={(e) => setScriptBackend(e.target.value as 'rhai' | 'python')}
                    onClick={(e) => e.stopPropagation()}
                  >
                    <option value="rhai">.rhai</option>
                    <option value="python">.py</option>
                  </select>
                </div>
              </div>
            )}
          </div>
        </div>
        {assets.length > 0 && (
          <span className="project-count">{assets.length}</span>
        )}
      </div>

      {/* Search */}
      <div className="project-search-row">
        <input
          className={projectPanelSearchInputClass}
          type="text"
          placeholder={t('project_search')}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

      {/* Tree */}
      <div className="project-scroll">
        {loading ? (
          <p className="panel-empty">{t('loading')}</p>
        ) : tree.length === 0 ? (
          <p className="panel-empty">{t('project_empty')}</p>
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
          className="context-menu"
          style={{ position: 'fixed', left: contextMenu.x, top: contextMenu.y, zIndex: 1000 }}
          onClick={(e) => e.stopPropagation()}
        >
          <button className="context-menu-item" onClick={() => handleRenameStart(contextMenu.asset)}>
            <IconEdit size={12} /> {t('action_rename')}
          </button>
          <button className="context-menu-item" onClick={() => handleCopyGuid(contextMenu.asset.guid)}>
            <IconCopy size={12} /> {t('action_copy_guid')}
          </button>
          <button className="context-menu-item" onClick={() => handleReimport(contextMenu.asset.source_path)}>
            <IconRefresh size={12} /> {t('action_reimport')}
          </button>
          <button className="context-menu-item" onClick={() => handleOpenInFileManager(contextMenu.asset.source_path)}>
            <IconFolder size={12} /> {t('action_show_in_fm')}
          </button>
          <div className="context-menu-sep" />
          <button
            className={`context-menu-item danger ${contextMenu.deleteConfirm ? 'confirming' : ''}`}
            onClick={() => handleDeleteAsset(contextMenu.asset.source_path)}
          >
            <IconTrash size={12} /> {contextMenu.deleteConfirm ? t('action_confirm_delete') : t('action_delete')}
          </button>
        </div>
      )}
    </div>
  );
}
