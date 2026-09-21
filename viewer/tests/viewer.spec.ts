import { test, expect, type Page } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import { resolve } from 'node:path';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';

const root = resolve(import.meta.dirname, '../..');
const python = process.env.VIEWER_PYTHON || resolve(root, '.venv/bin/python');
const corpus = process.env.VIEWER_CORPUS || resolve(root, 'fixtures/public');
let process_: ChildProcess | undefined;
let errors: string[] = [];

async function storedSheet(page: Page, url: string, descriptor: any) {
  return { ...descriptor, ...await (await page.request.get(url + descriptor.resource)).json() };
}

async function mockDrawingSheets(page: Page, state: any, sheets: any[]) {
  const descriptors = [];
  for (const sheet of sheets) {
    const id = sheet.id.startsWith(state.source.sha256 + '/') ? sheet.id : `${state.source.sha256}/sheet-${sheet.id}`;
    const { items, omissions, sources, views, ...rest } = sheet;
    const descriptor = { ...rest, id, item_count: items.length, omission_count: omissions.length };
    // Keep only descriptor fields in the snapshot, just as the real server does.
    for (const key of ['schema_version', 'scene_kind', 'source_sha256', 'sheet_id', 'units']) delete descriptor[key];
    if (sheet.status === 'unavailable') Object.assign(descriptor, { resource: null, bytes: null, sha256: null });
    else {
      const payload = { schema_version: 1, scene_kind: 'drawing_sheet', source_sha256: state.source.sha256,
        sheet_id: id, units: 'source_units_unverified', items: items.map((item: any, i: number) => ({ ...item, id: `${id}/test-${i}` })), omissions, sources,
        views: views?.map((v: any) => ({ ...v, id: `${id}/view-${v.placement_record}`,
          item_ids: items.flatMap((item: any, i: number) => item.placement_record === v.placement_record ? [`${id}/test-${i}`] : []) })) };
      const body = JSON.stringify(payload), hash = createHash('sha256').update(body).digest('hex');
      Object.assign(descriptor, { resource: `drawing-sheet-${hash}.json`, bytes: Buffer.byteLength(body), sha256: hash });
      await page.route(`**/${descriptor.resource}`, route => route.fulfill({ body, contentType: 'application/json' }));
    }
    descriptors.push(descriptor);
  }
  state.drawing.sheets = descriptors;
  await page.route('**/state.json', route => route.fulfill({ json: state }));
}

async function open(page: Page, file: string, args: string[] = []) {
  errors = [];
  const env = { ...process.env }; delete env.PYTHONPATH;
  process_ = spawn(python, ['-m', 'inventor_kit.viewer', resolve(corpus, file), '--no-browser', ...args],
    { cwd: process.env.VIEWER_CWD || root, env, stdio: ['ignore', 'pipe', 'pipe'] });
  const url = await new Promise<string>((accept, reject) => {
    let output = ''; let stderr = '';
    const timer = setTimeout(() => reject(new Error(`Startup timed out: ${stderr}`)), 15000);
    process_!.stderr!.on('data', data => { stderr += data.toString(); });
    process_!.stdout!.on('data', data => {
      output += data.toString();
      const found = output.match(/http:\/\/127\.0\.0\.1:\d+\/[^\s]+/);
      if (found) { clearTimeout(timer); accept(found[0]); }
    });
    process_!.on('error', error => { clearTimeout(timer); reject(error); });
    process_!.on('exit', code => { clearTimeout(timer); reject(new Error(`Startup exited ${code}: ${stderr}`)); });
  });
  const origin = new URL(url).origin;
  page.on('pageerror', error => errors.push(String(error)));
  page.on('console', msg => { if (msg.type() === 'error') errors.push(msg.text()); });
  await page.route('**/*', route => {
    if (new URL(route.request().url()).origin === origin) return route.continue();
    errors.push(`External request: ${route.request().url()}`); return route.abort();
  });
  await page.goto(url);
  // Match the worker's 120 s deadline, including a cold CAD import. Fail
  // immediately on a display/transport error instead of using a fixed sleep.
  await page.waitForFunction(() => document.body.dataset.jobStatus || document.body.dataset.displayError, null, { timeout: 125000 });
  expect(await page.locator('body').getAttribute('data-display-error')).toBeNull();
  await expect(page.locator('body')).toHaveAttribute('data-job-status', /finished|failed/);
  return url;
}

