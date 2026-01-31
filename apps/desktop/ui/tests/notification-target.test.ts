// @vitest-environment happy-dom
// OrdinDesktop.md DSK-NTF-1: after a notification, `ordin://open-target` opens the item it was about — an
// approval's card, a run's or a crew's screen, a question's workspace — through the window started the way
// main.ts starts it.
import { mount, unmount } from 'svelte';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import App from '../src/App.svelte';
import { startApp } from '../src/lib/app/bootstrap';
import type { AppContext } from '../src/lib/shell/context';
import '../src/lib/shell/load-registrations';
import { openNotificationTarget } from '../src/lib/shell/open-item';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import { makeKey, SCREEN_IDS } from '../src/lib/shell/screens';
import { LOCAL, settle } from './support/fake-backend';
import { sampleId, SampleTransport } from './support/sample-backend';

let transport: SampleTransport;
let app: AppContext;
let component: ReturnType<typeof mount> | null = null;

beforeAll(async () => {
  transport = new SampleTransport();
  app = await startApp(transport, { storage: null });
  const target = document.createElement('div');
  document.body.appendChild(target);
  component = mount(App, { target, props: { app } });
  await settle(40);
});

afterAll(() => {
  if (component !== null) unmount(component);
});

const ws = (): string => transport.workspaceId;

describe('DSK-NTF-1 — a notification opens its item', () => {
  it('Happy path — an approval notice opens its card in its workspace', async () => {
    const approval = sampleId(700);
    transport.desktop('open-target', { kind: 'approval', backend: LOCAL, workspace_id: ws(), approval_id: approval });
    await settle(10);
    const key = makeKey(SCREEN_IDS.approval, approval);
    expect(app.shell.openWorkspace).toBe(ws());
    expect(app.screens.front(ScreenAreaState.scope(LOCAL, ws()))).toBe(key);
  });

  it('Happy path — a finished run opens its screen, a question its workspace', async () => {
    const run = sampleId(701);
    expect(openNotificationTarget(app, { kind: 'run', backend: LOCAL, run_id: run })).toBe(makeKey(SCREEN_IDS.run, run));
    const crew = sampleId(702);
    expect(openNotificationTarget(app, { kind: 'crew_run', backend: LOCAL, crew_run_id: crew })).toBe(makeKey(SCREEN_IDS.crew, crew));
    expect(openNotificationTarget(app, { kind: 'question', backend: LOCAL, workspace_id: ws(), block_id: sampleId(703) })).toBe(ws());
    expect(app.shell.openWorkspace).toBe(ws());
  });

  it('Edge case — a payload of another kind, or with a part missing, opens nothing', () => {
    expect(openNotificationTarget(app, null)).toBeNull();
    expect(openNotificationTarget(app, { kind: 'handover', backend: LOCAL })).toBeNull();
    expect(openNotificationTarget(app, { kind: 'approval', backend: LOCAL, approval_id: sampleId(704) })).toBeNull();
    expect(openNotificationTarget(app, { kind: 'run', run_id: sampleId(705) })).toBeNull();
  });
});
