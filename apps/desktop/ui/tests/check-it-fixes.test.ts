// @vitest-environment happy-dom
// Check it's findings and fixes (SM-CHK-1..3): a finding reads as its sentence with the part's name,
// **Ask Ordin to fix it** sends Ordin the problem, and a fix with no action on the page is not drawn. The
// panel is mounted so its buttons run.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { forgetCheckRuns } from '../src/lib/views/fixed/check-it.svelte';
import CheckItPanel from '../src/lib/views/fixed/CheckItPanel.svelte';
import { backendWith, WS } from './fixed-support';
import { err, ok, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);
beforeEach(() => forgetCheckRuns());

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const text = (el: Element): string => (el.textContent ?? '').replace(/[⁨⁩]/g, '');
const AGENT = uid('agent-doc');
const PART = 'urn:ordin:default:workspaces:document-agent';
const report = {
  check_run_id: uid('run-1'), checked_at: '2026-01-16T10:00:00Z', passed: false, agents_that_stop: [], publish_gate_met: true, tidied: [], version_checked: '1', subject: { kind: 'installed' },
  findings: [{ code: 'reference_unresolved', severity: 'failure', part_identifier: PART, message_id: 'check-part-invalid', args: { part: 'Document agent' }, fixes: [{ fix: 'fix_it', action: { action: 'mint_identifier', field: 'id', value: 'urn:x' } }, { fix: 'ask_ordin' }], detail: '' }],
};

async function settled(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}

async function drawChecked(send: unknown): Promise<{ target: HTMLElement; sent: () => unknown[] }> {
  const { state, transport } = await backendWith({ setup: (t) => t.on('checker.check_installed', ok(report)).on('ordin.send_message', send as never) });
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(CheckItPanel as never, { target, props: { backend: state, workspaceId: WS, subjectName: 'Document agent', subject: { kind: 'installed', agentInstanceId: AGENT } } as never });
  await settled();
  (target.querySelector('button') as HTMLButtonElement).click();
  await settled();
  return { target, sent: () => transport.sentOf('ordin.send_message').map((s) => s.input) };
}

const button = (target: HTMLElement, words: string): HTMLButtonElement | undefined =>
  [...target.querySelectorAll('button')].find((b) => text(b).includes(words)) as HTMLButtonElement | undefined;

describe('Check it fixes (SM-CHK-1..3)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-CHK-1
  // spec: ordin-desktop/components/ScreensModule#SM-CHK-3
  it('a finding reads as its sentence with the part name; Fix it is not drawn without an action', async () => {
    const { target } = await drawChecked(ok({ accepted: true, position_in_queue: 0 }));
    expect(text(target)).toContain("Document agent doesn't pass one of ordin's checks.");
    expect(text(target)).not.toContain('check-part-invalid');
    expect(text(target)).not.toContain(PART);
    expect([...target.querySelectorAll('button')].map((b) => text(b).trim())).not.toContain('Fix it');
    expect(button(target, 'to fix it')).toBeDefined();
  });

  // spec: ordin-desktop/components/ScreensModule#SM-CHK-2
  it('Happy path — Ask Ordin to fix it sends Ordin the problem in the open workspace', async () => {
    const { target, sent } = await drawChecked(ok({ accepted: true, position_in_queue: 0 }));
    button(target, 'to fix it')?.click();
    await settled();
    expect(sent()).toEqual([
      { workspace_id: WS, text: "Check it found a problem with Document agent: Document agent doesn't pass one of ordin's checks. Can you fix it?" },
    ]);
    expect(text(target)).toContain('Sent to Ordin; the answer is in the conversation.');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-CHK-2
  it('Error path — the message cannot be sent: its sentence is shown and the button can be pressed again', async () => {
    const { target, sent } = await drawChecked(err('OH_TOO_MANY_WAITING', 'Ordin has too many messages waiting. Try again in a moment.'));
    button(target, 'to fix it')?.click();
    await settled();
    expect(text(target)).toContain('Ordin has too many messages waiting');
    expect(text(target)).not.toContain('Sent to Ordin');
    expect(button(target, 'to fix it')?.disabled).toBe(false);
    expect(sent()).toHaveLength(1);
  });
});
