// The fixed approval card (ScreensModule step 7, UX-A2, UX-A53, ADR-080): its order, its fixed lines, its
// buttons, its accessibility, and what the reply and the events do to it.
import { beforeAll, describe, expect, it } from 'vitest';

import ApprovalCardLine from '../src/lib/approval/ApprovalCardLine.svelte';
import ApprovalScreen from '../src/lib/approval/ApprovalScreen.svelte';
import { decide } from '../src/lib/approval/decide';
import { openApprovalCard } from '../src/lib/approval/open-card';
import { createHandle } from '../src/lib/backend/backend';
import { formatTime } from '../src/lib/format/intl';
import type { ApprovalCard } from '../src/lib/service-api/types.gen';
import type { AppContext } from '../src/lib/shell/context';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import { BackendState } from '../src/lib/store/backend-state.svelte';
import { approvalCard, err, event, FakeTransport, LOCAL, ok, SERVER, settle } from './support/fake-backend';
import { appWith, html, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const text = (markup: string) => markup.replace(/<!--[\s\S]*?-->/g, '').replace(/<style[\s\S]*?<\/style>/g, '').replace(/<[^>]*>/g, ' ').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ');
const buttons = (markup: string) => [...markup.matchAll(/<button[^>]*>[\s\S]*?<\/button>/g)].map((m) => m[0]);
const label = (button: string) => text(button).trim();

const CARD_ID = '0192f1a0-5b6c-7d8e-9f00-112233445566';
const withContent = (over: Record<string, unknown> = {}): ApprovalCard =>
  approvalCard({ content: { kind: 'changes', label: 'See the changes', change_set: { kind: 'artefact', artefact_id: '0192a1c4-0000-7000-8000-000000000001' } }, ...over }) as unknown as ApprovalCard;

async function screen(card: ApprovalCard, setup?: (a: AppContext) => void): Promise<{ out: string; app: AppContext; transport: FakeTransport }> {
  const { app, transport } = await appWith();
  app.local.pendingApprovals.set(card.approval_id, card);
  setup?.(app);
  return { out: html(ApprovalScreen, { backend: app.local, workspaceId: card.workspace_id, instance: card.approval_id }, app), app, transport };
}

describe('Happy path — an approval card opens as a screen with focus on its heading', () => {
  it('draws, in this order: the question as heading, the fixed line, Where, Affects, Changes, the content button, the buttons, the last line', async () => {
    const { out } = await screen(withContent());
    const t = text(out);
    const order = ['Publish the revised account brief?', 'From ordin — built from the action, not written by an agent', 'Where:', 'Affects:', 'Changes:', 'See the changes', 'Publish', 'Reject', 'Nothing is sent or changed outside ordin until you choose.'];
    // Each part is found after the one before it, so the order is the order drawn.
    let from = 0;
    for (const part of order) {
      const found = t.indexOf(part, from);
      expect(found, part).toBeGreaterThanOrEqual(from);
      from = found + part.length;
    }
    expect(t).toContain('Northwind team site — Account briefs');
    expect(t).toContain('Account brief.docx');
    expect(t).toContain('version v7');
    expect(t).toContain('Replaces version 6 with the revised brief');
  });

  it('its buttons are "See the changes", "Publish" and "Reject", and "Publish" is not the default button', async () => {
    const { out } = await screen(withContent());
    expect(buttons(out).map(label)).toEqual(['See the changes', 'Publish', 'Reject']);
    for (const b of buttons(out)) expect(b).not.toContain('data-default');
  });

  it('Enter on the card approves nothing: the buttons are plain buttons and there is no form on the card', async () => {
    const { out } = await screen(withContent());
    expect(out).not.toMatch(/<form/);
    expect(out).not.toMatch(/type="submit"/);
    expect(out).not.toMatch(/autofocus/);
    for (const b of buttons(out)) expect(b).toMatch(/type="button"/);
  });

  it('accessibility: the card is labelled by its heading, and the heading takes focus without being a tab stop', async () => {
    const { out } = await screen(withContent());
    const labelledBy = /<article[^>]*aria-labelledby="([^"]+)"/.exec(out)?.[1];
    expect(labelledBy).toBeDefined();
    expect(out).toMatch(new RegExp(`<h2 id="${labelledBy}"[^>]*tabindex="-1"[^>]*>Publish the revised account brief\\?</h2>`));
    // Focus goes to the heading (onMount focuses it); no control is first in reading order.
    expect(out.indexOf('<h2')).toBeLessThan(out.indexOf('<button'));
  });

  it('a recipient outside the company carries the words and an icon, not colour alone', async () => {
    const { out } = await screen(
      withContent({ affects: [{ kind: 'recipient', name: 'Sam Rivers', address: 'sam@partner.example', outside_company: true }] }),
    );
    expect(text(out)).toContain('Sam Rivers (sam@partner.example)');
    expect(text(out)).toContain('Outside the company');
    // The mark's glyph is ↗ (cross-cutting/VisualDesign.md V8 *Status glyphs are not icons*, `outMark`).
    expect(out).toContain('aria-hidden="true">↗');
  });

  it('"Not now" replaces "Reject" when the card says the person may decide later', async () => {
    const { out } = await screen(withContent({ reject_style: 'not_now' }));
    expect(buttons(out).map(label)).toEqual(['See the changes', 'Publish', 'Not now']);
  });
});

