// @vitest-environment happy-dom
// The progress card's Review opens the run's screen, and is not drawn on that screen (SM-PG-3).
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import PlanProgress from '../src/lib/blocks/kinds/PlanProgress.svelte';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import { APP_KEY } from '../src/lib/shell/context';
import { settle } from './support/fake-backend';
import { appWith, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const RUN = '0192a1c2-3344-7556-8677-8899aabbccdd';
const review = (target: HTMLElement) => [...target.querySelectorAll('button')].find((b) => (b.textContent ?? '').includes('Review'));

async function draw() {
  const { app } = await appWith();
  app.local.runs.set(RUN, { state: 'running', progressLines: [], folded: 0, count: null, plan: { plan_version: 1, steps: [] }, startUp: [] } as never);
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(PlanProgress as never, {
    target,
    props: { backend: app.local, workspaceId: WS, block: { kind: 'plan_progress', subject: { kind: 'run', run_id: RUN } } } as never,
    context: new Map<unknown, unknown>([[APP_KEY, app]]),
  });
  await settle();
  flushSync();
  return { app, target };
}

describe('Review on the progress card (SM-PG-3)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-PG-3
  it('Happy path — Review opens the run screen in front, and the card there no longer offers it', async () => {
    const { app, target } = await draw();
    const scope = ScreenAreaState.scope(app.local.id, WS);
    review(target)?.click();
    await settle();
    flushSync();
    expect(app.screens.front(scope)).toBe(`run:${RUN}`);
    expect(review(target)).toBeUndefined();
  });
});
