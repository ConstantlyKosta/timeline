import { expect, test } from '@playwright/test';
import { boot, nav, newNote, shot } from './helpers';

test('timeline: Month → Week → Day zoom with clusters', async ({ page }) => {
  await boot(page);
  await nav(page, 'timeline');
  await page.getByTestId('zoom-month').click();
  await expect(page.getByTestId('timeline')).toHaveAttribute('data-level', 'month');
  await expect.poll(() => page.getByTestId('entry').count().then(async (n) => n + (await page.getByTestId('cluster').count()))).toBeGreaterThan(0);
  await page.waitForTimeout(500);
  await shot(page, '01-timeline-month');
  const monthBlocks = (await page.getByTestId('entry').count()) + (await page.getByTestId('cluster').count());

  await page.getByTestId('today').click();
  await page.getByTestId('zoom-week').click();
  await expect(page.getByTestId('timeline')).toHaveAttribute('data-level', 'week');
  await page.waitForTimeout(600);
  await shot(page, '02-timeline-week');

  await page.getByTestId('zoom-day').click();
  await expect(page.getByTestId('timeline')).toHaveAttribute('data-level', 'day');
  await page.waitForTimeout(600);
  await shot(page, '03-timeline-day');
  const dayEntries = await page.getByTestId('entry').count();
  expect(dayEntries).toBeGreaterThan(0);
  console.log(`blocks: month=${monthBlocks} day-entries=${dayEntries}`);
});

test('timeline: ctrl+wheel zoom and cluster click zooms in', async ({ page }) => {
  await boot(page);
  await nav(page, 'timeline');
  await page.getByTestId('zoom-month').click();
  await page.waitForTimeout(500);
  const before = await page.getByTestId('timeline').getAttribute('data-level');
  const box = (await page.getByTestId('tl-surface').boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + 300);
  await page.keyboard.down('Control');
  for (let i = 0; i < 12; i++) { await page.mouse.wheel(0, -120); await page.waitForTimeout(30); }
  await page.keyboard.up('Control');
  await page.waitForTimeout(300);
  const after = await page.getByTestId('timeline').getAttribute('data-level');
  console.log(`level ${before} -> ${after}`);
  expect(after).not.toBe('month');
  await page.getByTestId('today').click();
  await page.waitForTimeout(400);
});

test('notes: live preview, tasks, tables, code, wiki-link autocomplete, tags', async ({ page }) => {
  await boot(page);
  await newNote(page);
  const ed = page.locator('.cm-content');
  await page.keyboard.insertText('# Trip planning\n\nSome **bold**, *italic* and ~~struck~~ text with `code` and a #travel tag.\n\n- [ ] Book flights\n- [x] Renew passport\n\n| City | Days |\n| --- | ---: |\n| Lisbon | 4 |\n| Porto | 2 |\n\n```ts\nconst answer: number = 42; // typed\nfunction hi() { return "hi"; }\n```\n\n> A quote\n\n');
  // wiki link autocomplete (typed for real so closeBrackets + completion run)
  await page.keyboard.type('See [[Lis');
  await expect(page.locator('.cm-tooltip-autocomplete')).toBeVisible();
  await expect(page.locator('.cm-tooltip-autocomplete li').first()).toContainText('Lisbon');
  await shot(page, '04-editor-wikilink-autocomplete');
  await page.keyboard.press('Enter');
  await page.keyboard.type(' and #tra');
  await expect(page.locator('.cm-tooltip-autocomplete')).toBeVisible();
  await shot(page, '05-editor-tag-autocomplete');
  await page.keyboard.press('Escape');
  await page.keyboard.type(' ');
  // move the cursor to the very top so every block is rendered, not editing
  await page.keyboard.press('Control+Home');
  await page.waitForTimeout(700);
  await expect(page.locator('.cm-h1')).toContainText('Trip planning');
  await expect(page.locator('.cm-strong')).toHaveText('bold');
  await expect(page.locator('.cm-strike')).toHaveText('struck');
  await expect(page.locator('.cm-tag').first()).toContainText('travel');
  await expect(page.getByTestId('table-widget')).toBeVisible();
  await expect(page.getByTestId('table-widget').locator('tbody tr')).toHaveCount(2);
  await expect(page.locator('.cm-wikilink-rendered')).toContainText('Lisbon Trip');
  expect(await page.locator('[data-testid="task-box"], .cm-task-box').count()).toBe(2);
  await expect(page.locator('.cm-codeblock [class*="tok-"]').first()).toBeVisible();
  await page.waitForTimeout(700);
  await expect(page.getByTestId('save-state')).toHaveText('Saved');
  await shot(page, '06-editor-live-preview');

  // toggle the first task via the checkbox widget
  await page.locator('.cm-task-box').first().click();
  await page.waitForTimeout(800);
  await expect(page.locator('.cm-task-done')).toHaveCount(2);
  await expect(page.getByTestId('context-panel')).toContainText('2/2');
  await shot(page, '07-editor-tasks-context');

  // source reveal: clicking into the table shows markdown source
  await page.getByTestId('table-widget').click();
  await expect(page.locator('.cm-table-src').first()).toBeVisible();
  await shot(page, '08-editor-table-source');
  void ed;
});