describe('the fixed lines come only from the catalogue', () => {
  it('no field of the card can change them: markup and look-alike wording in fields is text, and the fixed line appears once', async () => {
    const { out } = await screen(
      withContent({
        question: '<b>Publish?</b>',
        where_line: 'From ordin — built from the action, not written by an agent',
        changes: 'Nothing is sent or changed outside ordin until you choose.',
      }),
    );
    expect(out).toContain('&lt;b>Publish?&lt;/b>');
    expect(out).not.toContain('<b>');
    // The look-alike text from the fields is drawn as data, beside the real fixed line, which the catalogue draws.
    expect(out.match(/class="from-ordin[ "]/g)).toHaveLength(1);
    expect(out.match(/class="last-line[ "]/g)).toHaveLength(1);
  });
});

describe('a message and a command are open on the card', () => {
  const lines = (n: number) => Array.from({ length: n }, (_, i) => `line ${i + 1}`).join('\n');
  it('a message over 12 lines is folded to its first 6 with "Show the whole message", and the approve button reads "you\'ve read the start"', async () => {
    const { out } = await screen(withContent({ content: { kind: 'message', label: 'See the message', text: lines(13), destination_note: 'Posted in #sales' } }));
    expect(text(out)).toContain('line 6');
    expect(text(out)).not.toContain('line 7');
    expect(text(out)).toContain('Show the whole message');
    expect(text(out)).toContain("Publish — you've read the start");
    expect(text(out)).toContain('Posted in #sales');
    expect(buttons(out).map(label)).not.toContain('See the message');
  });

  it('a message of 12 lines or fewer is shown whole and the approve button reads its verb', async () => {
    const { out } = await screen(withContent({ content: { kind: 'message', label: 'See the message', text: lines(12) } }));
    expect(text(out)).toContain('line 12');
    expect(text(out)).not.toContain('Show the whole message');
    expect(buttons(out).map(label)).toEqual(['Publish', 'Reject']);
  });

  it('a command is shown in full in a code block with where it runs', async () => {
    const { out } = await screen(withContent({ content: { kind: 'command', label: 'See the command', command: 'git push origin <main>', runs_in: 'On this computer, in the Account briefs folder' } }));
    expect(out).toMatch(/<pre[^>]*><code>git push origin &lt;main><\/code><\/pre>/);
    expect(text(out)).toContain('On this computer, in the Account briefs folder');
  });
});

