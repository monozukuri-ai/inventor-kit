/** Immutable, bounded, input-bound sheet resource loading. */
export const MAX_SHEET_BYTES = 32 * 1024 * 1024;
export type SheetResource = { id: string; resource: string | null; bytes: number | null; sha256: string | null };

function unpackItemRecords(payload: any): any {
  const { items, placements, source_bases: bases, sheet_id: sheet, ...rest } = payload;
  const integer = (n: any) => Number.isSafeInteger(n) && n >= 0;
  if (typeof sheet !== 'string' || sheet.length > 512 || !Array.isArray(items) || items.length > 100000
      || !Array.isArray(placements) || placements.length > items.length
      || !Array.isArray(bases) || bases.length > items.length) throw new Error('Invalid drawing item record tables');
  for (const p of placements) {
    if (!Array.isArray(p) || p.length !== 3 || typeof p[0] !== 'string'
        || !/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(p[0]) || !integer(p[1])
        || !Array.isArray(p[2]) || p[2].length > 128 || p[2].some((n: any) => !integer(n))) {
      throw new Error('Invalid drawing placement record');
    }
  }
  for (const base of bases) {
    if (!Array.isArray(base) || base.length !== 3 || base.some((s: any) => typeof s !== 'string')
        || !['cfb_stream', 'inflated_stream'].includes(base[2])) throw new Error('Invalid drawing source base');
  }
  const identities = new Set<string>();
  const expanded = items.map((item: any) => {
    if (!item || typeof item !== 'object' || Array.isArray(item)
        || Object.keys(item).sort().join(',') !== 'geometry,record,style_index') throw new Error('Invalid drawing compact item');
    const r = item.record;
    if (!Array.isArray(r) || r.length !== 5 || r.some((n: any) => !integer(n))
        || r[0] >= placements.length || r[2] >= bases.length || r[3] > r[4]) throw new Error('Invalid drawing compact item reference');
    const [segment, placement, path] = placements[r[0]], [source, stream, domain] = bases[r[2]];
    const id = `${sheet}/${segment}/${placement}/${r[1]}`;
    if (identities.has(id)) throw new Error('Duplicate drawing item ID');
    identities.add(id);
    return { id, geometry: item.geometry, style_index: item.style_index, segment_id: segment,
      record_ordinal: r[1], placement_record: placement, group_path: path,
      source: { source_id: source, stream, byte_domain: domain, start_offset: r[3], end_offset: r[4] } };
  });
  return { ...rest, sheet_id: sheet, schema_version: 3, items: expanded };
}

export function unpackSheetPayload(payload: any): any {
  if (payload?.schema_version === 4) payload = unpackItemRecords(payload);
  if (payload?.schema_version === 1) return payload;
  if (![2, 3].includes(payload?.schema_version)) throw new Error('Unsupported drawing sheet schema');
  const { styles, items, ...rest } = payload;
  if (!Array.isArray(styles) || !Array.isArray(items) || styles.length > items.length || items.length > 100000
      || styles.some((s: any) => !s || typeof s !== 'object' || Array.isArray(s))
      || items.some((i: any) => !i || typeof i !== 'object' || Array.isArray(i) || 'style' in i
        || !Number.isSafeInteger(i.style_index) || i.style_index < 0 || i.style_index >= styles.length)) {
    throw new Error('Invalid drawing style table or reference');
  }
  // Reuse the verified table objects; never inflate to a duplicate JSON string.
  const result = { ...rest, schema_version: 1, items: items.map(({ style_index, ...item }: any) => ({ ...item, style: styles[style_index] })) };
  if (payload.schema_version === 3) {
    function views(rows: any, budget: { left: number }): any[] {
      if (!Array.isArray(rows) || rows.length > 4096) throw new Error('Invalid drawing view reference table');
      return rows.map((view: any) => {
        const refs = view?.item_indices;
        if (!view || typeof view !== 'object' || !Array.isArray(refs) || 'item_ids' in view || refs.length > budget.left
            || refs.some((i: any) => !Number.isSafeInteger(i) || i < 0 || i >= items.length)) {
          throw new Error('Invalid drawing view item reference');
        }
        budget.left -= refs.length;
        const { item_indices, ...rest } = view;
        return { ...rest, item_ids: refs.map((i: number) => items[i].id) };
      });
    }
    result.views = views(result.views, { left: 100000 });
    const report = result.export_report;
    if (!report || !Array.isArray(report.sheets) || report.sheets.length > 256
        || report.sheets.some((s: any) => !s || typeof s !== 'object' || Array.isArray(s))) {
      throw new Error('Invalid drawing export view reference table');
    }
    const budget = { left: 100000 };
    result.export_report = { ...report, sheets: report.sheets.map((s: any) => ({ ...s, views: views(s.views, budget) })) };
  }
  return result;
}

export async function loadSheet(source: string, sheet: SheetResource, signal: AbortSignal): Promise<any> {
  if (!/^[a-f0-9]{64}$/.test(source) || !sheet.id.startsWith(source + '/')) throw new Error('Sheet input identity mismatch');
  if (!sheet.resource || !/^drawing-sheet-[a-f0-9]{64}\.json$/.test(sheet.resource)
      || sheet.resource !== `drawing-sheet-${sheet.sha256}.json`
      || !Number.isSafeInteger(sheet.bytes) || sheet.bytes! <= 0 || sheet.bytes! > MAX_SHEET_BYTES) {
    throw new Error('Invalid drawing sheet resource descriptor');
  }
  const response = await fetch(sheet.resource, { signal });
  if (!response.ok || !response.body) throw new Error(`Sheet request failed (${response.status})`);
  const length = response.headers.get('content-length');
  if (length !== null && Number(length) !== sheet.bytes) throw new Error('Sheet resource size mismatch');
  const reader = response.body.getReader(), chunks: Uint8Array[] = [];
  let count = 0;
  try {
    while (true) {
      const next = await reader.read();
      if (next.done) break;
      count += next.value.byteLength;
      if (count > sheet.bytes!) throw new Error('Sheet resource byte limit exceeded');
      chunks.push(next.value);
    }
  } finally { await reader.cancel(); reader.releaseLock(); }
  if (count !== sheet.bytes) throw new Error('Truncated sheet resource');
  const bytes = new Uint8Array(count); let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
  const digest = Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)), b => b.toString(16).padStart(2, '0')).join('');
  if (digest !== sheet.sha256) throw new Error('Sheet resource hash mismatch');
  const payload = unpackSheetPayload(JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes)));
  if (payload.schema_version !== 1 || payload.scene_kind !== 'drawing_sheet'
      || payload.source_sha256 !== source || payload.sheet_id !== sheet.id
      || payload.units !== 'source_units_unverified' || !Array.isArray(payload.items)
      || !Array.isArray(payload.omissions) || !Array.isArray(payload.sources)
      || typeof payload.svg !== 'string' || !payload.export_report
      || payload.export_report.source_sha256 !== source || payload.export_report.selected_sheet_id !== sheet.id
      || payload.items.some((i: any) => typeof i.id !== 'string' || !i.id.startsWith(sheet.id + '/'))) {
    throw new Error('Sheet payload identity or schema mismatch');
  }
  const svgBytes = new TextEncoder().encode(payload.svg);
  const svgHash = Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', svgBytes)), b => b.toString(16).padStart(2, '0')).join('');
  if (payload.export_report.export?.sha256 !== svgHash || payload.export_report.export?.bytes !== svgBytes.byteLength) {
    throw new Error('SVG export report integrity mismatch');
  }
  return payload;
}