test.afterEach(async () => {
  if (process_ && process_.exitCode === null) {
    const child = process_;
    const exited = new Promise<number | null>(resolve => child.once('exit', resolve));
    child.kill('SIGINT');
    const timer = setTimeout(() => child.kill('SIGKILL'), 5000);
    const code = await exited;
    clearTimeout(timer);
    expect(code).toBe(0);
  }
  process_ = undefined;
  expect(errors).toEqual([]);
});

test('partial IPT body display retains the two omitted bodies and source IDs', async ({ page }) => {
  const url = await open(page, 'INV_nist_ftc_06_asme1_2021.ipt', ['--allow-partial']);
  await expect(page.locator('#cad')).toHaveAttribute('data-displayed', '1');
  await expect(page.locator('.body-row')).toHaveCount(3);
  await expect(page.locator('.body-row[data-status="unsupported"]')).toHaveCount(2);
  await expect(page.locator('#diagnostics')).toContainText('1 of 3 bodies');
  const scene = await (await page.request.get(url + 'state.json')).json();
  expect(scene.part.geometry_complete).toBe(false);
  expect(scene.part.current_state_verified).toBe(false);
  expect(scene.meshes[0].face_count).toBe(146);
  expect(scene.part.omissions).toHaveLength(2);
  expect(scene.nodes[0].body_id).toMatch(/^body-[a-f0-9]{64}$/);
  expect(errors).toEqual([]);
});

for (const file of ['SamplePart.ipt', 'Cylinder.ipt', 'INV_nist_ftc_09_asme1_2024.ipt', 'INV_nist_ctc_04_asme1_2021.ipt']) {
  test(`renders ${file} offline`, async ({ page }, info) => {
    const url = await open(page, file);
    await expect(page.locator('#cad')).toHaveAttribute('data-rendered', 'true');
    await expect(page.locator('#body-count')).toHaveText('1');
    await expect(page.locator('.notice')).toContainText('Current Model State is unverified');
    const state = await (await page.request.get(url + 'state.json')).json();
    expect(state.complete).toBe(false);
    expect(state.meshes[0].triangle_count).toBeGreaterThan(0);
    expect(state.meshes[0].face_count).toBeGreaterThan(0);
    await expect(page.locator('#previews img').first()).toBeVisible();
    await expect(page.locator('#candidates')).toContainText('Selected');
    // Fit must also recover a panned model, even when its zoom has not changed.
    await page.getByRole('button', { name: 'Fit', exact: true }).click();
    const iso = await page.locator('#cad canvas').screenshot();
    const box = (await page.locator('#cad canvas').boundingBox())!;
    await page.keyboard.down('Shift');
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + 120, box.y + box.height / 2 + 60, { steps: 10 });
    await page.mouse.up();
    await page.keyboard.up('Shift');
    await expect.poll(async () => (await page.locator('#cad canvas').screenshot()).equals(iso)).toBe(false);
    await page.getByRole('button', { name: 'Fit', exact: true }).click();
    await expect.poll(async () => (await page.locator('#cad canvas').screenshot()).equals(iso)).toBe(true);
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + 100, box.y + box.height / 2 + 50, { steps: 10 });
    await page.mouse.up();
    await expect.poll(async () => (await page.locator('#cad canvas').screenshot()).equals(iso)).toBe(false);
    await page.getByRole('button', { name: 'Fit', exact: true }).click();
    await page.getByRole('button', { name: 'Front', exact: true }).click();
    await expect.poll(async () => (await page.locator('#cad canvas').screenshot()).equals(iso)).toBe(false);
    await page.getByRole('button', { name: 'Body 1', exact: true }).click();
    await expect(page.locator('#selected')).toHaveAttribute('data-node-id', state.nodes[0].id);
    const shown = await page.locator('#cad canvas').screenshot();
    await page.getByLabel('Show Body 1', { exact: true }).uncheck();
    await expect(page.getByLabel('Show Body 1', { exact: true })).not.toBeChecked();
    const hidden = await page.locator('#cad canvas').screenshot();
    expect(hidden.equals(shown)).toBe(false);
    await page.getByLabel('Show Body 1', { exact: true }).check();
    await page.getByLabel('Edges', { exact: true }).uncheck();
    await page.getByLabel('Edges', { exact: true }).check();
    await page.getByRole('button', { name: 'Isometric', exact: true }).click();
    await page.screenshot({ path: info.outputPath('viewer.png') });
    expect(await page.locator('body').getAttribute('data-display-error')).toBeNull();
  });
}