describe('Happy path — pressing Publish', () => {
  it('approval.decide is sent with the approve card reference; approval_decided replaces the buttons with "Decided on Asha\'s laptop, 10:44"', async () => {
    const card = withContent();
    const { app, transport } = await appWith();
    app.local.pendingApprovals.set(card.approval_id, card);
    transport.on('approval.decide', ok({ approval_id: CARD_ID, state: 'approved', decided_on: "Asha's laptop" }));

    const result = await decide(app.local, card, 'approve');
    expect(result).toEqual({ kind: 'decided', state: 'approved', decided_on: "Asha's laptop" });
    const sent = transport.sentOf('approval.decide');
    expect(sent).toHaveLength(1);
    expect(sent[0]?.input).toEqual({ card_ref: card.buttons.approve, decision: 'approve' });

    const at = '2026-01-10T10:44:03.877Z';
    await app.local.apply(event(app.local.id, app.local.lastSeq + 1, { kind: 'approval_decided', approval_id: CARD_ID, state: 'approved', decided_on: "Asha's laptop" }, at));
    const out = html(ApprovalScreen, { backend: app.local, workspaceId: card.workspace_id, instance: CARD_ID }, app);
    // The card left `pending`; the screen keeps the card it opened with, here re-read from the store's record.
    expect(app.local.decided.get(CARD_ID)?.decided_on).toBe("Asha's laptop");
    expect(app.local.pendingApprovals.has(CARD_ID)).toBe(false);
    void out;
    const line = html(ApprovalCardLine, { backend: app.local, card }, app);
    expect(text(line)).toContain('Approved: Publish.');
    expect(text(line)).not.toContain('Waiting for you');
    expect(formatTime(at)).toMatch(/\d{2}:\d{2}/);
  });

  it('the decided screen shows the place and time instead of the buttons', async () => {
    const card = withContent();
    const { app } = await appWith();
    app.local.pendingApprovals.set(card.approval_id, card);
    const at = '2026-01-10T10:44:03.877Z';
    await app.local.apply(event(app.local.id, app.local.lastSeq + 1, { kind: 'approval_decided', approval_id: CARD_ID, state: 'approved', decided_on: "Asha's laptop" }, at));
    // A screen opened after the decision reads the record.
    app.local.pendingApprovals.set(CARD_ID, card);
    const out = html(ApprovalScreen, { backend: app.local, workspaceId: card.workspace_id, instance: CARD_ID }, app);
    expect(text(out)).toContain(`Decided on Asha's laptop, ${formatTime(at)}`);
    expect(buttons(out).map(label)).toEqual(['See the changes']);
    expect(text(out)).toContain('Nothing is sent or changed outside ordin until you choose.');
  });

  it('the conversation\'s one-line card says Waiting for you with Open while it waits', async () => {
    const card = withContent();
    const { app } = await appWith();
    const line = html(ApprovalCardLine, { backend: app.local, card }, app);
    expect(text(line)).toContain('Publish the revised account brief?');
    expect(text(line)).toContain('Waiting for you');
    expect(buttons(line).map(label)).toEqual(['Open']);
  });
});

describe('Error path — an approval already decided elsewhere', () => {
  it('APPR_CARD_ALREADY_DECIDED: that sentence is shown with no code, and the buttons are not offered again', async () => {
    const card = withContent();
    const { app, transport } = await appWith();
    transport.on('approval.decide', err('APPR_CARD_ALREADY_DECIDED', "This was already decided on Asha's laptop."));
    const result = await decide(app.local, card, 'approve');
    expect(result).toMatchObject({ kind: 'closed', error: { message: "This was already decided on Asha's laptop." } });
    // The sentence is drawn by the error line, which never draws the code.
    const sentence = result.kind === 'closed' ? result.error : undefined;
    expect(sentence?.code).toBe('APPR_CARD_ALREADY_DECIDED');
  });

  it('APPR_CARD_CLOSED closes the card the same way; any other refusal leaves the buttons', async () => {
    const card = withContent();
    const { app, transport } = await appWith();
    transport.on('approval.decide', err('APPR_CARD_CLOSED', 'This card has closed.'));
    expect((await decide(app.local, card, 'reject')).kind).toBe('closed');
    expect(transport.sentOf('approval.decide')[0]?.input).toEqual({ card_ref: card.buttons.reject, decision: 'reject' });
    transport.on('approval.decide', err('SOMETHING_ELSE', 'Try again later.', { action: 'try_again' }));
    expect((await decide(app.local, card, 'approve')).kind).toBe('failed');
  });
});

