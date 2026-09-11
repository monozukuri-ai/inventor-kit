import { test, expect, type Page } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import { resolve } from 'node:path';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';

const root = resolve(import.meta.dirname, '../..');
const python = process.env.VIEWER_PYTHON || resolve(root, '.venv/bin/python');
const corpus = process.env.VIEWER_CORPUS || resolve(root, 'fixtures/public');
let process_: ChildProcess | undefined;
let errors: string[] = [];

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

for (const file of ['SamplePart.ipt', 'Cylinder.ipt', 'INV_nist_ftc_09_asme1_2024.ipt']) {
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

test('broken input reports failure without a blank page', async ({ page }) => {
  const directory = mkdtempSync(resolve(tmpdir(), 'inventor-broken-'));
  try {
    const path = resolve(directory, 'broken.ipt'); writeFileSync(path, 'invalid');
    await open(page, path);
    await expect(page.locator('#diagnostics')).not.toBeEmpty();
    await expect(page.getByRole('heading', { name: 'No 3D geometry to display' })).toBeVisible();
  } finally { rmSync(directory, { recursive: true, force: true }); }
});
