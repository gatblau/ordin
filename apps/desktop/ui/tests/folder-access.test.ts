// @vitest-environment happy-dom
// Who may use which folder (SM-FA-1, SM-FA-2) and Ordin's access card (SM-FA-3, OrdinHost.md OH-ACC-1),
// mounted so their reads and changes run.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import Access from '../src/lib/blocks/kinds/Access.svelte';
import type { SourceLink } from '../src/lib/service-api/types.gen';
import FolderAccess from '../src/lib/views/fixed/FolderAccess.svelte';
import { backendWith, WS } from './fixed-support';
import { ok, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const AGENT = uid('agent-doc');
const FOLDER = uid('place-sample');
const ISSUE = uid('place-issue');
const text = (el: Element): string => (el.textContent ?? '').replace(/[⁨⁩]/g, '');
const link = (id: string, kind: SourceLink['kind'], placeId: string, name: string): SourceLink =>
  ({ id, workspace_id: WS, kind, state: 'active', origin_location: 'x', display_name: name, place_id: placeId, added_at: '2026-01-26T10:00:00.000Z' }) as SourceLink;
const granted = (access: 'read' | 'read_and_change') => ({ id: uid('grant'), workspace_id: WS, grantee: { kind: 'agent_instance', agent_instance_id: AGENT }, place_id: FOLDER, access, granted_at: '2026-01-26T10:00:00.000Z' });

async function draw(component: unknown, props: Record<string, unknown>): Promise<HTMLElement> {
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(component as never, { target, props: props as never });
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
  return target;
}

describe('Who may use which folder (SM-FA-1, SM-FA-2)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-FA-1
  it('Happy path — a folder the agent cannot use is offered at three levels, and a choice is sent at once', async () => {
    const { state, transport } = await backendWith({ setup: (t) => t.on('workspaces.grants', ok([])).on('workspaces.set_grant', ok(granted('read_and_change'))) });
    const target = await draw(FolderAccess, { backend: state, workspaceId: WS, agents: [{ id: AGENT, name: 'Document agent' }], links: [link('l1', 'local_folder', FOLDER, 'Sample documents')], by: 'folder' });
    expect(text(target)).toContain('What it can use');
    expect(text(target)).toContain('Sample documents');
    const select = target.querySelector('select') as HTMLSelectElement;
    expect(select.value).toBe('none');
    expect([...select.options].map((o) => o.textContent)).toEqual(["Can't use", 'Can read', 'Can read and change']);
    select.value = 'read_and_change';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await settle(10);
    expect(transport.sentOf('workspaces.set_grant').map((e) => e.input)).toEqual([
      { workspace_id: WS, grantee: { kind: 'agent_instance', agent_instance_id: AGENT }, place_id: FOLDER, access: 'read_and_change' },
    ]);
  });

  it("Edge case — can't use removes the grant, and a Jira issue offers no change", async () => {
    const { state, transport } = await backendWith({ setup: (t) => t.on('workspaces.grants', ok([granted('read')])).on('workspaces.set_grant', ok(null)) });
    const target = await draw(FolderAccess, {
      backend: state,
      workspaceId: WS,
      agents: [{ id: AGENT, name: 'Document agent' }],
      links: [link('l1', 'local_folder', FOLDER, 'Sample documents'), link('l2', 'jira_issue', ISSUE, 'PAY-12')],
      by: 'folder',
    });
    const [folder, issue] = [...target.querySelectorAll('select')] as HTMLSelectElement[];
    expect(folder?.value).toBe('read');
    expect([...(issue as HTMLSelectElement).options].map((o) => o.value)).toEqual(['none', 'read']);
    (folder as HTMLSelectElement).value = 'none';
    folder?.dispatchEvent(new Event('change', { bubbles: true }));
    await settle(10);
    expect(transport.sentOf('workspaces.set_grant').map((e) => (e.input as { access: unknown }).access)).toEqual([null]);
  });
});

describe("Ordin's access card (SM-FA-3)", () => {
  // spec: ordin-desktop/components/ScreensModule#SM-FA-3
  it('Happy path — ticking grants the folder at once', async () => {
    const { state, transport } = await backendWith({ setup: (t) => t.on('workspaces.grants', ok([])).on('workspaces.set_grant', ok(granted('read_and_change'))) });
    const block = {
      kind: 'access',
      agent_instance_id: AGENT,
      agent_name: 'Document agent',
      text: "Document agent can't use this workspace's folders yet. Tick the ones it may use, then ask me to go ahead.",
      places: [
        { place_id: FOLDER, name: 'Sample documents', read_only: false },
        { place_id: ISSUE, name: 'PAY-12', read_only: true },
      ],
    };
    const target = await draw(Access, { backend: state, workspaceId: WS, blockId: uid('block'), block });
    expect(text(target)).toContain("Document agent can't use this workspace's folders yet.");
    expect(text(target)).toContain('Document agent can read and change Sample documents');
    expect(text(target)).toContain('Document agent can read PAY-12');
    const box = target.querySelector('input[type="checkbox"]') as HTMLInputElement;
    expect(box.checked).toBe(false);
    box.checked = true;
    box.dispatchEvent(new Event('change', { bubbles: true }));
    await settle(10);
    expect(transport.sentOf('workspaces.set_grant').map((e) => e.input)).toEqual([
      { workspace_id: WS, grantee: { kind: 'agent_instance', agent_instance_id: AGENT }, place_id: FOLDER, access: 'read_and_change' },
    ]);
  });
});
