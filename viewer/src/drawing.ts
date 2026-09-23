/** Partial stored IDW display in source units. No 3D renderer dependency. */
import { loadSheet, type SheetResource } from './drawing-data';
type Point = [number, number, number];
type Font = { family: string; height_candidate: number; weight_candidate: number; width_factor: number | null; flags: number };
type Geometry =
  | { kind: 'polyline'; points: Point[] }
  | { kind: 'curve'; center: Point; u: Point; v: Point; start: number; end: number }
  | { kind: 'text'; text: string; position: Point; direction: Point; up: Point; raw_flags: number; font: Font | null }
  | { kind: 'image'; reference: number; origin: Point; u: Point; v: Point };
type Item = { id: string; geometry: Geometry; style: { rgba: number[] | null; width: number | null; dash: number[] | null }; source: unknown };
type SavedView = { id: string; name: string; item_ids: string[]; image_reference: number | null };
type Sheet = { id: string; index: number; name: string; status: string; size_in_source_units: [number, number] | null;
  items: Item[]; views?: SavedView[]; omissions: unknown[]; diagnostics: string[]; sources: unknown[] };
type Descriptor = Omit<Sheet, 'items' | 'omissions' | 'sources'> & SheetResource & { item_count: number; omission_count: number };
export type DrawingScene = { status: string; sheet_status: string; units: string; qualified: false; complete: false;
  source_sha256: string; experimental: boolean; sheets: Descriptor[];
  images: { reference: number; resource: string | null; status: string; diagnostic: string | null }[] };

// Native diameter control stores AIGDT's legacy 'n' glyph. Preserve the raw
// API text; only this observed glyph gets an explicit Unicode display fallback.
function diameterFallback(g: Geometry): boolean {
  return g.kind === 'text' && g.text === 'n' && g.raw_flags === 9
    && g.font?.family.toLowerCase() === 'aigdt' && g.font.flags === 0 && g.font.weight_candidate === 400;
}
const displayedText = (g: Geometry): string => g.kind === 'text' ? (diameterFallback(g) ? '⌀' : g.text) : '';

const el = (id: string) => document.getElementById(id)!;
const NS = 'http://www.w3.org/2000/svg';
const label = (tag: string, value: string) => { const e = document.createElement(tag); e.textContent = value; return e; };
const details = (value: unknown) => { const d = document.createElement('details'); d.append(label('summary', 'Source details'), label('pre', JSON.stringify(value, null, 2))); return d; };

