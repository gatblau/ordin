// A build without the `minutes` switch (ScreensModule.md SM-MIN-1, SM-UP-1): its `service.hello` does not
// name the group, so no Minutes row, no Meeting minutes settings and no recording indicator are drawn; the
// same hello with the group draws the row and the section.
import { beforeAll, describe, expect, it } from 'vitest';

import SideRail from '../src/lib/shell/SideRail.svelte';
import { getScreen } from '../src/lib/shell/screens';
import { answersAll } from '../src/lib/backend/groups';
import { visibleSections } from '../src/lib/views/Settings/sections';
import '../src/lib/views/minutes/register';
import RecordingIndicator from '../src/lib/views/minutes/RecordingIndicator.svelte';
import { hello, LOCAL, ok, R1_GROUPS } from './support/fake-backend';
import { appWith, html, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const workspaces = [{ id: WS, name: 'Q4 forecast', created_at: 'x', own_folder_place_id: 'p', state: 'active' }];
const words = (s: string): string => s.replace(/<[^>]*>/g, ' ').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();

async function appFor(groups: string[]) {
  const { app } = await appWith((t) => {
    t.on('workspaces.list', ok(workspaces));
    t.on('service.hello', ok(hello(LOCAL, groups)));
  });
  app.prefs.setExpanded([`${LOCAL}:${WS}`]);
  return app;
}

describe('a build without the switch (SM-MIN-1, SM-UP-1)', () => {
  it('Edge — no Minutes row, no Record screen, no Meeting minutes settings and no indicator when the backend does not answer the group', async () => {
    const app = await appFor(R1_GROUPS);
    const out = words(html(SideRail, {}, app));
    expect(out).toContain('Dashboards');
    expect(out).not.toContain('Minutes');
    expect(visibleSections(app.local).map((s) => s.id)).not.toContain('meeting-minutes');
    for (const id of ['minutes-home', 'minutes']) expect(answersAll(app.local, getScreen(id)?.commands ?? ['minutes.x'])).toBe(false);
    expect(words(html(RecordingIndicator, {}, app))).toBe('');
  });

  it('Happy path — with the group the Minutes row and the settings section are drawn', async () => {
    const app = await appFor([...R1_GROUPS, 'minutes']);
    expect(words(html(SideRail, {}, app))).toContain('Minutes');
    expect(visibleSections(app.local).map((s) => s.id)).toContain('meeting-minutes');
    for (const id of ['minutes-home', 'minutes']) expect(answersAll(app.local, getScreen(id)?.commands ?? [])).toBe(true);
  });
});
