import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { searchForWorkspaceRoot } from 'vite';
import { defineConfig } from 'vitest/config';

// The tests run in Node with no DOM: stores and helpers directly, components through
// `svelte/server`'s `render`. A test that needs a DOM opts in with `// @vitest-environment happy-dom`.
export default defineConfig({
  plugins: [svelte()],
  // Tests that need Svelte's reactivity (a DOM environment, see tests/support/reactive.svelte.ts) load its browser build. Set only under Vitest: setting
  // `resolve.conditions` anywhere else replaces Vite's defaults, and the production build then picks Svelte's server build.
  ...(process.env.VITEST ? { resolve: { conditions: ['browser'] } } : {}),
  // Tests in a DOM environment load the en-GB catalogues from `crates/ordin-localisation` (tests/support/fake-backend.ts),
  // which sits outside this folder; Vite refuses files outside its allow list there. Only that folder is added, only under Vitest.
  ...(process.env.VITEST
    ? {
        server: {
          fs: {
            allow: [
              searchForWorkspaceRoot(process.cwd()),
              fileURLToPath(new URL('../../../crates/ordin-localisation/catalogues', import.meta.url)),
            ],
          },
        },
      }
    : {}),
  build: { target: 'es2022', sourcemap: false },
  test: {
    environment: 'node',
    include: ['tests/**/*.test.ts'],
    // Plan 04, safety net A: records the requests the fake backends were sent (see the file).
    setupFiles: ['tests/support/record-requests.ts'],
    // The code editor (Monaco) calls off its own pending work when it is closed, as a rejection named
    // `Canceled`; the dead-control sweep opens and closes editors by the hundred. Only that error is let pass.
    onUnhandledError: (error) => !(error.name === 'Canceled' && error.message === 'Canceled'),
    coverage: {
      provider: 'v8',
      include: ['src/lib/**/*.{ts,svelte}'],
      exclude: ['src/lib/service-api/**'],
      reporter: ['text-summary'],
    },
  },
});