test('unsupported geometry keeps previews, properties and the conversion reason', async ({ page }) => {
  await open(page, 'EPFL_Elytron_140mm_v1.ipt');
  await expect(page.getByRole('heading', { name: 'No 3D geometry to display' })).toBeVisible();
  await expect(page.locator('#diagnostics')).toContainText('unsupported geometry intcurve-curve');
  await expect(page.locator('#previews img').first()).toBeVisible();
  expect(await page.locator('#properties .property').count()).toBeGreaterThan(0);
});

test('current-state refusal retains the inventory', async ({ page }) => {
  await open(page, 'SamplePart.ipt', ['--require-current-state']);
  await expect(page.getByRole('heading', { name: 'No 3D geometry to display' })).toBeVisible();
  await expect(page.locator('#previews img').first()).toBeVisible();
  await expect(page.locator('#candidates')).toContainText('Selection:');
});

test('metadata-only and IAM do not claim part geometry', async ({ page }) => {
  await open(page, 'BoltedConnection.iam', ['--metadata-only']);
  await expect(page.locator('#document-info')).toContainText('assembly');
  await expect(page.locator('#stages')).toContainText('not_attempted');
});

test('IAM inventory requires explicit saved-state permission', async ({ page }) => {
  await open(page, 'm5-samplebg/Subassembly.iam');
  await expect(page.getByRole('heading', { name: 'No 3D geometry to display' })).toBeVisible();
  await expect(page.locator('#body-list')).toContainText('Triangle');
  await expect(page.locator('#diagnostics')).toContainText('--allow-unverified-state');
  await expect(page.getByLabel('Show Triangle', { exact: true })).toBeDisabled();
});

test('native IAM displays a saved part and its occurrence provenance', async ({ page }, info) => {
  const url = await open(page, 'm5-samplebg/Subassembly.iam', ['--allow-unverified-state']);
  await expect(page.locator('#cad')).toHaveAttribute('data-displayed', '1');
  await page.getByRole('button', { name: 'Triangle', exact: true }).click();
  await expect(page.locator('#selected')).toHaveAttribute('data-node-id', '/document/occ-1');
  const scene = await (await page.request.get(url + 'state.json')).json();
  expect(scene.nodes[0].occurrence_path).toEqual([1]);
  expect(scene.assembly.displayed_instances).toBe(1);
  expect(scene.current_state).toBe('unverified'); expect(scene.complete).toBe(false);
  await page.screenshot({ path: info.outputPath('subassembly.png') });
});

test('incomplete IAM keeps its tree and reasons without partial permission', async ({ page }) => {
  await open(page, 'm5-samplebg/SampleBg.iam', ['--allow-unverified-state', '--search-root', resolve(corpus, 'm5-samplebg/iPartSample')]);
  await expect(page.getByRole('heading', { name: 'No 3D geometry to display' })).toBeVisible();
  await expect(page.locator('.body-row')).toHaveCount(7);
  await expect(page.locator('#omissions')).toContainText('identity mismatch');
  await expect(page.locator('#omissions')).toContainText('geometry unavailable');
  await expect(page.locator('#diagnostics')).toContainText('--allow-partial');
  expect(await page.locator('.body-row input:enabled').count()).toBe(0);
});

test('partial IAM renders five parts and isolates an occurrence', async ({ page }, info) => {
  const url = await open(page, 'm5-samplebg/SampleBg.iam', ['--allow-unverified-state', '--allow-partial', '--search-root', resolve(corpus, 'm5-samplebg/iPartSample')]);
  await expect(page.locator('#cad')).toHaveAttribute('data-displayed', '5');
  await expect(page.locator('.body-row')).toHaveCount(7);
  await expect(page.getByLabel('Show Subassembly', { exact: true })).toBeDisabled();
  const scene = await (await page.request.get(url + 'state.json')).json();
  expect(scene.omissions.map((o: { reason: string }) => o.reason).sort()).toEqual(['geometry_unavailable', 'identity_mismatch']);
  const all = await page.locator('#cad canvas').screenshot();
  await page.getByRole('button', { name: 'Cylinder', exact: true }).click();
  await page.getByRole('button', { name: 'Isolate', exact: true }).click();
  await expect(page.locator('.body-row[data-visible="true"]')).toHaveCount(1);
  expect((await page.locator('#cad canvas').screenshot()).equals(all)).toBe(false);
  await page.getByRole('button', { name: 'Show all', exact: true }).click();
  await expect(page.locator('.body-row[data-visible="true"]')).toHaveCount(5);
  expect(await (await page.request.get(url + 'state.json')).json()).toEqual(scene);
  await page.screenshot({ path: info.outputPath('samplebg.png') });
});

