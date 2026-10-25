import { expect, test } from '@playwright/test';
import { boot, nav, shot } from './helpers';

test('timeline: right-click empty space → New note here (dated at the clicked time)', async ({ page }) => {
  await boot(page);
  await nav(page, 'timeline');
  await page.getByTestId('zoom-week').click();
  await page.getByTestId('today').click();
  await page.waitForTimeout(600);
  const box = (await page.getByTestId('tl-surface').boundingBox())!;
  const centreText = await page.getByTestId('centre-date').innerText();
  await page.mouse.click(box.x + box.width / 2, box.y + box.height - 40, { button: 'right' });
  await expect(page.getByTestId('context-menu')).toBeVisible();
  await expect(page.getByTestId('ctx-open')).toHaveCount(0);
  await shot(page, '22-timeline-context-menu-empty');
  await page.getByTestId('ctx-new-note').click();
  await expect(page.locator('.cm-editor.cm-focused')).toBeVisible();
  // the note's date is the day under the cursor (centre of the viewport)
  const value = await page.locator('input[type="datetime-local"]').inputValue();
  const [y, mo, d] = value.slice(0, 10).split('-').map(Number);
  const shown = new Date(y, mo - 1, d).toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric' });
  expect(shown).toBe(centreText);
});

const visibleNote = async (page: import('@playwright/test').Page) => {
  const id = await page.locator('[data-testid="entry"][data-kind="note"]').evaluateAll((els) => {
    const s = document.querySelector('[data-testid="tl-surface"]')!.getBoundingClientRect();
    const e = els.find((el) => { const r = el.getBoundingClientRect(); return r.left > s.left + 20 && r.right < s.right - 20 && r.top >= s.top && r.bottom <= s.bottom; });
    return e ? (e as HTMLElement).dataset.entryId! : null;
  });
  expect(id).not.toBeNull();
  return page.locator(`[data-entry-id="${id}"]`);
};

test('timeline: right-click an item → Open / Delete with confirmation', async ({ page }) => {
  await boot(page);
  await nav(page, 'timeline');
  await page.getByTestId('zoom-week').click();
  await page.getByTestId('today').click();
  await page.waitForTimeout(600);
  const entries = page.getByTestId('entry');
  const before = await entries.count();
  expect(before).toBeGreaterThan(0);
  const target = await visibleNote(page);
  const title = await target.getAttribute('data-title');
  await target.click({ button: 'right' });
  await expect(page.getByTestId('context-menu')).toContainText('Open');
  await expect(page.getByTestId('ctx-delete')).toBeVisible();
  await shot(page, '23-timeline-context-menu-item');
  await page.getByTestId('ctx-delete').click();
  await expect(page.getByTestId('confirm-delete')).toContainText(title ?? '');
  await page.getByTestId('delete-cancel').click();
  await expect(page.getByTestId('confirm-delete')).toHaveCount(0);
  expect(await entries.count()).toBe(before);
  await target.click({ button: 'right' });
  await page.getByTestId('ctx-delete').click();
  await page.getByTestId('delete-confirm').click();
  await expect(page.getByTestId('toast')).toContainText('Deleted');
  await expect.poll(() => entries.count()).toBeLessThan(before);
  // open from the menu
  const next = await visibleNote(page);
  await next.click({ button: 'right' });
  await page.getByTestId('ctx-open').click();
  await expect(page.locator('.cm-editor')).toBeVisible();
});

test('timeline: week cards show first-image / photo thumbnails when there is room', async ({ page }) => {
  await boot(page);
  await nav(page, 'notes');
  // the sample photo "Alfama tiles" is ~39 days old; the timeline restores this saved position when it mounts
  await page.evaluate(() => (window as unknown as { __tl: { setTl(t: { ppd: number; centerMs: number }): void } }).__tl.setTl({ ppd: 130, centerMs: Date.now() - 39 * 86400000 }));
  await nav(page, 'timeline');
  await page.waitForTimeout(700);
  await expect(page.locator('.pcard img.pthumb-bg, .pcard img.pthumb').first()).toBeVisible();
  await shot(page, '27-timeline-week-thumbnails');
  // at month zoom the same photo is a smaller chip without a thumbnail
  await page.getByTestId('zoom-month').click();
  await page.waitForTimeout(700);
  await expect(page.locator('.pcard img')).toHaveCount(0);
});

test('timeline: week/month previews adapt to zoom and crowding; thousands of notes stay virtualised', async ({ page, request }) => {
  await boot(page);
  // a crowded fortnight (60 notes within two days) plus 600 spread over a year
  const now = Date.now();
  const mk = async (i: number, date: number) => request.post('/api/create_note', { data: { body: `# Crowd note ${i}\n\nSome words about topic ${i} that make a preview.\nSecond line ${i}.`, date } });
  await Promise.all(Array.from({ length: 60 }, (_, i) => mk(i, now - 12 * 86400000 + i * 20 * 60000)));
  await Promise.all(Array.from({ length: 7 }, (_, i) => mk(500 + i, now - 5 * 86400000 + i * 3 * 3600000))); // a moderately busy day
  for (let k = 0; k < 6; k++) await Promise.all(Array.from({ length: 100 }, (_, i) => mk(1000 + k * 100 + i, now - (10 + ((k * 100 + i) * 365) / 600) * 86400000)));
  await page.reload();
  await nav(page, 'timeline');
  await page.getByTestId('zoom-week').click();
  await page.getByTestId('today').click();
  await page.waitForTimeout(700);
  { const b = (await page.getByTestId('tl-surface').boundingBox())!; await page.mouse.move(b.x + 100, b.y + b.height - 60); await page.mouse.down(); await page.mouse.move(b.x + 700, b.y + b.height - 60, { steps: 12 }); await page.mouse.up(); }
  await page.waitForTimeout(700);
  const tiers = async () => page.locator('[data-testid="entry"]').evaluateAll((els) => els.reduce<Record<string, number>>((a, e) => { const t = (e as HTMLElement).dataset.tier ?? 'plain'; a[t] = (a[t] ?? 0) + 1; return a; }, {}));
  const week = await tiers();
  console.log('week tiers', JSON.stringify(week), 'clusters', await page.getByTestId('cluster').count());
  await shot(page, '24-timeline-week-previews');
  expect((week.rich ?? 0) + (week.medium ?? 0) + (week.title ?? 0)).toBeGreaterThan(0);
  expect(week.medium ?? 0).toBeGreaterThan(0); // 7 notes in one day: richest cards no longer fit, so they are demoted
  // cards never extend below the visible area
  const area = (await page.getByTestId('tl-surface').boundingBox())!;
  const bottoms = await page.locator('[data-testid="entry"],[data-testid="cluster"]').evaluateAll((els) => els.map((e) => e.getBoundingClientRect().bottom));
  expect(Math.max(...bottoms)).toBeLessThanOrEqual(area.y + area.height + 1);
  await page.getByTestId('zoom-month').click();
  await page.waitForTimeout(700);
  const month = await tiers();
  console.log('month tiers', JSON.stringify(month), 'clusters', await page.getByTestId('cluster').count());
  await shot(page, '25-timeline-month-previews');
  expect(await page.getByTestId('cluster').count() + (month.dot ?? 0)).toBeGreaterThan(0); // the 60-note burst collapsed to counts
  // DOM stays small although 700+ notes exist
  expect(await page.locator('.tl-content > *').count()).toBeLessThan(400);
  // dark theme
  await page.getByTestId('theme').click();
  await page.getByTestId('zoom-week').click();
  await page.waitForTimeout(700);
  await shot(page, '26-timeline-week-previews-dark');
});
