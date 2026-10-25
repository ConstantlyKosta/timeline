import { expect, test } from '@playwright/test';
import { boot, nav, newNote, shot } from './helpers';

test('note mind map (Markmap outline): headings + bullets, follows typing, click jumps the editor', async ({ page }) => {
  await boot(page);
  await newNote(page);
  const md = '# Trip to Lisbon\n\n## Food\n- pastel de nata\n  - Manteigaria\n- sardines\n\n## Sights\n- [ ] Alfama tiles\n- [x] Tram 28\n\n## Notes\nplain paragraph, not a node\n';
  await page.keyboard.insertText(md);
  await expect(page.locator('.cm-content')).toContainText('Manteigaria');

  await page.getByTestId('toggle-note-map').click();
  const panel = page.getByTestId('markmap-panel');
  await expect(panel).toBeVisible();
  // root (hoisted single h1) + 3 headings + 2+1... = 1 + 3 + (nata, Manteigaria, sardines) + (2 tasks) = 9
  await expect(panel).toHaveAttribute('data-nodes', '9');
  const nodes = page.locator('[data-testid="markmap-svg"] g.markmap-node');
  await expect(nodes).toHaveCount(9);
  await expect(page.locator('[data-testid="markmap-svg"]')).toContainText('Manteigaria');
  await expect(page.locator('[data-testid="markmap-svg"]')).toContainText('☑ Tram 28');
  await page.waitForTimeout(800); // let the fit-to-panel settle
  await shot(page, '42-note-mind-map');

  // clicking a branch jumps to that line in the editor
  await page.locator('[data-testid="markmap-svg"] g.markmap-node', { hasText: 'Tram 28' }).locator('foreignObject').click();
  await expect.poll(() => page.evaluate(() => {
    const sel = window.getSelection();
    const line = sel?.anchorNode && (sel.anchorNode.nodeType === 3 ? sel.anchorNode.parentElement : (sel.anchorNode as Element))?.closest('.cm-line');
    return line?.textContent ?? '';
  })).toContain('Tram 28');

  // the map follows edits
  await page.locator('.cm-content').click();
  await page.keyboard.press('Control+End');
  await page.keyboard.insertText('\n## Later\n- one more');
  await expect(panel).toHaveAttribute('data-nodes', '11', { timeout: 5000 });

  // the toggle is remembered
  await page.reload();
  await nav(page, 'notes');
  await page.getByTestId('note-item').first().click();
  await expect(page.getByTestId('markmap-panel')).toBeVisible();
  await page.getByTestId('toggle-note-map').click();
  await expect(page.getByTestId('markmap-panel')).toBeHidden();
});