test('synthetic nested repeated parts share downloads and keep selection independent', async ({ page }, info) => {
  const url = await open(page, 'm5-samplebg/Subassembly.iam', ['--allow-unverified-state']);
  await expect(page.locator('#cad')).toHaveAttribute('data-rendered', 'true');
  const source = await (await page.request.get(url + 'state.json')).json();
  const original = source.nodes[0];
  const a = [[0, -1, 0, 11], [1, 0, 0, 2], [0, 0, 1, 3], [0, 0, 0, 1]];
  const b = [[1, 0, 0, 0], [0, 0, -1, 4], [0, 1, 0, 0], [0, 0, 0, 1]];
  const ab = [[0, 0, 1, 7], [1, 0, 0, 2], [0, 1, 0, 3], [0, 0, 0, 1]];
  const c = [[1, 0, 0, 120], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]];
  const scene = { ...source, nodes: [
    { ...original, name: 'Nested', kind: 'assembly', mesh_id: null, status: 'group', local_transform_mm: a, world_transform_mm: a },
    { ...original, id: '/document/occ-1/occ-1', parent: '/document/occ-1', name: 'Repeated', occurrence_path: [1, 1], local_transform_mm: b, world_transform_mm: ab },
    { ...original, id: '/document/occ-2', name: 'Repeated', occurrence_path: [2], local_transform_mm: c, world_transform_mm: c },
    { ...original, id: '/document/occ-3', name: 'Unknown placement', occurrence_path: [3], mesh_id: null, status: 'placement_unavailable', local_transform_mm: null, world_transform_mm: null },
  ], assembly: { ...source.assembly, structure_status: 'partial', allow_partial: true, displayed_instances: 2, displayed_definitions: 1 },
    omissions: [{ instance: 3, path: [3], reason: 'placement_unavailable', detail: 'Synthetic unknown placement' }] };
  await page.route('**/state.json', route => route.fulfill({ json: scene }));
  const downloads = new Map<string, number>();
  page.on('request', request => { if (request.url().endsWith('.bin')) downloads.set(request.url(), (downloads.get(request.url()) ?? 0) + 1); });
  await page.reload();
  await expect(page.locator('#cad')).toHaveAttribute('data-displayed', '2');
  expect(downloads.size).toBe(Object.keys(source.meshes[0].buffers).length);
  expect([...downloads.values()].every(n => n === 1)).toBe(true);
  // Exercise the real canvas picking callback, then the reverse tree-to-view path.
  const box = (await page.locator('#cad canvas').boundingBox())!;
  let picked: string | null = null;
  for (const [x, y] of [[.25, .3], [.75, .7], [.3, .35], [.8, .7]]) {
    await page.mouse.dblclick(box.x + x * box.width, box.y + y * box.height);
    await page.waitForTimeout(100);
    picked = await page.locator('#selected').getAttribute('data-node-id');
    if (picked) break;
  }
  expect(['/document/occ-1/occ-1', '/document/occ-2']).toContain(picked);
  const first = page.locator('.body-row[data-node-id="/document/occ-1/occ-1"]');
  const second = page.locator('.body-row[data-node-id="/document/occ-2"]');
  await expect(first.locator('xpath=../..')).toHaveClass('component-tree');
  await first.getByRole('button').click();
  await expect(first).toHaveClass(/active/); await expect(second).not.toHaveClass(/active/);
  await second.getByRole('button').click();
  await expect(second).toHaveClass(/active/); await expect(first).not.toHaveClass(/active/);
  await first.locator('input').uncheck();
  await expect(second.locator('input')).toBeChecked();
  await page.getByRole('button', { name: 'Nested', exact: true }).click();
  await page.getByRole('button', { name: 'Isolate', exact: true }).click();
  await expect(first.locator('input')).toBeChecked(); await expect(second.locator('input')).not.toBeChecked();
  await expect(page.getByLabel('Show Unknown placement', { exact: true })).toBeDisabled();
  await page.getByRole('button', { name: 'Show all', exact: true }).click();
  await page.screenshot({ path: info.outputPath('synthetic-nested.png') });
});

