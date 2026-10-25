import { expect, test, type Page } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { boot, nav, shot } from './helpers';

// Runs last (file name) because it imports a ~110 note vault into the shared test database.
test.describe.configure({ mode: 'serial' });

/* eslint-disable @typescript-eslint/no-explicit-any */
const G = (page: Page) => ({
  ids: () => page.evaluate(() => (window as any).__tlGraph.ids() as string[]),
  pos: (id: string) => page.evaluate((i) => (window as any).__tlGraph.screenPos(i) as { x: number; y: number }, id),
  attrs: (id: string) => page.evaluate((i) => (window as any).__tlGraph.attrs(i) as Record<string, any>, id),
});
const count = async (page: Page) => Number(await page.getByTestId('graph-view').getAttribute('data-node-count'));
const settle = async (page: Page) => { await expect(page.getByTestId('graph-view')).toHaveAttribute('data-settled', '1'); await page.waitForTimeout(500); };
async function idOf(page: Page, title: string) {
  const ids = await G(page).ids();
  for (const id of ids) if ((await G(page).attrs(id)).label.replace('📌 ', '') === title) return id;
  throw new Error(`no node ${title}`);
}

const open = async (page: Page, id: string) => {
  const d = page.getByTestId(`gp-${id}`);
  if (!(await d.evaluate((e: HTMLDetailsElement) => e.open))) await d.locator('summary').click();
};

let vault = '';
test.beforeAll(() => {
  vault = fs.mkdtempSync(path.join(os.tmpdir(), 'bigvault-'));
  execFileSync('node', [path.resolve(process.cwd(), '../fixtures/gen-big-vault.mjs'), vault]);
});
test.afterAll(() => fs.rmSync(vault, { recursive: true, force: true }));

test('big vault: import, render, hover preview, search, filters', async ({ page, request }) => {
  await boot(page);
  const { jobId } = await (await request.post('/api/import_start', { data: { paths: [vault], tzOffsetMinutes: 120 } })).json();
  await expect.poll(async () => (await (await request.post('/api/job_status', { data: { jobId } })).json()).state, { timeout: 40_000 }).toBe('done');
  await page.reload();
  await nav(page, 'graph');
  await expect.poll(() => count(page), { timeout: 15_000 }).toBeGreaterThan(100);
  await settle(page);
  await expect(page.getByTestId('graph-canvas').locator('canvas').first()).toBeVisible();
  await shot(page, '30-mind-map-overview');

  // hover shows the preview card
  const aurora = await idOf(page, 'Aurora');
  await expect(async () => {
    const p = await G(page).pos(aurora);
    await page.mouse.move(5, 5);
    await page.mouse.move(p.x, p.y);
    await expect(page.getByTestId('graph-tooltip')).toContainText('Aurora', { timeout: 1500 });
  }).toPass({ timeout: 10_000 });
  await expect(page.getByTestId('graph-tooltip')).toContainText('Hub for the Aurora project');
  await shot(page, '31-mind-map-hover');
  await page.mouse.move(5, 5);

  // search highlights without removing anything
  const before = await count(page);
  await page.getByTestId('graph-search').fill('tag:book');
  await expect(page.getByTestId('graph-match-count')).toContainText('12 matches');
  expect(await count(page)).toBe(before);
  await page.waitForTimeout(300);
  await shot(page, '32-mind-map-search');
  await page.getByTestId('graph-search').fill('');

  // filters
  await open(page, 'filters');
  await page.getByTestId('gp-orphans').uncheck();
  await expect.poll(() => count(page)).toBeLessThan(before);
  await page.getByTestId('gp-orphans').check();
  await page.getByTestId('gp-unresolved').uncheck();
  await expect.poll(() => count(page)).toBeLessThan(before);
  await page.getByTestId('gp-unresolved').check();
  await expect.poll(() => count(page)).toBe(before);
});

