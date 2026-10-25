import { expect, type Page } from '@playwright/test';
import path from 'node:path';

export const SHOTS = path.resolve(process.cwd(), '../docs/screenshots');
export const shot = (page: Page, name: string) => page.screenshot({ path: path.join(SHOTS, `${name}.png`) });

export async function boot(page: Page) {
  await page.goto('/');
  await expect(page.getByTestId('app')).toBeVisible();
}
export const nav = (page: Page, v: 'timeline' | 'notes' | 'graph' | 'search' | 'library') => page.getByTestId(`nav-${v}`).click();

/** Create a fresh note and put the caret in the editor. */
export async function newNote(page: Page) {
  await nav(page, 'notes');
  await page.getByTestId('new-note').click();
  await expect(page.locator('.cm-editor.cm-focused')).toBeVisible();
  await expect(page.locator('.cm-content')).toBeFocused();
}