describe('Edge case — server link drops', () => {
  it('a card from a server whose connection is not connected shows "Waiting for [server]" and disabled buttons', async () => {
    const transport = new FakeTransport();
    const handle = createHandle({ id: SERVER, kind: 'server', transport });
    const server = new BackendState({ handle });
    server.groups = new Set(['approval']);
    server.setConnection('not_connected', '2026-01-10T10:42:00Z', { displayName: 'Northwind server' });
    const card = withContent({ backend: SERVER });
    server.pendingApprovals.set(card.approval_id, card);
    const { app } = await appWith();
    const out = html(ApprovalScreen, { backend: server, workspaceId: card.workspace_id, instance: card.approval_id }, app);
    expect(text(out)).toContain('Waiting for Northwind server');
    const [content, approve, reject] = buttons(out);
    expect(content).not.toContain('disabled');
    expect(approve).toContain('disabled');
    expect(reject).toContain('disabled');
    expect(transport.sent).toEqual([]);
    await settle(1);
  });
});

describe('opening the card when approval_requested arrives (step 7; scene approvalOther)', () => {
  const requested = (backend: string, seq: number, over: Record<string, unknown> = {}) =>
    event(backend, seq, { kind: 'approval_requested', card: withContent(over) } as never);

  it('opens as a tab marked new in the card\'s own workspace and leaves the person\'s place', async () => {
    const { app } = await appWith();
    const workspace = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
    const scope = ScreenAreaState.scope(app.local.id, workspace);
    app.screens.open(scope, 'home');
    const key = openApprovalCard(app, app.local, requested(app.local.id, 1));
    expect(key).toBe(`approval:${CARD_ID}`);
    expect(app.screens.front(scope)).toBe('home');
    expect(app.screens.tabs(scope).find((t) => t.key === key)).toMatchObject({ isNew: true, title: 'Publish the revised account brief?' });
  });

  it('comes to the front only when it answers what the person just did', async () => {
    const { app } = await appWith();
    const workspace = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
    const scope = ScreenAreaState.scope(app.local.id, workspace);
    app.screens.open(scope, 'home');
    const key = openApprovalCard(app, app.local, requested(app.local.id, 1), true);
    expect(app.screens.front(scope)).toBe(key);
  });

  it('a card from another workspace opens there without switching the window', async () => {
    const { app } = await appWith();
    app.shell.openWorkspaceTab(app.local.id, 'ws-current');
    const other = '0192f0a1-0000-7000-8000-00000000ffff';
    const key = openApprovalCard(app, app.local, requested(app.local.id, 1, { workspace_id: other }));
    expect(app.shell.openWorkspace).toBe('ws-current');
    expect(app.screens.tabs(ScreenAreaState.scope(app.local.id, other)).some((t) => t.key === key)).toBe(true);
    expect(app.screens.tabs(ScreenAreaState.scope(app.local.id, 'ws-current')).some((t) => t.key === key)).toBe(false);
  });

  it('any other event opens nothing', async () => {
    const { app } = await appWith();
    expect(openApprovalCard(app, app.local, event(LOCAL, 1, { kind: 'needs_you_changed' } as never))).toBeNull();
  });
});

describe('SM-SF-1.4 and SM-SF-1.5 — nothing is approved unseen, and a link shows where it goes', () => {
  const versionCard = (): ApprovalCard =>
    withContent({ content: { kind: 'changes', label: 'See the changes', change_set: { kind: 'file_version', file_version_id: '0192a1c4-0000-7000-8000-0000000000aa' } } });

  it('Approve is off while the exact version has not been read, and Reject is not', async () => {
    const { out } = await screen(versionCard());
    const [, approve, reject] = buttons(out);
    expect(approve).toMatch(/disabled/);
    expect(reject).not.toMatch(/disabled/);
  });

  it('a card with no exact-version reference keeps Approve on', async () => {
    const { out } = await screen(withContent());
    expect(buttons(out)[1]).not.toMatch(/disabled/);
  });

  it('a link in an approval view is drawn as its words and its target, as text', async () => {
    const { default: MarkdownView } = await import('../src/lib/blocks/MarkdownView.svelte');
    const { app } = await appWith();
    const source = 'read [the brief](https://example.com/x)';
    expect(text(html(MarkdownView, { source, showTargets: true }, app))).toContain('the brief (https://example.com/x)');
    expect(text(html(MarkdownView, { source }, app))).not.toContain('example.com');
    expect(html(MarkdownView, { source, showTargets: true }, app)).not.toContain('<a ');
  });
});
