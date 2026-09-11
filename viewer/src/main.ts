import { Display, Viewer, type Shape, type ChangeNotification } from 'three-cad-viewer';
import { renderTree, type Mesh, type Node } from './scene-graph';
import 'three-cad-viewer/css';
import './style.css';

type Scene = {
  schema_version: number; job_status: string; source: { name: string; kind: string; sha256: string | null };
  stages: Record<string, string>; nodes: Node[]; meshes: Mesh[];
  selection: { selected_id: string | null; status: string; basis: string | null } | null;
  candidates: { id: string; table_status: string; state_binding: string; source: unknown }[];
  properties: { name: string; value: unknown; storage_scope: string; state_binding: string; source: unknown; value_kind: string; fmtid: string; pid: number }[];
  thumbnails: { resource: string; width: number; height: number; source: { stream: string }; state_binding: string }[];
  diagnostics: { code: string; severity: string; message: string; source?: unknown }[];
  assembly: { structure_status: string; allow_unverified_state: boolean; allow_partial: boolean; displayed_instances: number; displayed_definitions: number; source_documents: unknown[] } | null;
  omissions: { instance: number; path: number[]; reason: string; detail: string }[];
  reference_issues: { status: string; detail?: string }[];
};
const el = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const text = (tag: string, value: unknown, className = '') => {
  const node = document.createElement(tag); node.textContent = String(value ?? '—'); node.className = className; return node;
};
const detail = (value: unknown) => {
  const node = document.createElement('details'); node.append(text('summary', 'Source details'), text('pre', JSON.stringify(value, null, 2))); return node;
};
let viewer: Viewer | undefined;
let display: Display | undefined;
let scene: Scene | undefined;
let stopped = false;
const visible = new Map<string, boolean>();
let renderedIds = new Set<string>();
let selectedId: string | null = null;
const descendants = (id: string) => (scene?.nodes ?? []).filter(n => n.mesh_id && (n.id === id || n.id.startsWith(id + '/')));
const statusText = (status: string) => ({ displayable: 'Geometry available', group: 'Assembly', pending: 'Preparing geometry', state_unverified: 'Permission required' }[status] ?? status.replaceAll('_', ' '));

function select(id: string | null) {
  const node = scene?.nodes.find(n => n.id === id);
  selectedId = node?.id ?? null;
  el('selected').textContent = node ? `${node.name} selected` : 'No component selected';
  el('selected').dataset.nodeId = node?.id ?? '';
  document.querySelectorAll<HTMLElement>('.body-row').forEach(row => row.classList.toggle('active', row.dataset.nodeId === id));
  el<HTMLButtonElement>('isolate').disabled = !node || !descendants(node.id).length;
  el('selection-info').replaceChildren(...(node ? [text('h2', node.name), text('p', node.reason ?? statusText(node.status)), detail(node)] : [text('p', 'Select a component to inspect its source and saved placement.', 'hint')]));
}

function information(s: Scene) {
  el('file-name').textContent = s.source.name;
  document.title = `${s.source.name} · Inventor Kit`;
  const info = el('document-info'); info.replaceChildren();
  for (const [key, value] of [['Name', s.source.name], ['Type', s.source.kind], ['Units', 'mm (geometry)'], ['State', 'Unverified'], ['SHA-256', s.source.sha256]]) {
    info.append(text('dt', key), text('dd', value));
  }
  el('property-count').textContent = String(s.properties.length);
  el('properties').replaceChildren(...s.properties.map(p => {
    const row = text('div', '', 'property');
    row.append(text('span', p.name), text('p', typeof p.value === 'object' ? JSON.stringify(p.value) : p.value), detail({ scope: p.storage_scope, state: p.state_binding, type: p.value_kind, fmtid: p.fmtid, pid: p.pid, source: p.source })); return row;
  }));
  el('previews').replaceChildren(...s.thumbnails.map((p, index) => {
    const figure = document.createElement('figure'); figure.className = 'preview';
    const image = document.createElement('img'); image.src = p.resource; image.alt = `Saved preview ${index + 1}`; image.width = p.width; image.height = p.height;
    figure.append(image, text('figcaption', `Saved preview ${index + 1} · ${p.source.stream}`)); return figure;
  }));
  if (!s.thumbnails.length) el('previews').append(text('p', 'No supported saved preview.', 'hint'));
  el('candidates').replaceChildren(...s.candidates.map(c => {
    const item = text('div', '', 'candidate'); item.append(text('span', `${c.id === s.selection?.selected_id ? 'Selected · ' : ''}${c.table_status} · ${c.state_binding}`), text('code', c.id), detail(c.source)); return item;
  }));
  if (s.selection) el('candidates').append(text('p', `Selection: ${s.selection.status}`, 'hint'));
  if (!s.candidates.length) el('candidates').append(text('p', 'No geometry candidates inventoried.', 'hint'));
  el('stages').replaceChildren(...Object.entries(s.stages).map(([name, value]) => {
    const row = text('div', '', 'stage'); row.append(text('span', name), text('span', value)); return row;
  }));
  el('diagnostics').replaceChildren(...s.diagnostics.map(d => {
    const row = text('div', '', 'diagnostic'); row.append(text('div', d.message), detail(d)); return row;
  }));
  el('assembly-section').hidden = s.assembly === null;
  if (s.assembly) {
    const a = s.assembly;
    el('assembly-info').replaceChildren(text('p', `${a.displayed_instances} displayed parts · ${a.displayed_definitions} shared definitions`),
      text('p', `Structure: ${a.structure_status}. Partial display: ${a.allow_partial ? 'allowed' : 'not allowed'}.`, 'hint'), detail(a));
    if (!a.allow_unverified_state) el('assembly-info').prepend(text('p', 'To display saved placements, restart with --allow-unverified-state.'));
    el('omissions').replaceChildren(...s.omissions.map(o => {
      const row = text('div', '', 'diagnostic'); row.append(text('strong', `Occurrence ${o.path.join(' / ')} · ${o.reason.replaceAll('_', ' ')}`), text('p', o.detail)); return row;
    }));
    el('reference-issues').replaceChildren(...s.reference_issues.map(issue => {
      const row = text('div', '', 'diagnostic'); row.append(text('p', issue.status.replaceAll('_', ' ')), detail(issue)); return row;
    }));
  }
}

