import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { boot, nav, shot } from './helpers';

const VAULT = path.resolve(process.cwd(), '../fixtures/vault');

test('import: a folder (Obsidian vault) via the folder picker, with summary', async ({ page }) => {
  await boot(page);
  await nav(page, 'notes');
  const before = await page.getByTestId('note-list').getAttribute('data-total');
  await page.getByTestId('import-folder-input').setInputFiles(VAULT);
  await expect(page.getByTestId('import-summary')).toBeVisible();
  await expect(page.getByTestId('sum-imported')).toContainText('7');
  await expect(page.getByTestId('sum-duplicates')).toContainText('1');
  await expect(page.getByTestId('sum-errors')).toContainText('1');
  await expect(page.getByTestId('import-summary')).toContainText('Obsidian vault detected');
  await shot(page, '21-import-summary');
  await page.getByTestId('import-open-first').click();
  await expect(page.locator('.cm-editor')).toBeVisible();
  const after = await page.getByTestId('note-list').getAttribute('data-total');
  expect(Number(after) - Number(before)).toBe(7);
  // front matter title, folder, wiki links resolve
  await page.getByTestId('note-filter').fill('Atlas');
  await expect(page.getByTestId('note-item').first()).toContainText('Projects');
  await page.getByTestId('note-filter').fill('');
  // importing again: everything is skipped as already imported
  await page.getByTestId('import-folder-input').setInputFiles(VAULT);
  await expect(page.getByTestId('sum-imported')).toContainText('0');
  await expect(page.getByTestId('sum-duplicates')).toContainText('8');
  await page.getByTestId('import-done').click();
});

test('import: native-path job (what the desktop app uses) with attachments, progress API', async ({ page, request }) => {
  await boot(page);
  // copy the vault and touch each note so content hashes differ from the previous test
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'vault-'));
  fs.cpSync(VAULT, tmp, { recursive: true });
  const walk = (d: string) => fs.readdirSync(d, { withFileTypes: true }).forEach((e) => {
    const p = path.join(d, e.name);
    if (e.isDirectory()) walk(p); else if (e.name.endsWith('.md') && e.name !== 'binary.md') fs.appendFileSync(p, '\n\nSecond copy marker.\n');
  });
  walk(tmp);
  const { jobId } = await (await request.post('/api/import_start', { data: { paths: [tmp], tzOffsetMinutes: 120 } })).json();
  let st: { state: string; result: Record<string, unknown> } = { state: 'running', result: {} };
  await expect.poll(async () => { st = await (await request.post('/api/job_status', { data: { jobId } })).json(); return st.state; }).toBe('done');
  expect(st.result).toMatchObject({ imported: 7, skippedDuplicate: 1, errorCount: 1, attachmentsImported: 2 });
  fs.rmSync(tmp, { recursive: true, force: true });
  await page.reload();
  await nav(page, 'notes');
  await page.getByTestId('note-filter').fill('Atlas');
  await page.getByTestId('note-item').first().click();
  await expect(page.locator('.cm-editor')).toBeVisible();
});
