import { defineConfig } from '@playwright/test';

const PORT = 8923;
// Runs the real Rust core through the dev bridge (same `dispatch` the desktop app uses), serving the built UI.
export default defineConfig({
  testDir: './e2e',
  timeout: 60_000,
  workers: 1,
  retries: process.env.CI ? 1 : 0, // a few editor/WebGL timing races on loaded runners
  fullyParallel: false,
  reporter: [['list']],
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    viewport: { width: 1360, height: 860 },
    channel: process.env.PW_CHANNEL ?? 'chrome', // system Chrome; set PW_CHANNEL= (empty) to use bundled Chromium
    colorScheme: 'light',
    timezoneId: 'Asia/Jerusalem',
    locale: 'en-GB',
    launchOptions: { args: ['--autoplay-policy=no-user-gesture-required'] },
  },
  webServer: {
    command: `rm -rf e2e-data && cargo run -q -p dev-bridge -- --data ./e2e-data --port ${PORT} --static ui/dist`,
    cwd: '..',
    url: `http://127.0.0.1:${PORT}/`,
    reuseExistingServer: false,
    timeout: 240_000,
  },
});
