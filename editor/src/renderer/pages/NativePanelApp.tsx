import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { rpc } from '../api';
import { safeJsonStringify } from '../safeJson';

interface ShellState {
  has_project: boolean;
  project_name?: string;
  scene_dirty: boolean;
  can_undo: boolean;
  can_redo: boolean;
  scene_version?: number;
  selected_entity?: string | null;
}

interface SceneObject {
  id: string;
  name: string;
  tag: string;
  position: [number, number, number];
  parent_id?: string | null;
}

interface EntityDetails {
  id: string;
  name: string;
  tag: string;
  transform: {
    position: [number, number, number];
    rotation: [number, number, number, number];
    scale: [number, number, number];
  };
  components: Array<{
    type: string;
    data: Record<string, unknown>;
  }>;
}

type NativePanelKind = 'toolbar' | 'hierarchy' | 'inspector' | 'statusbar';

const COMPONENT_TYPES = [
  'Camera',
  'Light',
  'MeshRenderer',
  'Rigidbody',
  'Collider',
  'FluidVolume',
  'WindZone',
  'AudioSource',
  'AudioListener',
  'AcousticMaterial',
  'AcousticGeometry',
  'AcousticRoom',
  'AcousticPortal',
  'AudioZone',
  'Environment',
  'Skybox',
  'Script',
];

function formatVec3(value: [number, number, number]) {
  return value.map(item => item.toFixed(2)).join(', ');
}

function formatVec4(value: [number, number, number, number]) {
  return value.map(item => item.toFixed(3)).join(', ');
}

function parseNumber(value: string, fallback: number) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function formatInspectorValue(value: unknown): string {
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  if (Array.isArray(value)) return value.map(item => formatInspectorValue(item)).join(', ');
  if (value === null || value === undefined) return '';
  return safeJsonStringify(value, 2);
}

function parseInspectorValue(raw: string, current: unknown): unknown {
  if (typeof current === 'number') {
    const next = Number(raw);
    return Number.isFinite(next) ? next : current;
  }
  if (typeof current === 'boolean') return raw === 'true';
  if (Array.isArray(current)) {
    const parts = raw.split(',').map(part => part.trim());
    if (current.every(item => typeof item === 'number')) {
      const parsed = parts.map(Number);
      return parsed.length === current.length && parsed.every(Number.isFinite) ? parsed : current;
    }
    return parts;
  }
  if (current && typeof current === 'object') {
    try {
      return JSON.parse(raw);
    } catch {
      return current;
    }
  }
  return raw;
}

function componentFieldOptions(componentType: string, fieldName: string): string[] | null {
  if (componentType === 'Light' && fieldName === 'kind') return ['directional', 'point', 'spot'];
  if (componentType === 'Environment' && fieldName === 'tonemap') return ['aces'];
  if (componentType === 'Rigidbody' && fieldName === 'body_type') return ['dynamic', 'kinematic', 'static'];
  if (componentType === 'Collider' && fieldName === 'shape') return ['box', 'sphere', 'capsule'];
  if (componentType === 'AudioSource' && fieldName === 'spatial_mode') return ['direct', 'spatial'];
  if (componentType === 'AudioSource' && fieldName === 'shape') return ['point', 'cone'];
  if (componentType === 'AudioSource' && fieldName === 'attenuation') return ['none', 'linear', 'inverse'];
  if (componentType === 'AudioListener' && fieldName === 'output_mode') return ['stereo', 'surround'];
  if (componentType === 'AudioListener' && fieldName === 'hrtf_quality') return ['low', 'medium', 'high'];
  return null;
}

function formatFieldLabel(fieldName: string): string {
  return fieldName.replaceAll('_', ' ');
}