async function loadMesh(mesh: Mesh): Promise<Shape> {
  const values: Record<string, Float32Array | Uint32Array> = {};
  await Promise.all(Object.entries(mesh.buffers).map(async ([key, ref]) => {
    const response = await fetch(ref.resource);
    if (!response.ok) throw new Error(`Mesh resource unavailable: ${response.status}`);
    const buffer = await response.arrayBuffer();
    if (buffer.byteLength !== ref.bytes || ref.bytes !== ref.count * 4) throw new Error('Mesh buffer size mismatch');
    values[key] = ref.dtype === 'uint32' ? new Uint32Array(buffer) : new Float32Array(buffer);
  }));
  return values as unknown as Shape;
}

function setVisibility() {
  for (const node of scene?.nodes ?? []) {
    if (!node.mesh_id) continue;
    const shown = visible.get(node.id) !== false;
    viewer?.setState(node.id, [shown ? 1 : 0, shown && el<HTMLInputElement>('edges').checked ? 1 : 0]);
  }
  document.querySelectorAll<HTMLElement>('.body-row').forEach(row => {
    const members = descendants(row.dataset.nodeId!);
    const shown = members.filter(n => visible.get(n.id) !== false).length;
    const checkbox = row.querySelector('input')!;
    checkbox.checked = shown > 0; checkbox.indeterminate = shown > 0 && shown < members.length;
    row.dataset.visible = String(shown > 0);
  });
}

function bodyTree(s: Scene) {
  el('tree-title').textContent = s.assembly ? 'Components' : 'Bodies';
  el('body-count').textContent = String(s.nodes.length);
  const children = new Map<string | null, Node[]>();
  for (const node of s.nodes) {
    const rows = children.get(node.parent) ?? []; rows.push(node); children.set(node.parent, rows);
    if (node.mesh_id) visible.set(node.id, true);
  }
  const branch = (parent: string | null, depth: number): HTMLUListElement => {
    if (depth > 64) throw new Error('Invalid occurrence hierarchy');
    const list = document.createElement('ul'); list.className = 'component-tree';
    for (const node of children.get(parent) ?? []) {
      const item = document.createElement('li');
      const row = text('div', '', 'body-row'); row.dataset.nodeId = node.id; row.dataset.status = node.status;
      const checkbox = document.createElement('input'); checkbox.type = 'checkbox'; checkbox.setAttribute('aria-label', `Show ${node.name}`);
      checkbox.disabled = !descendants(node.id).length;
      checkbox.addEventListener('change', () => { for (const n of descendants(node.id)) visible.set(n.id, checkbox.checked); setVisibility(); });
      const label = text('div', '', 'component-label');
      const button = text('button', node.name) as HTMLButtonElement;
      button.addEventListener('click', () => {
        viewer?.clearSelection();
        if (renderedIds.has(node.id)) viewer?.handlePick(node.parent ?? '/document', node.id.split('/').at(-1)!, false, false, false, null, null, true);
        select(node.id);
      });
      label.append(button);
      if (s.assembly) label.append(text('small', `${node.occurrence_path?.join(' / ')} · ${statusText(node.status)}`, 'component-status'));
      row.append(checkbox, label); row.title = node.reason ?? node.name;
      item.append(row);
      if (children.has(node.id)) item.append(branch(node.id, depth + 1));
      list.append(item);
    }
    return list;
  };
  el('body-list').replaceChildren(branch(null, 0));
  setVisibility();
  el<HTMLButtonElement>('show-all').disabled = !s.meshes.length;
}

