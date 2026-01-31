// @vitest-environment happy-dom
// SM-TW-14 and SM-TK-3 mounted (ScreensModule.md, amended January 2026, user decision): Share the whole
// workspace opens the team-workspace share dialog with the own folder fixed, and the panel says what a share carried.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import TeamPanel from '../src/lib/views/fixed/TeamPanel.svelte';
import { ok, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { loadWindowCatalogue } from './support/ssr';
import { backendWith, WS } from './fixed-support';
import { TEAM_GROUPS } from './team-workspace-support';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const clean = (s: string | null | undefined): string => (s ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
const page = (): string => clean(document.body.textContent);
async function flush(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}
const button = (label: string): HTMLButtonElement => {
  const found = [...document.querySelectorAll('button')].find((b) => clean(b.textContent) === label);
  expect(found, `a button named ${label}`).toBeDefined();
  return found as HTMLButtonElement;
};
const status = (over: Record<string, unknown> = {}) => ({
  workspace_id: WS, folders: [], descriptions: [], folder_large: false, held_for_check: 0, problems: [], workspace_facts: 0, workspace_large: false, ...over,
});
const counts = { new_facts: 42, changed_facts: 0, removed_facts: 0, new_pages: 6, changed_pages: 0, removed_pages: 0 };

function draw(props: Record<string, unknown>): void {
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(TeamPanel as never, { target, props: props as never });
}

describe('the team panel mounted', () => {
  it('SM-TW-14 — Share the whole workspace upgrades a knowledge-only workspace through the share dialog', async () => {
    const bound = { binding: { folder: 'k', access: 'read_write', source_link_id: uid('sl1') }, header_line: 'acme/supplier-team › knowledge' };
    const { state, transport } = await backendWith({ groups: TEAM_GROUPS, setup: (tr) => tr.on('team_knowledge.status', ok(status(bound))) });
    draw({ backend: state, workspaceId: WS });
    await flush();
    button('Share the whole workspace').click();
    await flush();
    expect(document.querySelector('[role=dialog]')).not.toBeNull();
    expect(page()).toContain('Team knowledge: acme/supplier-team › knowledge');
    expect(transport.sentOf('team_knowledge.workspace_share_preview')[0]?.input).toEqual({ ws: WS, choice: { kind: 'link', source_link_id: uid('sl1') } });
    expect(transport.commandsSent()).not.toContain('team_knowledge.turn_on');
  });

  // SM-TK-2 (amended January 2026; user decision; Approved by the user January 2026): no pointer line.
  it('SM-TK-2 — a workspace with nothing shared draws nothing', async () => {
    const { state } = await backendWith({ groups: TEAM_GROUPS, setup: (tr) => tr.on('team_knowledge.status', ok(status())) });
    draw({ backend: state, workspaceId: WS });
    await flush();
    expect(page()).toBe('');
    expect(document.querySelectorAll('button')).toHaveLength(0);
  });

  it('SM-TK-3 — after a share the panel says what it carried', async () => {
    const bound = { binding: { folder: 'k', access: 'read_write' }, header_line: 'k' };
    const { state, transport } = await backendWith({
      groups: TEAM_GROUPS,
      setup: (tr) => tr.on('team_knowledge.status', ok(status(bound))).on('team_knowledge.share', ok(status({ ...bound, busy: 'sharing' }))),
    });
    draw({ backend: state, workspaceId: WS });
    await flush();
    button("Share Ordin's memory and knowledge pages with your team").click();
    await flush();
    expect(page()).toContain('Preparing your share…');
    expect(page()).not.toContain('Your share of');
    transport.on('team_knowledge.status', ok(status({ ...bound, last_share: { share_id: uid('s1'), approval_id: uid('a1'), held_for_check: 0, counts } })));
    state.workspaceRevisions.set(WS, state.workspaceRevision(WS) + 1);
    await flush();
    expect(page()).toContain('Your share of 42 facts and 6 pages is waiting for your approval in Needs you.');
  });
});