function useNativePanelData() {
  const [shell, setShell] = useState<ShellState | null>(null);
  const [objects, setObjects] = useState<SceneObject[]>([]);
  const [entity, setEntity] = useState<EntityDetails | null>(null);

  const refresh = useCallback(async () => {
    const state = await rpc<ShellState>('shell/get_state');
    setShell(state);
    if (!state.has_project) {
      setObjects([]);
      setEntity(null);
      return;
    }
    const tree = await rpc<{ objects: SceneObject[] }>('shell/get_scene_tree');
    setObjects(tree.objects);
    if (state.selected_entity) {
      const detail = await rpc<EntityDetails>('shell/get_entity', { id: state.selected_entity }).catch(() => null);
      setEntity(detail);
    } else {
      setEntity(null);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    const poll = () => {
      refresh().catch(() => {
        if (!cancelled) {
          setShell(null);
          setObjects([]);
          setEntity(null);
        }
      });
    };
    poll();
    const interval = window.setInterval(poll, 600);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [refresh]);

  return { shell, objects, entity, refresh };
}

function ToolbarPanel({ shell, refresh }: { shell: ShellState | null; refresh: () => Promise<void> }) {
  return (
    <div className="native-panel-bar native-panel-toolbar">
      <strong>{shell?.project_name ?? 'Varg'}</strong>
      <span>{shell?.scene_dirty ? 'dirty' : 'saved'}</span>
      <button type="button" disabled={!shell?.scene_dirty} onClick={() => rpc('shell/save_scene').then(refresh)}>Save</button>
      <button type="button" disabled={!shell?.can_undo} onClick={() => rpc('shell/undo').then(refresh)}>Undo</button>
      <button type="button" disabled={!shell?.can_redo} onClick={() => rpc('shell/redo').then(refresh)}>Redo</button>
      <button type="button" onClick={() => rpc('play/start').then(refresh).catch(() => {})}>Run</button>
    </div>
  );
}

function HierarchyPanel({ shell, objects, refresh }: {
  shell: ShellState | null;
  objects: SceneObject[];
  refresh: () => Promise<void>;
}) {
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState('');

  const rows = useMemo(() => {
    const children = new Map<string | null, SceneObject[]>();
    for (const object of objects) {
      const key = object.parent_id ?? null;
      children.set(key, [...(children.get(key) ?? []), object]);
    }
    const flattened: Array<{ object: SceneObject; depth: number; hasChildren: boolean }> = [];
    const walk = (parent: string | null, depth: number) => {
      for (const object of children.get(parent) ?? []) {
        const hasChildren = children.has(object.id);
        flattened.push({ object, depth, hasChildren });
        if (hasChildren && !collapsed.has(object.id)) walk(object.id, depth + 1);
      }
    };
    walk(null, 0);
    return flattened;
  }, [collapsed, objects]);

  const select = async (id: string) => {
    await rpc('shell/select_entity', { id });
    await refresh();
  };

  const createObject = async () => {
    const created = await rpc<SceneObject>('shell/create_object', {
      name: `Object ${objects.length + 1}`,
      tag: 'Entity',
      parent_id: shell?.selected_entity ?? undefined,
    });
    await rpc('shell/select_entity', { id: created.id });
    await refresh();
  };

  const deleteSelected = async () => {
    if (!shell?.selected_entity) return;
    await rpc('shell/delete_object', { id: shell.selected_entity });
    await refresh();
  };

  const duplicateSelected = async () => {
    if (!shell?.selected_entity) return;
    const duplicated = await rpc<SceneObject>('shell/duplicate_object', { id: shell.selected_entity });
    await rpc('shell/select_entity', { id: duplicated.id });
    await refresh();
  };

  const commitRename = async (object: SceneObject) => {
    const name = renameDraft.trim();
    setRenaming(null);
    if (!name || name === object.name) return;
    await rpc('shell/rename_object', { id: object.id, name });
    await refresh();
  };

  return (
    <div className="native-panel-stack">
      <header className="native-panel-header">
        <span>Hierarchy <span>{objects.length}</span></span>
        <span className="native-panel-actions">
          <button type="button" onClick={createObject}>+</button>
          <button type="button" disabled={!shell?.selected_entity} onClick={duplicateSelected}>D</button>
          <button type="button" disabled={!shell?.selected_entity} onClick={deleteSelected}>-</button>
        </span>
      </header>
      <div className="native-panel-list">
        {rows.map(({ object, depth, hasChildren }) => (
          <div
            key={object.id}
            className={`native-panel-tree-row ${object.id === shell?.selected_entity ? 'selected' : ''}`}
            style={{ paddingLeft: 6 + depth * 14 }}
            onClick={() => select(object.id)}
          >
            <button
              type="button"
              className="native-panel-twisty"
              disabled={!hasChildren}
              onClick={event => {
                event.stopPropagation();
                setCollapsed(current => {
                  const next = new Set(current);
                  if (next.has(object.id)) next.delete(object.id);
                  else next.add(object.id);
                  return next;
                });
              }}
            >
              {hasChildren ? collapsed.has(object.id) ? '>' : 'v' : ''}
            </button>
            {renaming === object.id ? (
              <input
                className="native-panel-inline-input"
                value={renameDraft}
                autoFocus
                onChange={event => setRenameDraft(event.currentTarget.value)}
                onBlur={() => commitRename(object)}
                onKeyDown={event => {
                  if (event.key === 'Enter') event.currentTarget.blur();
                  if (event.key === 'Escape') setRenaming(null);
                }}
                onClick={event => event.stopPropagation()}
              />
            ) : (
              <button
                type="button"
                className="native-panel-tree-name"
                onDoubleClick={event => {
                  event.stopPropagation();
                  setRenaming(object.id);
                  setRenameDraft(object.name);
                }}
              >
                <span>{object.name}</span>
                {object.tag && <small>{object.tag}</small>}
              </button>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function InspectorPanel({ entity, refresh }: { entity: EntityDetails | null; refresh: () => Promise<void> }) {
  const [name, setName] = useState('');
  const [position, setPosition] = useState<[string, string, string]>(['0', '0', '0']);
  const [rotation, setRotation] = useState<[string, string, string, string]>(['0', '0', '0', '1']);
  const [scale, setScale] = useState<[string, string, string]>(['1', '1', '1']);
  const [addComponentType, setAddComponentType] = useState(COMPONENT_TYPES[0]);

  useEffect(() => {
    setName(entity?.name ?? '');
    setPosition((entity?.transform.position ?? [0, 0, 0]).map(value => value.toFixed(2)) as [string, string, string]);
    setRotation((entity?.transform.rotation ?? [0, 0, 0, 1]).map(value => value.toFixed(3)) as [string, string, string, string]);
    setScale((entity?.transform.scale ?? [1, 1, 1]).map(value => value.toFixed(2)) as [string, string, string]);
  }, [entity?.id, entity?.name, entity?.transform.position, entity?.transform.rotation, entity?.transform.scale]);

  const commitName = async () => {
    if (!entity || !name.trim() || name.trim() === entity.name) return;
    await rpc('shell/rename_object', { id: entity.id, name: name.trim() });
    await refresh();
  };

  const commitVec3 = async (field: 'position' | 'scale', values: [string, string, string]) => {
    if (!entity) return;
    const current = entity.transform[field];
    const next = values.map((value, index) => parseNumber(value, current[index])) as [number, number, number];
    await rpc('shell/update_transform', { id: entity.id, [field]: next });
    await refresh();
  };

  const commitRotation = async () => {
    if (!entity) return;
    const current = entity.transform.rotation;
    const next = rotation.map((value, index) => parseNumber(value, current[index])) as [number, number, number, number];
    await rpc('shell/update_transform', { id: entity.id, rotation: next });
    await refresh();
  };

  const addComponent = async () => {
    if (!entity) return;
    await rpc('shell/add_component', { id: entity.id, component_type: addComponentType });
    await refresh();
  };

  const removeComponent = async (componentType: string) => {
    if (!entity) return;
    await rpc('shell/remove_component', { id: entity.id, component_type: componentType });
    await refresh();
  };

  const updateComponentField = async (componentType: string, fieldName: string, value: unknown) => {
    if (!entity) return;
    await rpc('shell/update_component', {
      id: entity.id,
      component_type: componentType,
      data: { [fieldName]: value },
    });
    await refresh();
  };

  if (!entity) {
    return <div className="native-panel-empty">No selection</div>;
  }

  return (
    <div className="native-panel-stack">
      <header className="native-panel-header">Inspector</header>
      <label className="native-panel-field">
        <span>Name</span>
        <input value={name} onChange={event => setName(event.currentTarget.value)} onBlur={commitName} />
      </label>
      <section className="native-panel-section">
        <strong>Transform</strong>
        <span className="native-panel-vector-label">Position {formatVec3(entity.transform.position)}</span>
        <div className="native-panel-vector">
          {position.map((value, index) => (
            <input
              key={`position-${index}`}
              value={value}
              inputMode="decimal"
              onChange={event => setPosition(current => {
                const next = [...current] as [string, string, string];
                next[index] = event.currentTarget.value;
                return next;
              })}
              onBlur={() => commitVec3('position', position)}
            />
          ))}
        </div>
        <span className="native-panel-vector-label">Rotation {formatVec4(entity.transform.rotation)}</span>
        <div className="native-panel-vector native-panel-vector4">
          {rotation.map((value, index) => (
            <input
              key={`rotation-${index}`}
              value={value}
              inputMode="decimal"
              onChange={event => setRotation(current => {
                const next = [...current] as [string, string, string, string];
                next[index] = event.currentTarget.value;
                return next;
              })}
              onBlur={commitRotation}
            />
          ))}
        </div>
        <span className="native-panel-vector-label">Scale {formatVec3(entity.transform.scale)}</span>
        <div className="native-panel-vector">
          {scale.map((value, index) => (
            <input
              key={`scale-${index}`}
              value={value}
              inputMode="decimal"
              onChange={event => setScale(current => {
                const next = [...current] as [string, string, string];
                next[index] = event.currentTarget.value;
                return next;
              })}
              onBlur={() => commitVec3('scale', scale)}
            />
          ))}
        </div>
      </section>
      <section className="native-panel-section">
        <strong>Components</strong>
        {entity.components.map(component => (
          <div className="native-panel-component" key={component.type}>
            <div className="native-panel-component-title">
              <span>{component.type}</span>
              <button type="button" onClick={() => removeComponent(component.type)}>-</button>
            </div>
            {Object.entries(component.data ?? {}).length === 0 ? (
              <small>No editable fields</small>
            ) : Object.entries(component.data ?? {}).map(([fieldName, value]) => (
              <ComponentField
                key={`${component.type}-${fieldName}`}
                componentType={component.type}
                fieldName={fieldName}
                value={value}
                onCommit={updateComponentField}
              />
            ))}
          </div>
        ))}
        <div className="native-panel-add-row">
          <select value={addComponentType} onChange={event => setAddComponentType(event.currentTarget.value)}>
            {COMPONENT_TYPES.map(type => <option key={type} value={type}>{type}</option>)}
          </select>
          <button type="button" onClick={addComponent}>Add</button>
        </div>
      </section>
    </div>
  );
}

function ComponentField({ componentType, fieldName, value, onCommit }: {
  componentType: string;
  fieldName: string;
  value: unknown;
  onCommit: (componentType: string, fieldName: string, value: unknown) => Promise<void>;
}) {
  const [draft, setDraft] = useState(formatInspectorValue(value));
  const options = componentFieldOptions(componentType, fieldName);

  useEffect(() => {
    setDraft(formatInspectorValue(value));
  }, [value]);

  const commit = () => onCommit(componentType, fieldName, parseInspectorValue(draft, value));

  if (typeof value === 'boolean') {
    return (
      <label className="native-panel-component-field">
        <span>{formatFieldLabel(fieldName)}</span>
        <input
          type="checkbox"
          checked={value}
          onChange={event => onCommit(componentType, fieldName, event.currentTarget.checked)}
        />
      </label>
    );
  }

  if (options) {
    return (
      <label className="native-panel-component-field">
        <span>{formatFieldLabel(fieldName)}</span>
        <select value={draft} onChange={event => {
          setDraft(event.currentTarget.value);
          onCommit(componentType, fieldName, event.currentTarget.value);
        }}>
          {options.map(option => <option key={option} value={option}>{option}</option>)}
        </select>
      </label>
    );
  }

  return (
    <label className="native-panel-component-field">
      <span>{formatFieldLabel(fieldName)}</span>
      <input
        value={draft}
        onChange={event => setDraft(event.currentTarget.value)}
        onBlur={commit}
        onKeyDown={event => {
          if (event.key === 'Enter') event.currentTarget.blur();
          if (event.key === 'Escape') setDraft(formatInspectorValue(value));
        }}
      />
    </label>
  );
}

function StatusbarPanel({ shell, entity }: { shell: ShellState | null; entity: EntityDetails | null }) {
  return (
    <div className="native-panel-bar native-panel-status">
      <span>{shell?.has_project ? shell.project_name : 'No project'}</span>
      <span>{shell?.scene_dirty ? 'Unsaved changes' : 'Saved'}</span>
      {entity && <strong>{entity.name}</strong>}
    </div>
  );
}

export default function NativePanelApp({ panel }: { panel: NativePanelKind }) {
  const { shell, objects, entity, refresh } = useNativePanelData();

  if (panel === 'toolbar') return <ToolbarPanel shell={shell} refresh={refresh} />;
  if (panel === 'hierarchy') return <HierarchyPanel shell={shell} objects={objects} refresh={refresh} />;
  if (panel === 'inspector') return <InspectorPanel entity={entity} refresh={refresh} />;
  if (panel === 'statusbar') return <StatusbarPanel shell={shell} entity={entity} />;
  return <div className="native-panel-empty">Unknown panel</div>;
}