test('broken input reports failure without a blank page', async ({ page }) => {
  const directory = mkdtempSync(resolve(tmpdir(), 'inventor-broken-'));
  try {
    const path = resolve(directory, 'broken.ipt'); writeFileSync(path, 'invalid');
    await open(page, path);
    await expect(page.locator('#diagnostics')).not.toBeEmpty();
    await expect(page.getByRole('heading', { name: 'No 3D geometry to display' })).toBeVisible();
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test('IDW stored elements, images and source-unit controls without the 3D bundle', async ({ page }, testInfo) => {
  const scripts: string[] = [];
  page.on('request', request => { if (request.resourceType() === 'script') scripts.push(request.url()); });
  const url = await open(page, 'SampleBg.idw', ['--experimental-drawing']);
  await expect(page.locator('#cad')).toHaveAttribute('data-mode', 'drawing');
  await expect(page.locator('#drawing-svg [data-kind="polyline"]')).toHaveCount(91);
  await expect(page.locator('#drawing-svg [data-kind="curve"]')).toHaveCount(11);
  await expect(page.locator('#drawing-svg [data-kind="text"]')).toHaveCount(53);
  await expect(page.locator('#drawing-svg image')).toHaveCount(2);
  await expect(page.locator('#sheet-buttons button')).toHaveText(['Blatt']);
  await expect(page.locator('#document-info')).toContainText('Source units (unverified)');
  expect(scripts.some(s => s.includes('three-cad-viewer'))).toBe(false);
  const state = await (await page.request.get(url + 'state.json')).json();
  expect(state.drawing.qualified).toBe(false);
  expect(state.drawing.millimeters_per_unit).toBeNull();
  for (const image of state.drawing.images) {
    const response = await page.request.get(url + image.resource);
    expect(response.status()).toBe(200);
    expect(response.headers()['content-type']).toMatch(/^image\/(jpeg|png)$/);
    expect((await response.body()).length).toBeGreaterThan(100);
  }
  const initial = await page.locator('#drawing-svg').getAttribute('viewBox');
  await page.locator('#drawing-zoom').click();
  expect(await page.locator('#drawing-svg').getAttribute('viewBox')).not.toBe(initial);
  await page.locator('#drawing-fit').click();
  await expect(page.locator('#drawing-svg')).toHaveAttribute('viewBox', initial!);
  await page.locator('#drawing-text').uncheck();
  await expect(page.locator('#drawing-svg [data-kind="text"]')).toHaveCount(0);
  await page.locator('#drawing-text').check();
  await page.locator('#drawing-curves').uncheck();
  await expect(page.locator('#drawing-svg [data-kind="curve"]')).toHaveCount(0);
  await page.locator('#drawing-curves').check();
  await page.locator('#drawing-search').fill('Sample');
  await page.locator('#drawing-search-results button').first().click();
  await expect(page.locator('#selection-info')).toContainText('Source details');
  await page.locator('#drawing-search').fill('');
  await page.screenshot({ path: testInfo.outputPath('idw-viewer.png'), fullPage: true });
});

test('IDW unsupported profile retains previews and a clear unavailable state', async ({ page }) => {
  await open(page, 'drawings/iacs/Template_IACS.idw', ['--experimental-drawing']);
  await expect(page.locator('#empty')).toBeVisible();
  await expect(page.locator('#empty h2')).toHaveText('Drawing display unavailable');
  await expect(page.locator('#previews img')).not.toHaveCount(0);
  await expect(page.locator('#drawing-svg [data-item-id]')).toHaveCount(0);
});

test('IDW synthetic sheet switches clear stale selection and preserve literal multiline text', async ({ page }) => {
  const url = await open(page, 'SampleBg.idw', ['--experimental-drawing']);
  const state = await (await page.request.get(url + 'state.json')).json();
  const first = await storedSheet(page, url, state.drawing.sheets[0]);
  const second = structuredClone(first); second.id = 'synthetic-second'; second.name = 'Second';
  second.items = [structuredClone(first.items.find((i: any) => i.geometry.kind === 'text'))];
  second.items[0].id = 'synthetic-literal';
  second.items[0].geometry.text = '<script>window.injected=true</script>\n日本語';
  const third = { ...second, id: 'synthetic-unavailable', name: 'Unavailable', status: 'unavailable', items: [], size_in_source_units: null };
  await mockDrawingSheets(page, state, [first, second, third]);
  await page.reload();
  await expect(page.locator('#sheet-buttons button')).toHaveCount(3);
  await page.locator('#sheet-buttons button').nth(1).click();
  await expect(page.locator('#drawing-svg [data-item-id]')).toHaveCount(1);
  await expect(page.locator('#drawing-svg text tspan')).toHaveCount(2);
  await expect(page.locator('#drawing-svg text')).toHaveAttribute('data-font-sizing', 'unverified-em-fallback');
  expect(await page.evaluate(() => (window as any).injected)).toBeUndefined();
  await page.locator('#drawing-search').fill('script');
  await page.locator('#drawing-search-results button').click();
  await expect(page.locator('#selected')).toContainText('<script>');
  await page.locator('#sheet-buttons button').nth(2).click();
  await expect(page.locator('#drawing-svg [data-item-id]')).toHaveCount(0);
  await expect(page.locator('#selected')).toHaveText('No drawing element selected');
  await expect(page.locator('#empty h2')).toHaveText('Sheet display unavailable');
  for (let i = 0; i < 4; i++) {
    await page.locator('#sheet-buttons button').nth(i % 2).click();
  }
  await expect(page.locator('#drawing-svg [data-item-id]')).toHaveCount(1);
});

test('IDW capital-height candidates render at the stored height and preserve spaces', async ({ page }) => {
  const url = await open(page, 'SampleBg.idw', ['--experimental-drawing']);
  const state = await (await page.request.get(url + 'state.json')).json();
  const sheet = await storedSheet(page, url, state.drawing.sheets[0]);
  const original = sheet.items.find((i: any) => i.geometry.kind === 'text');
  sheet.size_in_source_units = [200, 120];
  sheet.items = [
    ['Arial', 10, 80, 'H', 1, 9], ['Tahoma', 70, 80, 'H', 1, 9],
    ['Arial', 130, 80, 'H', .5, 9], ['Arial', 10, 30, ' H ', 1, 9],
    ['Arial', 110, 30, 'H', 1, 10],
  ].map(([family, x, y, text, width, flags], index) => {
    const item = structuredClone(original); item.id = `synthetic-font-${index}`;
    Object.assign(item.geometry, { text, position: [x, y, 0], direction: [1, 0, 0], up: [0, 1, 0], raw_flags: flags });
    Object.assign(item.geometry.font, { family, height_candidate: 20, weight_candidate: 400, flags: 0, width_factor: width });
    return item;
  });
  await mockDrawingSheets(page, state, [sheet]);
  await page.reload();
  await expect(page.locator('#drawing-svg text')).toHaveCount(5);
  await expect(page.locator('#drawing-font-note')).toContainText('approximate');
  const result = await page.evaluate(async () => {
    await document.fonts.ready;
    const texts = Array.from(document.querySelectorAll<SVGTextElement>('#drawing-svg text'));
    const spaced = texts[3];
    const spaceAdvance = spaced.getStartPositionOfChar(1).x - spaced.getStartPositionOfChar(0).x;
    const trailingAdvance = spaced.getComputedTextLength() - spaced.getEndPositionOfChar(1).x;
    // Rasterize the actual Viewer SVG at 5 pixels/source unit. Measure ink,
    // independently of the browser's line-box bounds or font-size declaration.
    const copy = document.querySelector('#drawing-svg')!.cloneNode(true) as SVGSVGElement;
    copy.setAttribute('viewBox', '0 0 200 120'); copy.setAttribute('width', '1000'); copy.setAttribute('height', '600');
    const img = new Image(); img.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(new XMLSerializer().serializeToString(copy));
    await img.decode();
    const canvas = document.createElement('canvas'); canvas.width = 1000; canvas.height = 600;
    const ctx = canvas.getContext('2d')!; ctx.drawImage(img, 0, 0);
    const pixels = ctx.getImageData(0, 0, 1000, 600).data;
    const bounds = [0, 300, 600].map(start => {
      let x0 = 1000, y0 = 600, x1 = -1, y1 = -1;
      for (let y = 10; y < 300; y++) for (let x = start + 10; x < start + 290; x++) {
        const p = (y * 1000 + x) * 4;
        if (pixels[p + 3] && Math.max(pixels[p], pixels[p + 1], pixels[p + 2]) < 100) {
          x0 = Math.min(x0, x); x1 = Math.max(x1, x); y0 = Math.min(y0, y); y1 = Math.max(y1, y);
        }
      }
      return { width: x1 - x0 + 1, height: y1 - y0 + 1 };
    });
    return { bounds, spaceAdvance, trailingAdvance, unknownSizing: texts[4].dataset.fontSizing };
  });
  for (const box of result.bounds) expect(Math.abs(box.height - 100)).toBeLessThanOrEqual(2);
  expect(Math.abs(result.bounds[0].width / 2 - result.bounds[2].width)).toBeLessThanOrEqual(2);
  expect(result.spaceAdvance).toBeGreaterThan(0);
  expect(result.trailingAdvance).toBeGreaterThan(0);
  expect(result.unknownSizing).toBe('unverified-em-fallback');

  // An older browser keeps the explicit fallback and explains the limitation.
  await page.addInitScript(() => {
    const supports = CSS.supports.bind(CSS);
    CSS.supports = ((property: string, value?: string) => property === 'font-size-adjust' ? false : supports(property, value!)) as typeof CSS.supports;
  });
  await page.reload();
  await expect(page.locator('#drawing-font-note')).toContainText('cannot adjust text height');
  await expect(page.locator('#drawing-svg text').first()).toHaveAttribute('data-font-sizing', 'unverified-em-fallback');
});

test('IDW Tahoma bold and italic retain capital height and change the rendered ink', async ({ page }) => {
  const url = await open(page, 'SampleBg.idw', ['--experimental-drawing']);
  const state = await (await page.request.get(url + 'state.json')).json();
  const sheet = await storedSheet(page, url, state.drawing.sheets[0]);
  const original = sheet.items.find((i: any) => i.geometry.kind === 'text');
  sheet.size_in_source_units = [160, 60];
  sheet.items = [[400, 0], [700, 0], [400, 1]].map(([weight, flags], index) => {
    const item = structuredClone(original); item.id = `synthetic-style-${index}`;
    Object.assign(item.geometry, { text: 'H', position: [10 + index * 50, 30, 0], direction: [1, 0, 0], up: [0, 1, 0], raw_flags: 9 });
    Object.assign(item.geometry.font, { family: 'Tahoma', height_candidate: 20, weight_candidate: weight, flags, width_factor: 1 });
    return item;
  });
  await mockDrawingSheets(page, state, [sheet]); await page.reload();
  await expect(page.locator('#drawing-svg text')).toHaveCount(3);
  const boxes = await page.evaluate(async () => {
    await document.fonts.ready;
    const copy = document.querySelector('#drawing-svg')!.cloneNode(true) as SVGSVGElement;
    copy.setAttribute('viewBox', '0 0 160 60'); copy.setAttribute('width', '800'); copy.setAttribute('height', '300');
    const img = new Image(); img.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(new XMLSerializer().serializeToString(copy));
    await img.decode();
    const canvas = document.createElement('canvas'); canvas.width = 800; canvas.height = 300;
    const ctx = canvas.getContext('2d')!; ctx.drawImage(img, 0, 0);
    const pixels = ctx.getImageData(0, 0, 800, 300).data;
    return [0, 250, 500].map(start => {
      let x0 = 800, x1 = -1, y0 = 300, y1 = -1, ink = 0;
      for (let y = 10; y < 250; y++) for (let x = start + 10; x < start + 240; x++) {
        const p = (y * 800 + x) * 4;
        if (pixels[p + 3] && Math.max(pixels[p], pixels[p + 1], pixels[p + 2]) < 100) {
          x0 = Math.min(x0, x); x1 = Math.max(x1, x); y0 = Math.min(y0, y); y1 = Math.max(y1, y); ink++;
        }
      }
      return { width: x1 - x0 + 1, height: y1 - y0 + 1, ink };
    });
  });
  for (const box of boxes) expect(Math.abs(box.height - 100)).toBeLessThanOrEqual(2);
  expect(boxes[1].ink).toBeGreaterThan(boxes[0].ink);
  expect(boxes[2].width).toBeGreaterThan(boxes[0].width);
});

test('IDW loads only the selected sheet and rejects stale responses after switching', async ({ page }) => {
  const url = await open(page, 'SampleBg.idw', ['--experimental-drawing']);
  const state = await (await page.request.get(url + 'state.json')).json();
  const first = await storedSheet(page, url, state.drawing.sheets[0]);
  const second = structuredClone(first); second.id = 'race-second'; second.name = 'Second';
  second.items = first.items.filter((i: any) => i.geometry.kind === 'text').slice(0, 1);
  await mockDrawingSheets(page, state, [first, second]);
  let release!: () => void, entered!: () => void;
  const held = new Promise<void>(resolve => { release = resolve; });
  const requested = new Promise<void>(resolve => { entered = resolve; });
  const requests: string[] = [];
  page.on('request', request => { if (request.url().includes('drawing-sheet-')) requests.push(request.url()); });
  await page.route(`**/${state.drawing.sheets[0].resource}`, async route => {
    entered(); await held;
    // Switching cancels this request. A late response must never replace Second.
    try { await route.fallback(); } catch { /* Aborted by the sheet switch. */ }
  });
  await page.reload(); await requested;
  expect(requests).toHaveLength(1);
  expect(requests[0]).toContain(state.drawing.sheets[0].resource);
  await page.locator('#sheet-buttons button').nth(1).click();
  await expect(page.locator('#drawing-svg [data-item-id]')).toHaveCount(1);
  release();
  await expect(page.locator('#cad')).toHaveAttribute('data-sheet-id', state.drawing.sheets[1].id);
  await expect(page.locator('#render-status')).toContainText('Second');
  expect(requests.some(url => url.endsWith(state.drawing.sheets[1].resource))).toBe(true);
});

test('IDW damaged sheet resources fail closed and retain sheet diagnostics', async ({ page }) => {
  const url = await open(page, 'SampleBg.idw', ['--experimental-drawing']);
  const state = await (await page.request.get(url + 'state.json')).json();
  await page.route(`**/${state.drawing.sheets[0].resource}`, route => route.fulfill({ body: '{}', contentType: 'application/json' }));
  await page.reload();
  await expect(page.locator('#empty h2')).toHaveText('Sheet display unavailable');
  await expect(page.locator('#drawing-svg [data-item-id]')).toHaveCount(0);
  await expect(page.locator('#drawing-omissions')).toContainText('Sheet resource');
});


test('IDW diameter glyph fallback keeps source text and supports symbol search', async ({ page }) => {
  const url = await open(page, 'SampleBg.idw', ['--experimental-drawing']);
  const state = await (await page.request.get(url + 'state.json')).json();
  const sheet = await storedSheet(page, url, state.drawing.sheets[0]);
  const item = structuredClone(sheet.items.find((i: any) => i.geometry.kind === 'text'));
  Object.assign(item.geometry, { text: 'n', raw_flags: 9,
    font: { ...item.geometry.font, family: 'AIGDT', height_candidate: .45, flags: 0, weight_candidate: 400 } });
  sheet.items = [item];
  await mockDrawingSheets(page, state, [sheet]);
  await page.reload();
  await expect(page.locator('#drawing-svg text')).toHaveText('⌀');
  await expect(page.locator('#drawing-svg text')).toHaveAttribute('data-raw-text', 'n');
  await expect(page.locator('#drawing-symbol-note')).toContainText('Unicode substitute');
  await page.locator('#drawing-search').fill('⌀');
  await page.locator('#drawing-search-results button').click();
  await expect(page.locator('#selected')).toHaveText('⌀');
  await expect(page.locator('#selection-info')).toContainText('"text": "n"');
  await page.locator('#drawing-search').fill('n');
  await expect(page.locator('#drawing-search-results button')).toHaveCount(1);
});

test('IDW saved view selection highlights members and retains unknown rotation', async ({ page }) => {
  const url = await open(page, 'SampleBg.idw', ['--experimental-drawing']);
  const state = await (await page.request.get(url + 'state.json')).json();
  const sheet = await storedSheet(page, url, state.drawing.sheets[0]);
  sheet.views = [{ name: 'Synthetic view', placement_record: sheet.items[0].placement_record,
    rotation: null, view_type: null, parent_view_id: null, image_reference: null }];
  await mockDrawingSheets(page, state, [sheet]);
  await page.reload();
  await page.locator('#drawing-views button').click();
  await expect(page.locator('#selected')).toHaveText('Synthetic view');
  await expect(page.locator('#selection-info')).toContainText('"rotation": null');
  expect(await page.locator('#drawing-svg .drawing-selected').count()).toBeGreaterThan(0);
  await page.locator('#drawing-search').fill('does not exist');
  await expect(page.locator('#drawing-views button')).toHaveCount(1);
});