// A stored family is one literal name, not a CSS family list. Keep the native
// sans-serif families sans-serif when absent; never fetch external fonts.
function fontFamily(font: Font | null): string {
  if (!font) return 'sans-serif';
  const name = font.family.replace(/[\x00-\x1f\x7f"\\]/g, c => `\\${c.charCodeAt(0).toString(16)} `);
  const fallback = /^(arial|tahoma)$/i.test(font.family) ? ', sans-serif' : '';
  return `"${name}"${fallback}`;
}

export function showDrawing(data: DrawingScene): () => void {
  let stopped = false, sheet: Sheet | undefined, selected: string | null = null;
  let request = 0, controller: AbortController | undefined;
  const capHeightSupported = CSS.supports('font-size-adjust', 'cap-height 1');
  let view = [0, 0, 1, 1], drag: { x: number; y: number; origin: number[] } | null = null;
  const svg = document.createElementNS(NS, 'svg'); svg.id = 'drawing-svg'; svg.setAttribute('aria-label', 'Saved 2D drawing');
  const controls = document.createElement('div'); controls.id = 'drawing-controls';
  const fit = label('button', 'Fit sheet') as HTMLButtonElement; fit.id = 'drawing-fit';
  const zoom = label('button', 'Zoom in') as HTMLButtonElement; zoom.id = 'drawing-zoom';
  const texts = document.createElement('input'); texts.type = 'checkbox'; texts.checked = true; texts.id = 'drawing-text';
  const curves = document.createElement('input'); curves.type = 'checkbox'; curves.checked = true; curves.id = 'drawing-curves';
  const search = document.createElement('input'); search.type = 'search'; search.id = 'drawing-search'; search.placeholder = 'Search drawing text'; search.setAttribute('aria-label', 'Search drawing text');
  const textLabel = label('label', ' Text'); textLabel.prepend(texts);
  const curveLabel = label('label', ' Curves'); curveLabel.prepend(curves);
  controls.append(fit, zoom, textLabel, curveLabel, search);
  el('cad').replaceChildren(controls, svg);
  el('cad').dataset.mode = 'drawing';
  el('tree-title').textContent = 'Sheets'; el('body-count').textContent = String(data.sheets.length);
  el('body-list').replaceChildren();
  const tree = document.createElement('div'); tree.id = 'sheet-buttons'; el('body-list').append(tree);
  document.querySelector('.notice')!.textContent = 'Saved drawing · Partial display · Units and current state unverified';
  document.querySelector('.viewport')!.setAttribute('aria-label', '2D drawing');
  (document.querySelector('.viewport > nav') as HTMLElement).hidden = true;
  (document.querySelector('.tree-controls') as HTMLElement).hidden = true;
  document.querySelector('.viewport footer > span:last-child')!.textContent = 'Source units (unverified) · Pan / zoom';
  document.querySelector('.bodies > .hint')!.textContent = 'Select a stored sheet.';
  document.querySelector('.source-note')!.textContent = 'Saved drawing elements. Unsupported elements and unresolved styles remain in the diagnostics.';
  el('selected').textContent = 'No drawing element selected';
  el('selection-info').replaceChildren(label('p', 'Select an element or search for drawing text.'));
  const omissions = document.createElement('section'); omissions.id = 'drawing-omissions'; el('diagnostics').prepend(omissions);
  const results = document.createElement('div'); results.id = 'drawing-search-results'; el('body-list').append(results);
  const viewList = document.createElement('div'); viewList.id = 'drawing-views'; el('body-list').append(viewList);
  const sheetLabel = (s: { name: string; index: number }) => !s.name ? `Sheet ${s.index + 1}`
    : data.sheets.filter(other => other.name === s.name).length > 1 ? `${s.index + 1} · ${s.name}` : s.name;
  function node<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number>, parent: SVGElement = svg) {
    const e = document.createElementNS(NS, tag); for (const [key, value] of Object.entries(attrs)) e.setAttribute(key, String(value)); parent.append(e); return e;
  }
  const viewBox = () => svg.setAttribute('viewBox', view.join(' '));
  function reset() {
    const size = sheet?.size_in_source_units;
    if (!size) return;
    const margin = Math.max(...size) * .02;
    view = [-margin, -margin, size[0] + 2 * margin, size[1] + 2 * margin]; viewBox();
  }
  function select(item: Item) {
    selected = item.id;
    el('selected').textContent = item.geometry.kind === 'text' ? displayedText(item.geometry) : 'Drawing element selected';
    el('selection-info').replaceChildren(label('h2', 'Drawing element'), details(item));
    svg.querySelectorAll<SVGElement>('[data-item-id]').forEach(e => e.classList.toggle('drawing-selected', e.dataset.itemId === selected));
  }
  function rows() {
    results.replaceChildren(); const query = search.value.toLocaleLowerCase();
    if (!query) return;
    const matches = (sheet?.items ?? []).filter(i => i.geometry.kind === 'text' && (i.geometry.text.toLocaleLowerCase().includes(query) || displayedText(i.geometry).toLocaleLowerCase().includes(query)));
    results.append(label('p', `${matches.length} matching text elements`));
    for (const i of matches) { const button = label('button', displayedText(i.geometry)); button.onclick = () => select(i); results.append(button); }
  }
  function draw() {
    svg.replaceChildren();
    if (!sheet?.size_in_source_units || sheet.status === 'unavailable') return;
    const [W, H] = sheet.size_in_source_units;
    node('rect', { x: 0, y: 0, width: W, height: H, fill: 'white', stroke: '#b8c4cf', 'stroke-width': .035 });
    for (const item of sheet.items) {
      const g = item.geometry, style = item.style;
      const color = style.rgba?.every(c => c >= 0 && c <= 1) ? `rgba(${style.rgba.slice(0, 3).map(c => Math.round(c * 255)).join(',')},${style.rgba[3]})` : '#111';
      let e: SVGElement;
      if (g.kind === 'image') {
        const image = data.images.find(i => i.reference === g.reference);
        if (!image?.resource || !/^drawing-image-\d+\.(png|jpg)$/.test(image.resource)) continue;
        e = node('image', { href: image.resource, width: 1, height: 1, preserveAspectRatio: 'none',
          transform: `matrix(${g.u[0]} ${-g.u[1]} ${g.v[0]} ${-g.v[1]} ${g.origin[0]} ${H - g.origin[1]})` });
      } else if (g.kind === 'text') {
        if (!texts.checked) continue;
        const d = g.direction, u = g.up, f = g.font, factor = f?.width_factor ?? 1, symbolFallback = diameterFallback(g);
        e = node('text', { transform: `matrix(${d[0] * factor} ${-d[1] * factor} ${-u[0]} ${u[1]} ${g.position[0]} ${H - g.position[1]})`,
          'font-family': symbolFallback ? 'sans-serif' : fontFamily(f), 'font-size': f?.height_candidate ?? .25, 'font-weight': f?.weight_candidate ?? 400,
          'font-style': f?.flags === 1 ? 'italic' : 'normal', fill: color });
        // Native regular/rotated/multiline controls and separate Tahoma bold
        // and italic controls support capital height as a candidate. Each
        // stored run keeps its own baseline; no line spacing is inferred.
        const measuredFont = symbolFallback || (f !== null && (
          (/^(arial|tahoma)$/i.test(f.family) && f.flags === 0 && f.weight_candidate === 400)
          || (/^tahoma$/i.test(f.family) && ((f.flags === 0 && f.weight_candidate === 700)
            || (f.flags === 1 && f.weight_candidate === 400)))));
        const capitalHeight = capHeightSupported && measuredFont && f !== null && g.raw_flags === 9
          && Number.isFinite(f.height_candidate) && f.height_candidate > 0 && !/[\r\n]/.test(g.text);
        e.style.fontSizeAdjust = capitalHeight ? 'cap-height 1' : 'none';
        e.dataset.fontSizing = capitalHeight ? 'capital-height-candidate' : 'unverified-em-fallback';
        if (symbolFallback) { e.dataset.symbolFallback = 'AIGDT:n->U+2300'; e.dataset.rawText = g.text; }
        // Run positions include leading/trailing spaces, including space-only runs.
        e.style.whiteSpace = 'pre';
        displayedText(g).split(/\r\n|\n|\r/).forEach((line, index) => { node('tspan', { x: 0, dy: index ? '1.2em' : 0 }, e).textContent = line; });
      } else {
        if (g.kind === 'curve' && !curves.checked) continue;
        let points: Point[];
        if (g.kind === 'curve') {
          const n = Math.max(8, Math.ceil((g.end - g.start) * 128 / (2 * Math.PI)));
          points = Array.from({ length: n + 1 }, (_, i) => {
            const a = g.start + (g.end - g.start) * i / n;
            return g.center.map((c, j) => c + g.u[j] * Math.cos(a) + g.v[j] * Math.sin(a)) as Point;
          });
        } else points = g.points;
        e = node('polyline', { points: points.map(p => `${p[0]},${H - p[1]}`).join(' '), fill: 'none', stroke: color,
          'stroke-width': style.width ?? .035, 'stroke-dasharray': (style.dash ?? []).join(' '), 'stroke-linejoin': 'round' });
      }
      e.dataset.itemId = item.id; e.dataset.kind = g.kind; e.classList.toggle('drawing-selected', item.id === selected);
      e.addEventListener('click', () => select(item));
    }
    viewBox();
  }
  function present(s: Sheet) {
    sheet = s; selected = null; drag = null;
    el('cad').dataset.sheetId = s.id;
    el('selected').textContent = 'No drawing element selected';
    el('selection-info').replaceChildren(label('h2', sheetLabel(s)), details({ ...s, items: undefined }));
    tree.querySelectorAll('button').forEach(b => b.setAttribute('aria-pressed', String(b.dataset.sheetId === s.id)));
    viewList.replaceChildren();
    for (const v of s.views ?? []) {
      const button = label('button', v.name || 'Saved view'); button.dataset.viewId = v.id;
      button.onclick = () => {
        const members = new Set(v.item_ids);
        svg.querySelectorAll<SVGElement>('[data-item-id]').forEach(e => e.classList.toggle('drawing-selected', members.has(e.dataset.itemId!)));
        el('selected').textContent = v.name || 'Saved view';
        el('selection-info').replaceChildren(label('h2', 'Saved view'), details(v));
      };
      viewList.append(button);
    }
    omissions.replaceChildren(label('h3', 'Drawing omissions'), label('p', `${s.omissions.length} omitted records`), details(s.omissions));
    if (s.items.some(i => i.geometry.kind === 'text')) {
      const note = label('p', capHeightSupported
        ? 'Text appearance is approximate and depends on installed fonts.'
        : 'This browser cannot adjust text height. Text sizes and fonts are approximate.');
      note.id = 'drawing-font-note'; omissions.append(note);
    }
    if (s.items.some(i => diameterFallback(i.geometry))) {
      const note = label('p', 'Diameter symbol uses a Unicode substitute for AIGDT. The original text is retained; glyph shape is approximate.');
      note.id = 'drawing-symbol-note'; omissions.append(note);
    }
    if (s.items.some(i => { const g = i.geometry; return g.kind === 'image' && data.images.some(a => a.reference === g.reference && ['decoded_monochrome_view_cache_unqualified', 'decoded_rgba_view_cache_unqualified'].includes(a.status)); })) {
      omissions.append(label('p', 'Some views use saved raster images. Detail when zooming is limited by their stored resolution.'));
    }
    for (const issue of s.diagnostics) omissions.append(label('p', issue));
    for (const image of data.images.filter(i => !i.resource)) omissions.append(label('p', `Image ${image.reference}: ${image.diagnostic ?? image.status}`));
    const available = s.status !== 'unavailable' && s.size_in_source_units !== null;
    svg.style.display = available ? '' : 'none'; fit.disabled = zoom.disabled = !available;
    el('empty').hidden = available;
    if (!available) { el('empty').querySelector('h2')!.textContent = 'Sheet display unavailable'; el('empty').querySelector('p')!.textContent = 'The stored sheet could not be linked to supported display elements. See Read results.'; }
    el('render-status').textContent = available
      ? `${sheetLabel(s)} · ${s.items.length} saved elements · Partial display`
      : `${sheetLabel(s)} · Display unavailable`;
    el('cad').dataset.rendered = String(available); el('cad').dataset.displayed = String(s.items.length);
    reset(); draw(); rows();
  }
  async function choose(descriptor: Descriptor) {
    controller?.abort(); controller = new AbortController();
    const generation = ++request;
    sheet = undefined; selected = null; drag = null;
    svg.replaceChildren(); results.replaceChildren(); omissions.replaceChildren(); viewList.replaceChildren();
    el('selected').textContent = 'No drawing element selected';
    el('selection-info').replaceChildren();
    el('cad').dataset.rendered = 'false'; el('cad').dataset.displayed = '0';
    tree.querySelectorAll('button').forEach(b => b.setAttribute('aria-pressed', String(b.dataset.sheetId === descriptor.id)));
    const unavailable = { ...descriptor, items: [], omissions: [], sources: [], status: 'unavailable' };
    if (!descriptor.resource) { present(unavailable); return; }
    fit.disabled = zoom.disabled = true; el('empty').hidden = false;
    el('empty').querySelector('h2')!.textContent = 'Loading sheet';
    el('empty').querySelector('p')!.textContent = sheetLabel(descriptor);
    el('render-status').textContent = 'Loading selected sheet…';
    try {
      const payload = await loadSheet(data.source_sha256, descriptor, controller.signal);
      if (stopped || generation !== request) return;
      if (payload.items.length !== descriptor.item_count || payload.omissions.length !== descriptor.omission_count) {
        throw new Error('Sheet payload counts differ from descriptor');
      }
      present({ ...descriptor, items: payload.items, views: payload.views, omissions: payload.omissions, sources: payload.sources });
    } catch (error) {
      if (stopped || generation !== request) return;
      present({ ...unavailable, diagnostics: [...descriptor.diagnostics, `Sheet resource unavailable: ${String(error)}`] });
    }
  }
  for (const s of data.sheets) {
    const button = label('button', sheetLabel(s)); button.dataset.sheetId = s.id;
    button.addEventListener('click', () => { void choose(s); }); tree.append(button);
  }
  fit.onclick = reset; zoom.onclick = () => { const cx = view[0] + view[2] / 2, cy = view[1] + view[3] / 2;
    view[2] = Math.max(view[2] / 1.5, 1e-9); view[3] = Math.max(view[3] / 1.5, 1e-9); view[0] = cx - view[2] / 2; view[1] = cy - view[3] / 2; viewBox(); };
  texts.onchange = curves.onchange = draw; search.oninput = rows;
  function position(e: PointerEvent) {
    const matrix = svg.getScreenCTM(); if (!matrix) return null;
    return new DOMPoint(e.clientX, e.clientY).matrixTransform(matrix.inverse());
  }
  svg.onpointerdown = e => { const p = position(e); if (p) { drag = { x: p.x, y: p.y, origin: [...view] }; svg.setPointerCapture(e.pointerId); } };
  svg.onpointermove = e => { if (!drag) return; const p = position(e); if (!p) return;
    view[0] += drag.x - p.x; view[1] += drag.y - p.y; viewBox(); };
  svg.onpointerup = svg.onpointercancel = svg.onlostpointercapture = () => { drag = null; };
  if (data.sheets.length) void choose(data.sheets.find(s => s.resource && s.status !== 'unavailable') ?? data.sheets[0]);
  else {
    el('empty').hidden = false; el('empty').querySelector('h2')!.textContent = 'Drawing display unavailable';
    el('empty').querySelector('p')!.textContent = 'No supported stored sheet list was resolved. Saved previews and diagnostics remain available.';
    el('render-status').textContent = 'Drawing display unavailable'; fit.disabled = zoom.disabled = true;
  }
  return () => { stopped = true; ++request; controller?.abort(); drag = null; svg.replaceChildren(); controls.remove(); svg.remove(); };
}
