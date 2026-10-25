import { expect, test } from '@playwright/test';
import { boot, nav, shot } from './helpers';

// Runs against the dev bridge with TIMELINE_ASR=fake (see playwright.config.ts): the whole UI flow
// (settings, model download job, transcription job, cancel, auto-transcribe) is real; only the
// speech engine is a deterministic stand-in. Real whisper.cpp output is covered by the Rust tests.
test('speech-to-text: download a model, transcribe a recording, cancel, auto-transcribe', async ({ page }) => {
  await boot(page);
  await nav(page, 'library');
  await expect(page.getByTestId('media-detail')).toBeVisible();
  const panel = page.getByTestId('asr-panel');
  await panel.locator('summary').click();
  await expect(page.getByTestId('asr-model')).toHaveCount(2);
  // language list is limited by the model: base.en is English only
  await expect(page.getByTestId('asr-language').locator('option[value="ru"]')).toHaveAttribute('disabled', '');

  // transcribe with the model missing: it is downloaded first (with progress), then transcription runs
  await page.getByTestId('transcribe').click(); // sample recordings already have a transcript: first click arms
  await expect(page.getByTestId('transcribe')).toContainText('Replace current transcript?');
  await page.getByTestId('transcribe').click();
  await expect(page.getByTestId('transcribe-progress')).toBeVisible();
  await shot(page, '40-transcribe-progress');
  await expect(page.getByTestId('segment').first()).toContainText('Welcome to the fake transcript.', { timeout: 20_000 });
  await expect(page.getByTestId('segment')).toHaveCount(3); // [BLANK_AUDIO] marker dropped
  await expect(page.getByTestId('transcript-language')).toHaveText('en');
  await expect(page.getByTestId('asr-model').first().getByTestId('asr-delete')).toBeVisible(); // base.en is now installed

  // clicking a segment still seeks the player (existing synced transcript player)
  await page.getByTestId('segment').nth(1).locator('button').click();
  await expect(page.getByTestId('segment').nth(1)).toHaveClass(/active/, { timeout: 5_000 });

  // cancel a running transcription: the old transcript is untouched
  await page.getByTestId('transcribe').click();
  await page.getByTestId('transcribe').click();
  await expect(page.getByTestId('transcribe-progress')).toBeVisible();
  await page.getByTestId('transcribe-cancel').click();
  await expect(page.getByTestId('transcribe-progress')).toBeHidden({ timeout: 10_000 });
  await expect(page.getByTestId('segment')).toHaveCount(3);

  // settings persist across a reload
  await page.getByTestId('asr-auto').check();
  await page.reload();
  await nav(page, 'library');
  await page.getByTestId('asr-panel').locator('summary').click();
  await expect(page.getByTestId('asr-auto')).toBeChecked();
  await shot(page, '41-speech-settings');

  // auto-transcribe on import: a new wav gets a transcript without pressing anything
  const wav = Buffer.alloc(44 + 16000 * 2);
  wav.write('RIFF', 0); wav.writeUInt32LE(36 + 16000 * 2, 4); wav.write('WAVEfmt ', 8); wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20); wav.writeUInt16LE(1, 22); wav.writeUInt32LE(16000, 24); wav.writeUInt32LE(32000, 28);
  wav.writeUInt16LE(2, 32); wav.writeUInt16LE(16, 34); wav.write('data', 36); wav.writeUInt32LE(16000 * 2, 40);
  await page.getByTestId('import-input').setInputFiles({ name: 'auto-memo.wav', mimeType: 'audio/wav', buffer: wav });
  await expect(page.getByTestId('media-title')).toHaveValue(/auto-memo/);
  await expect(page.getByTestId('segment').first()).toContainText('Welcome to the fake transcript.', { timeout: 20_000 });
  // turn it off again so later specs are unaffected
  await page.getByTestId('asr-auto').uncheck();
});