async function render(s: Scene) {
  const container = el('cad');
  // Shared definitions have one buffer download and one decoded Shape object.
  const values = new Map(await Promise.all(s.meshes.map(async mesh => [mesh.id, await loadMesh(mesh)] as const)));
  const tree = renderTree(s.nodes, s.meshes, values);
  renderedIds = tree.renderedIds;
  if (stopped) return;
  const options = { cadWidth: container.clientWidth, height: container.clientHeight, treeWidth: 0,
    theme: 'light' as const, pinning: false, glass: false, tools: false, measureTools: false, selectTool: false,
    explodeTool: false, zscaleTool: false, zebraTool: false, studioTool: false, externalMeasurementBackend: false };
  display = new Display(container, options);
  viewer = new Viewer(display, options, (change: ChangeNotification) => {
    if (change.lastPick) {
      const pick = change.lastPick.new;
      select(pick ? `${pick.path}/${pick.name}` : null);
    }
  });
  viewer.render(tree.root, { ambientIntensity: 1, directIntensity: 1.2, metalness: 0.05, roughness: 0.8 }, { up: 'Z', ortho: true, axes: true, collapse: 1 });
  display.cadTree.hidden = true;
  display.cadInfo.hidden = true;
  display.cadTools.hidden = true;
  viewer.presetCamera('iso');
  el('empty').hidden = true;
  document.querySelectorAll<HTMLButtonElement>('button[data-view]').forEach(button => { button.disabled = false; });
  el<HTMLInputElement>('edges').disabled = false;
  el('render-status').textContent = `${tree.count} displayed ${s.assembly ? 'parts' : 'bodies'} · ${tree.triangles.toLocaleString()} triangles`;
  container.dataset.rendered = 'true';
  container.dataset.triangles = String(tree.triangles);
  container.dataset.displayed = String(tree.count);
}

document.querySelectorAll<HTMLButtonElement>('button[data-view]').forEach(button => button.addEventListener('click', () => {
  const direction = button.dataset.view;
  if (direction === 'fit') { viewer?.centerVisibleObjects(); viewer?.resize(); }
  else viewer?.presetCamera(direction as 'iso' | 'front' | 'top');
}));
el('edges').addEventListener('change', setVisibility);
el('isolate').addEventListener('click', () => {
  if (!selectedId) return;
  const keep = new Set(descendants(selectedId).map(n => n.id));
  for (const node of scene?.nodes ?? []) if (node.mesh_id) visible.set(node.id, keep.has(node.id));
  setVisibility();
});
el('show-all').addEventListener('click', () => { for (const node of scene?.nodes ?? []) if (node.mesh_id) visible.set(node.id, true); setVisibility(); });
const observer = new ResizeObserver(() => { if (viewer) viewer.resizeCadView(el('cad').clientWidth, 0, el('cad').clientHeight); });
observer.observe(el('cad'));
window.addEventListener('pagehide', () => { stopped = true; observer.disconnect(); viewer?.dispose(); display?.dispose(); });

function message(title: string, description: string) {
  el('empty').hidden = false;
  el('empty').querySelector('h2')!.textContent = title;
  el('empty').querySelector('p')!.textContent = description;
}

async function poll() {
  try {
    const response = await fetch('state.json');
    if (!response.ok) throw new Error(`Local server returned ${response.status}`);
    scene = await response.json() as Scene;
    if (scene.schema_version !== 1) throw new Error('Unsupported scene version');
    information(scene);
    if (scene.job_status === 'queued' || scene.job_status === 'running') {
      const stage = scene.stages.conversion === 'available' ? 'Preparing display meshes…' : 'Reading saved geometry…';
      message('Opening document', stage);
      if (!stopped) setTimeout(poll, 300);
      return;
    }
    document.body.dataset.jobStatus = scene.job_status;
    if (scene.meshes.length) await render(scene);
    else {
      message('No 3D geometry to display', 'See Read results for the outcome. Available document information and saved previews are shown in the side panel.');
      el('render-status').textContent = scene.job_status === 'failed' ? 'Conversion process failed' : 'Document information';
    }
    bodyTree(scene);
  } catch (error) {
    viewer?.dispose(); viewer = undefined;
    message('Viewer could not display the model', String(error));
    el('render-status').textContent = 'Display failed';
    document.body.dataset.displayError = String(error);
  }
}
void poll();
