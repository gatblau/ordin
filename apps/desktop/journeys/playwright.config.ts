// The journeys: Playwright attached to the real program on the Windows build machine (plan 04 Phase 6,
// OrdinDesktop.md DSK-JRN-1). One program, one window, so the journeys run one after another, in the
// order of their file names, and share the data root the run started with.
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './journeys',
  workers: 1,
  fullyParallel: false,
  retries: 0,
  // A journey waits for Claude Code and the stand-ins; a task with Ordin takes about a minute.
  timeout: 10 * 60_000,
  expect: { timeout: 30_000 },
  outputDir: 'run/output',
  reporter: [['list'], ['html', { outputFolder: 'run/report', open: 'never' }], ['json', { outputFile: 'run/results.json' }]],
  globalSetup: './support/global-setup.ts',
  globalTeardown: './support/global-teardown.ts',
});