test('big vault: colour by folder, shapes by tag, groups, curved links, legend', async ({ page }) => {
  await boot(page);
  await nav(page, 'graph');
  await expect.poll(() => count(page)).toBeGreaterThan(100);
  await settle(page);
  await open(page, 'display');
  await open(page, 'groups');
  await page.getByTestId('gp-colorby').selectOption('folder');
  await page.getByTestId('gp-shapeby').selectOption('tag');
  await page.getByTestId('gp-curved').check();
  await page.getByTestId('gp-add-group').click();
  await page.getByTestId('gp-group-query').fill('kind:ghost');
  await page.getByTestId('gp-sizeby').selectOption('pagerank');
  await page.waitForTimeout(500);
  await expect(page.getByTestId('graph-legend')).toContainText('Projects');
  await expect.poll(() => G(page).ids().then((ids) => G(page).attrs(ids[0]).then((a) => a.type))).toMatch(/circle|shape|thumb/);
  await shot(page, '33-mind-map-folders-shapes');
  await page.getByTestId('gp-curved').uncheck();
  await page.getByTestId('gp-colorby').selectOption('kind');
  await page.getByTestId('gp-shapeby').selectOption('kind');
  await page.getByTestId('gp-sizeby').selectOption('degree');
});

test('big vault: local graph, pin by drag persists, views, forces', async ({ page }) => {
  await boot(page);
  await nav(page, 'graph');
  await expect.poll(() => count(page)).toBeGreaterThan(100);
  await settle(page);
  const total = await count(page);

  // local graph from the right-click menu
  const beacon = await idOf(page, 'Beacon');
  const p = await G(page).pos(beacon);
  await page.mouse.click(p.x, p.y, { button: 'right' });
  await expect(page.getByTestId('context-menu')).toBeVisible();
  await page.getByTestId('gm-local').click();
  await expect.poll(() => count(page)).toBeLessThan(total * 0.6);
  await expect(page.getByTestId('graph-depth')).toBeVisible();
  await page.waitForTimeout(700);
  await shot(page, '34-mind-map-local-graph');
  await page.getByTestId('graph-local').click();
  await expect.poll(() => count(page)).toBe(total);
  await settle(page);

  // drag a node: it pins and the position survives a reload
  const delta = await idOf(page, 'Delta');
  const d0 = await G(page).pos(delta);
  await page.mouse.move(d0.x, d0.y);
  await page.mouse.down();
  await page.mouse.move(d0.x + 60, d0.y + 40, { steps: 6 });
  await page.mouse.up();
  await expect(page.getByTestId('graph-unpin')).toBeEnabled();
  const moved = await G(page).attrs(delta);
  await expect.poll(async () => {
    const r = await (await page.request.post('/api/graph_positions', { data: {} })).json();
    const list = (r.result ?? r) as { id: string; x: number; y: number }[];
    const q = list.find((n) => n.id === delta);
    return q ? Math.abs(q.x - moved.x) < 1 && Math.abs(q.y - moved.y) < 1 : false;
  }, { timeout: 10_000 }).toBe(true); // debounced position save has landed
  await page.reload();
  await nav(page, 'graph');
  await expect.poll(() => count(page)).toBe(total);
  await settle(page);
  const again = await G(page).attrs(await idOf(page, 'Delta'));
  expect(Math.abs(again.x - moved.x)).toBeLessThan(1);
  expect(Math.abs(again.y - moved.y)).toBeLessThan(1);
  await page.getByTestId('graph-unpin').click();

  // saved view round trip
  await open(page, 'display');
  await page.getByTestId('gp-colorby').selectOption('tag');
  await page.getByTestId('view-name').fill('By tag');
  await page.getByTestId('view-save').click();
  await expect(page.getByTestId('view-By tag')).toBeVisible();
  await page.getByTestId('gp-colorby').selectOption('none');
  await page.getByTestId('view-By tag').click();
  await expect(page.getByTestId('gp-colorby')).toHaveValue('tag');

  // forces sliders are live
  const slider = page.getByTestId('gp-repel');
  await slider.fill('200');
  await expect(slider).toHaveValue('200');

  // clicking a node opens the note
  await page.waitForTimeout(1500); // the slider re-heated the simulation; let it cool
  await settle(page);
  const dp = await G(page).pos(await idOf(page, 'Beacon'));
  await page.mouse.click(dp.x, dp.y);
  await expect(page.locator('.cm-editor')).toBeVisible();
  // (nodes can overlap, so we only assert that *a* note opened, not which one)
  await expect(page.locator('.cm-content')).not.toHaveText('');
});

test('big vault: dark theme screenshot', async ({ page }) => {
  await boot(page);
  await page.getByTestId('theme').click();
  await nav(page, 'graph');
  await expect.poll(() => count(page)).toBeGreaterThan(100);
  await settle(page);
  await page.getByTestId('preset-Gallery').click();
  await page.waitForTimeout(800);
  await shot(page, '35-mind-map-dark-gallery');
});
