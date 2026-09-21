/** Immutable, bounded, input-bound sheet resource loading. */
export const MAX_SHEET_BYTES = 16 * 1024 * 1024;
export type SheetResource = { id: string; resource: string | null; bytes: number | null; sha256: string | null };

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
  const payload = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
  if (payload.schema_version !== 1 || payload.scene_kind !== 'drawing_sheet'
      || payload.source_sha256 !== source || payload.sheet_id !== sheet.id
      || payload.units !== 'source_units_unverified' || !Array.isArray(payload.items)
      || !Array.isArray(payload.omissions) || !Array.isArray(payload.sources)
      || payload.items.some((i: any) => typeof i.id !== 'string' || !i.id.startsWith(sheet.id + '/'))) {
    throw new Error('Sheet payload identity or schema mismatch');
  }
  return payload;
}