test('notes: slash menu inserts a table and a heading', async ({ page }) => {
  await boot(page);
  await newNote(page);
  await page.keyboard.type('/tab');
  await expect(page.locator('.cm-tooltip-autocomplete')).toBeVisible();
  await shot(page, '09-editor-slash-menu');
  await page.keyboard.press('Enter');
  await page.keyboard.press('Control+End');
  await page.keyboard.insertText('\n\nText after the table.');
  await page.waitForTimeout(600);
  await shot(page, '09b-editor-slash-table');
  await expect(page.getByTestId('table-widget')).toBeVisible();
});

test('notes: backlinks, persistence and quick switcher', async ({ page }) => {
  await boot(page);
  await nav(page, 'notes');
  await page.getByTestId('note-item').filter({ hasText: 'Lisbon Trip' }).first().click();
  await expect(page.getByTestId('context-panel')).toBeVisible();
  await page.waitForTimeout(500);
  const count = Number(await page.getByTestId('backlink-count').innerText());
  console.log('Lisbon Trip backlinks', count);
  expect(count).toBeGreaterThan(0);
  await shot(page, '10-notes-backlinks');

  // persistence across reload
  await page.reload();
  await nav(page, 'notes');
  await expect(page.getByTestId('note-item').filter({ hasText: 'Trip planning' })).toHaveCount(1);

  await page.keyboard.press('Control+k');
  await expect(page.getByTestId('switcher-input')).toBeVisible();
  await page.keyboard.type('trip');
  await page.waitForTimeout(300);
  await shot(page, '11-quick-switcher');
  await page.keyboard.press('Enter');
  await expect(page.getByTestId('note-pane')).toBeVisible();
});

test('notes: pasted image is stored and rendered', async ({ page }) => {
  await boot(page);
  await newNote(page);
  await page.evaluate(async () => {
    const c = document.createElement('canvas'); c.width = 120; c.height = 60;
    const g = c.getContext('2d')!; g.fillStyle = '#3b82f6'; g.fillRect(0, 0, 120, 60); g.fillStyle = '#fff'; g.font = '20px sans-serif'; g.fillText('pasted', 20, 38);
    const blob: Blob = await new Promise((r) => c.toBlob((b) => r(b!), 'image/png'));
    const dt = new DataTransfer(); dt.items.add(new File([blob], 'pasted.png', { type: 'image/png' }));
    document.querySelector('.cm-content')!.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
  });
  await expect(page.locator('.cm-content')).toContainText('media:'); // import is async; caret ends up after the image
  await page.keyboard.insertText('\n\nThe image above was pasted.');
  await expect(page.locator('img.cm-image')).toBeVisible({ timeout: 8000 });
  await expect.poll(() => page.locator('img.cm-image').evaluate((i: HTMLImageElement) => i.naturalWidth)).toBeGreaterThan(0);
  await shot(page, '12-editor-pasted-image');
});

test('mind map renders notes, links, tags and ghost nodes', async ({ page }) => {
  await boot(page);
  await nav(page, 'graph');
  const gv = page.getByTestId('graph-view');
  await expect(gv).toHaveAttribute('data-node-count', /^([4-9]|\d\d+)$/);
  await expect.poll(() => page.evaluate(() => (window as any).__tlGraph?.ids().length)).toBeGreaterThan(3);
  await expect(page.getByTestId('graph-legend')).toBeVisible();
  console.log(`graph nodes=${await gv.getAttribute('data-node-count')} edges=${await gv.getAttribute('data-edge-count')}`);
  await page.waitForTimeout(600);
  await shot(page, '13-mind-map');
});

test('search finds notes by meaning', async ({ page }) => {
  await boot(page);
  await nav(page, 'search');
  await page.getByTestId('search-input').fill('feeling worn out and exhausted');
  await expect(page.getByTestId('search-hit').first()).toBeVisible();
  console.log('top hit:', await page.getByTestId('search-hit').first().innerText());
  await shot(page, '14-search');
});

test('library: audio player plays and transcript seeks', async ({ page }) => {
  await boot(page);
  await nav(page, 'library');
  await page.getByTestId('media-item').first().click();
  await expect(page.getByTestId('audio-player')).toHaveAttribute('data-ready', 'true', { timeout: 15000 });
  await expect(page.locator('[data-testid="waveform"] canvas, [data-testid="waveform"] div').first()).toBeVisible();
  await page.getByTestId('play').click();
  await expect(page.getByTestId('audio-player')).toHaveAttribute('data-playing', 'true');
  await page.waitForTimeout(1600);
  const clock = await page.getByTestId('clock').innerText();
  console.log('clock', clock);
  expect(clock.startsWith('0:00 /')).toBeFalsy();
  const segs = page.getByTestId('segment');
  expect(await segs.count()).toBeGreaterThan(1);
  await segs.nth(2).locator('button').click();
  await page.waitForTimeout(500);
  await shot(page, '15-audio-player');
  await page.getByTestId('play').click();
});

test('dark theme + narrow (phone) layout', async ({ page }) => {
  await boot(page);
  await page.getByTestId('theme').click();
  await nav(page, 'timeline');
  await page.waitForTimeout(500);
  await shot(page, '16-dark-timeline');
  await page.setViewportSize({ width: 390, height: 780 });
  await page.waitForTimeout(500);
  await shot(page, '17-phone-timeline');
  await nav(page, 'notes');
  await page.waitForTimeout(300);
  await shot(page, '18-phone-notes');
});
