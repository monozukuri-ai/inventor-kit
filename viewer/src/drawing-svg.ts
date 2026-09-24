/** Mount only the bounded declarative SVG vocabulary emitted by our renderer. */
const NS = 'http://www.w3.org/2000/svg';
const tags = new Set(['svg', 'title', 'desc', 'metadata', 'rect', 'defs', 'image', 'use', 'text', 'tspan', 'polyline', 'path']);
const attrs = new Set(['xmlns', 'viewBox', 'width', 'height', 'role', 'aria-label', 'data-source-sha256', 'data-sheet-id',
  'data-renderer-version', 'id', 'x', 'y', 'fill', 'stroke', 'stroke-width', 'href', 'preserveAspectRatio', 'transform',
  'opacity', 'data-item-id', 'data-kind', 'data-style-state', 'font-family', 'font-size', 'font-weight', 'font-style',
  'style', 'data-font-sizing', 'data-symbol-fallback', 'data-raw-text', 'data-font-fallback', 'data-text-replacement',
  'dy', 'd', 'points', 'stroke-dasharray', 'stroke-dashoffset', 'data-dash-rendering', 'stroke-linejoin', 'stroke-linecap',
  'data-curve-rendering', 'data-projection-error-bound']);

export function parseDrawingSvg(source: string, sourceHash: string, sheetId: string,
  items: { id: string; geometry: { kind: string } }[]): SVGSVGElement {
  const doc = new DOMParser().parseFromString(source, 'image/svg+xml');
  const svg = doc.documentElement;
  if (doc.doctype || doc.querySelector('parsererror') || svg.localName !== 'svg'
      || svg.getAttribute('data-source-sha256') !== sourceHash || svg.getAttribute('data-sheet-id') !== sheetId
      || svg.getAttribute('data-renderer-version') !== '1') throw new Error('Invalid saved SVG identity');
  const instructions = doc.createTreeWalker(doc, NodeFilter.SHOW_PROCESSING_INSTRUCTION);
  if (instructions.nextNode()) throw new Error('Unsupported SVG processing instruction');
  const itemKinds = new Map(items.map(i => [i.id, i.geometry.kind])), seen = new Set<string>();
  for (const node of [svg, ...Array.from(svg.querySelectorAll('*'))]) {
    if (node.namespaceURI !== NS || !tags.has(node.localName) || (node !== svg && node.localName === 'svg')) {
      throw new Error('Unsupported SVG element');
    }
    for (const a of Array.from(node.attributes)) {
      if (!attrs.has(a.name)) throw new Error('Unsupported SVG attribute');
      if (a.name === 'style' && !/^white-space:pre;font-size-adjust:(cap-height 1|none)$/.test(a.value)) throw new Error('Unsupported SVG style');
      if (['fill', 'stroke'].includes(a.name) && !/^(white|none|#[a-f0-9]{3,6}|rgb\([0-9]+,[0-9]+,[0-9]+\))$/.test(a.value)) throw new Error('Unsupported SVG paint');
      if (a.name === 'id' && !/^drawing-image-\d+$/.test(a.value)) throw new Error('Unsupported SVG definition ID');
      if (a.name === 'href' && !((node.localName === 'image' && /^data:image\/(png|jpeg);base64,[A-Za-z0-9+/=]+$/.test(a.value))
          || (node.localName === 'use' && /^#drawing-image-\d+$/.test(a.value)))) throw new Error('External SVG reference');
    }
    const id = node.getAttribute('data-item-id');
    if (id !== null) {
      if (!itemKinds.has(id) || seen.has(id) || node.getAttribute('data-kind') !== itemKinds.get(id)) throw new Error('SVG item identity mismatch');
      seen.add(id);
    }
  }
  return svg as unknown as SVGSVGElement;
}
