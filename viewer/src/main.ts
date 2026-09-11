import { Display, Viewer, type Shape, type Shapes, type ChangeNotification } from 'three-cad-viewer';
import 'three-cad-viewer/css';
import './style.css';

type BufferRef = { resource: string; dtype: 'float32' | 'uint32'; count: number; bytes: number };
type Mesh = { id: string; buffers: Record<string, BufferRef>; bounds: Record<string, number>; face_count: number; triangle_count: number };
type Node = { id: string; parent: string | null; name: string; mesh_id: string; body_index: number; source_sha256: string; candidate_id: string };
type Scene = {
  schema_version: number; job_status: string; source: { name: string; kind: string; sha256: string | null };
  stages: Record<string, string>; nodes: Node[]; meshes: Mesh[];
  selection: { selected_id: string | null; status: string; basis: string | null } | null;
  candidates: { id: string; table_status: string; state_binding: string; source: unknown }[];
  properties: { name: string; value: unknown; storage_scope: string; state_binding: string; source: unknown; value_kind: string; fmtid: string; pid: number }[];
  thumbnails: { resource: string; width: number; height: number; source: { stream: string }; state_binding: string }[];
  diagnostics: { code: string; severity: string; message: string; source?: unknown }[];
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

function select(id: string | null) {
  const node = scene?.nodes.find(n => n.id === id);
  el('selected').textContent = node ? `${node.name} selected` : 'No body selected';
  el('selected').dataset.nodeId = node?.id ?? '';
  document.querySelectorAll<HTMLElement>('.body-row').forEach(row => row.classList.toggle('active', row.dataset.nodeId === id));
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
    const shown = visible.get(node.id) !== false;
    viewer?.setState(node.id, [shown ? 1 : 0, shown && el<HTMLInputElement>('edges').checked ? 1 : 0]);
  }
}

async function render(s: Scene) {
  const container = el('cad');
  const parts = await Promise.all(s.nodes.map(async node => {
    const mesh = s.meshes.find(m => m.id === node.mesh_id);
    if (!mesh) throw new Error('Body mesh is missing');
    // The renderer's tree uses names as path components; the application keeps
    // user-facing names separately so IDs survive display-name changes.
    return { version: 3, id: node.id, name: node.id.split('/').at(-1)!, type: 'shapes' as const, subtype: 'solid' as const,
      shape: await loadMesh(mesh), state: [1, 1] as [1, 1], color: '#86b5ad', alpha: 1,
      loc: [[0, 0, 0], [0, 0, 0, 1]] as [[number, number, number], [number, number, number, number]], renderback: false };
  }));
  if (stopped) return;
  const bound = (key: string, fn: (...args: number[]) => number) => fn(...s.meshes.map(m => m.bounds[key]));
  const bb = { xmin: bound('xmin', Math.min), xmax: bound('xmax', Math.max),
    ymin: bound('ymin', Math.min), ymax: bound('ymax', Math.max), zmin: bound('zmin', Math.min), zmax: bound('zmax', Math.max) };
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
  const shapes: Shapes = { version: 3, parts, id: '/document', name: 'document', normal_len: 0,
    loc: [[0, 0, 0], [0, 0, 0, 1]], bb };
  viewer.render(shapes, { ambientIntensity: 1, directIntensity: 1.2, metalness: 0.05, roughness: 0.8 }, { up: 'Z', ortho: true, axes: true, collapse: 1 });
  display.cadTree.hidden = true;
  display.cadInfo.hidden = true;
  display.cadTools.hidden = true;
  viewer.presetCamera('iso');
  el('empty').hidden = true;
  el('body-count').textContent = String(s.nodes.length);
  el('body-list').replaceChildren(...s.nodes.map(node => {
    visible.set(node.id, true);
    const row = text('div', '', 'body-row'); row.dataset.nodeId = node.id;
    const checkbox = document.createElement('input'); checkbox.type = 'checkbox'; checkbox.checked = true; checkbox.setAttribute('aria-label', `Show ${node.name}`);
    checkbox.addEventListener('change', () => { visible.set(node.id, checkbox.checked); setVisibility(); });
    const button = text('button', node.name) as HTMLButtonElement;
    button.addEventListener('click', () => { viewer?.handlePick('/document', node.id.split('/').at(-1)!, false, false, false, null, null, true); select(node.id); });
    row.append(checkbox, button); return row;
  }));
  document.querySelectorAll<HTMLButtonElement>('button[data-view]').forEach(button => { button.disabled = false; });
  el<HTMLInputElement>('edges').disabled = false;
  el('render-status').textContent = `${s.nodes.length} bodies · ${s.meshes.reduce((n, m) => n + m.triangle_count, 0).toLocaleString()} triangles`;
  container.dataset.rendered = 'true';
  container.dataset.triangles = String(s.meshes.reduce((n, m) => n + m.triangle_count, 0));
}

document.querySelectorAll<HTMLButtonElement>('button[data-view]').forEach(button => button.addEventListener('click', () => {
  const direction = button.dataset.view;
  if (direction === 'fit') { viewer?.centerVisibleObjects(); viewer?.resize(); }
  else viewer?.presetCamera(direction as 'iso' | 'front' | 'top');
}));
el('edges').addEventListener('change', setVisibility);
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
    if (scene.nodes.length) await render(scene);
    else {
      message('No 3D geometry to display', 'See Read results for the outcome. Available document information and saved previews are shown in the side panel.');
      el('render-status').textContent = scene.job_status === 'failed' ? 'Conversion process failed' : 'Document information';
    }
  } catch (error) {
    viewer?.dispose(); viewer = undefined;
    message('Viewer could not display the model', String(error));
    el('render-status').textContent = 'Display failed';
    document.body.dataset.displayError = String(error);
  }
}
void poll();
