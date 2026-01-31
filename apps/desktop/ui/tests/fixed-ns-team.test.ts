// SM-NS-13: Adopt, Describe this folder and the recall check (Team knowledge amendment, ADR-110, steps 4, 6, 7).
// The words are read back through `t`, so these tests hold whether or not the catalogue already carries them.
import { beforeAll, describe, expect, it } from 'vitest';

import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import { adoptEdit, TeamController } from '../src/lib/views/fixed/team.svelte';
import AdoptDialog from '../src/lib/views/fixed/AdoptDialog.svelte';
import TeamPanel from '../src/lib/views/fixed/TeamPanel.svelte';
import { err, ok } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));
const clean = (markup: string) => markup.replace(/<!--.*?-->/g, '');

const status = (over: Record<string, unknown> = {}) => ({
  workspace_id: WS, folders: [], descriptions: [], folder_large: false, held_for_check: 0, problems: [], workspace_facts: 0, workspace_large: false,
  binding: { folder: 'knowledge', access: 'read_write' }, header_line: 'acme/supplier-team › knowledge', ...over,
});
const FACT = { kind: 'fact', id: uid('f1') } as const;
const PAGE = { kind: 'page', page_id: uid('p1') } as const;

describe('Adopt into [own folder]… (step 6)', () => {
  it('table: edit carries only the fields the person changed; none gives null', () => {
    const was = { statement: 'S', title: 'T', body: 'B' };
    expect(adoptEdit('fact', was, { ...was })).toBeNull();
    expect(adoptEdit('fact', was, { ...was, statement: 'S2' })).toEqual({ statement: 'S2' });
    expect(adoptEdit('fact', was, { ...was, title: 'ignored' })).toBeNull();
    expect(adoptEdit('page', was, { ...was })).toBeNull();
    expect(adoptEdit('page', was, { ...was, title: 'T2' })).toEqual({ title: 'T2' });
    expect(adoptEdit('page', was, { ...was, title: 'T2', body: 'B2' })).toEqual({ title: 'T2', body: 'B2' });
  });

  it('Happy path — adopting a fact with a change', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('team_knowledge.adopt', ok({ edited: true, id: uid('f1'), kind: 'fact', version: 1 })) });
    const c = new TeamController(handle, WS);
    const out = await c.adopt(FACT, adoptEdit('fact', { statement: 'S', title: '', body: '' }, { statement: 'S edited', title: '', body: '' }));
    expect(out.ok).toBe(true);
    expect(transport.sentOf('team_knowledge.adopt')[0]?.input).toEqual({ ws: WS, item: FACT, edit: { statement: 'S edited' } });
    await Promise.resolve();
    expect(c.announcer.text).toBe(t('team-adopted'));
  });

  it('with no change edit is null; a page sends its changed title and body', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('team_knowledge.adopt', ok({ edited: false, id: uid('p1'), kind: 'page', version: 1 })) });
    const c = new TeamController(handle, WS);
    await c.adopt(PAGE, null);
    await c.adopt(PAGE, { title: 'New', body: 'Body' });
    expect(transport.sentOf('team_knowledge.adopt').map((e) => e.input)).toEqual([
      { ws: WS, item: PAGE, edit: null },
      { ws: WS, item: PAGE, edit: { title: 'New', body: 'Body' } },
    ]);
  });

  it('Adopt again sends team_knowledge.adopt_again on the source\'s current text', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('team_knowledge.adopt_again', ok({ edited: false, id: uid('f1'), kind: 'fact', version: 2 })) });
    await new TeamController(handle, WS).adopt(FACT, null, true);
    expect(transport.sentOf('team_knowledge.adopt_again')[0]?.input).toEqual({ ws: WS, item: FACT, edit: null });
    expect(transport.sentOf('team_knowledge.adopt')).toEqual([]);
  });

  it('Error path — the adoption is refused for personal data; nothing is announced', async () => {
    const { handle } = await backendWith({ setup: (tr) => tr.on('team_knowledge.adopt', err('TEAM_ADOPT_PERSONAL_DATA', "This may hold personal data, so it wasn't adopted. Change the text and try again.")) });
    const c = new TeamController(handle, WS);
    const out = await c.adopt(FACT, { statement: 'x' });
    expect(out.ok).toBe(false);
    expect(out.ok ? '' : out.error.message).toContain('personal data');
    expect(c.announcer.text).toBe('');
  });

  it('the dialog draws its title, source line, the text in an editable field, the help, Cancel and Adopt', async () => {
    const { handle, state } = await backendWith();
    const c = new TeamController(handle, WS);
    const markup = clean(html(AdoptDialog, { controller: c, backend: handle, item: FACT, ownFolder: 'acme/supplier-team', fromFolder: 'acme/platform-team', sharedBy: 'Sam', statement: 'Suppliers are paid in 30 days.', onclose: () => undefined }));
    void state;
    const out = text(markup);
    expect(out).toContain(t('team-adopt-title', { folder: 'acme/supplier-team' }));
    expect(out).toContain(t('team-adopt-from', { folder: 'acme/platform-team', name: 'Sam' }));
    expect(out).toContain(t('team-adopt-help'));
    expect(out).toContain(t('team-adopt-verb'));
    expect(markup).toContain('Suppliers are paid in 30 days.');
    expect(markup).toMatch(/<textarea/);
  });

  it('a page is edited as a title and a body', async () => {
    const { handle } = await backendWith();
    const markup = clean(html(AdoptDialog, { controller: new TeamController(handle, WS), backend: handle, item: PAGE, ownFolder: 'o', fromFolder: 'f', sharedBy: 'n', title: 'Payment terms', body: 'Thirty days.', onclose: () => undefined }));
    expect(markup).toMatch(/<input[^>]*value="Payment terms"/);
    expect(markup).toContain('Thirty days.');
  });

  it('Keep as it is sends team_knowledge.keep_as_is for the item', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('team_knowledge.keep_as_is', ok({})) });
    expect(await new TeamController(handle, WS).keepAsIs(FACT)).toBe(true);
    expect(transport.sentOf('team_knowledge.keep_as_is')[0]?.input).toEqual({ ws: WS, item: FACT });
  });
});

describe('Describe this folder… (step 4)', () => {
  it('sends the path and text; an emptied field sends null', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('team_knowledge.set_description', ok(status())) });
    const c = new TeamController(handle, WS);
    await c.setDescription('', 'Supplier rules');
    await c.setDescription('rates', '');
    expect(transport.sentOf('team_knowledge.set_description').map((e) => e.input)).toEqual([
      { ws: WS, path: '', text: 'Supplier rules' },
      { ws: WS, path: 'rates', text: null },
    ]);
  });

  it('Describe this folder… is offered on an own folder with read-write access only', async () => {
    const { state } = await backendWith({ groups: ['team_knowledge', 'workspaces'] });
    const rw = text(clean(html(TeamPanel, { backend: state, workspaceId: WS, initialStatus: status() })));
    expect(rw).toContain(t('team-describe'));
    const ro = text(clean(html(TeamPanel, { backend: state, workspaceId: WS, initialStatus: status({ binding: { folder: 'knowledge', access: 'read_only' } }) })));
    expect(ro).not.toContain(t('team-describe'));
    const none = text(clean(html(TeamPanel, { backend: state, workspaceId: WS, initialStatus: status({ binding: null }) })));
    expect(none).not.toContain(t('team-describe'));
  });

  it('Error path — a refusal is kept by the controller for the dialog to draw', async () => {
    const { handle } = await backendWith({ setup: (tr) => tr.on('team_knowledge.set_description', err('TEAM_DESCRIPTION_INVALID', 'A description holds 1 to 500 characters.')) });
    const c = new TeamController(handle, WS);
    expect(await c.setDescription('', 'x')).toBe(false);
    expect(c.error?.message).toBe('A description holds 1 to 500 characters.');
  });
});

describe('Check how well ordin finds this knowledge (step 7)', () => {
  it('sends team_knowledge.check_recall and keeps the busy status', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('team_knowledge.check_recall', ok(status({ busy: 'checking_recall' }))) });
    const c = new TeamController(handle, WS);
    await c.checkRecall();
    expect(transport.sentOf('team_knowledge.check_recall')[0]?.input).toEqual({ ws: WS });
    expect(c.busy).toBe(true);
  });

  it('the panel offers the check, and draws one line per folder with percent = recall_at_5_permille / 10, rounded', async () => {
    const { state } = await backendWith({ groups: ['team_knowledge', 'workspaces'] });
    const initialStatus = status({
      last_recall_check: {
        ran_at: '2026-01-10T10:00:00Z',
        folders: [
          { folder_id: null, name: 'acme/supplier-team', checks: 12, recall_at_5_permille: 833, mrr_permille: 0, precision_at_5_permille: 0 },
          { folder_id: 'f2', name: 'acme/platform-team', checks: 4, recall_at_5_permille: 500, mrr_permille: 0, precision_at_5_permille: 0 },
        ],
      },
    });
    const out = text(clean(html(TeamPanel, { backend: state, workspaceId: WS, initialStatus })));
    expect(out).toContain(t('team-check-recall'));
    expect(out).toContain(t('team-recall-result', { folder: 'acme/supplier-team', percent: 83, checks: 12 }));
    expect(out).toContain(t('team-recall-result', { folder: 'acme/platform-team', percent: 50, checks: 4 }));
  });

  it('Edge case — a report with no folders says no folder has questions to check yet', async () => {
    const { state } = await backendWith({ groups: ['team_knowledge', 'workspaces'] });
    const out = text(clean(html(TeamPanel, { backend: state, workspaceId: WS, initialStatus: status({ last_recall_check: { ran_at: '2026-01-10T10:00:00Z', folders: [] } }) })));
    expect(out).toContain(t('team-recall-no-checks'));
  });

  it('while checking, the busy line shows and the check is disabled', async () => {
    const { state } = await backendWith({ groups: ['team_knowledge', 'workspaces'] });
    const markup = clean(html(TeamPanel, { backend: state, workspaceId: WS, initialStatus: status({ busy: 'checking_recall' }) }));
    expect(text(markup)).toContain(t('team-busy-checking-recall'));
    expect(markup).toMatch(/<button[^>]*disabled[^>]*>\s*[^<]*(Check how well|team-check-recall)/);
  });
});

describe('A folder read by this workspace lists what was not taken in', () => {
  it('TEAM_ITEM_HELD_ELSEWHERE and TEAM_DESCRIPTION_NOT_TAKEN are drawn as their messages', async () => {
    const { state } = await backendWith({ groups: ['team_knowledge', 'workspaces'] });
    const folder = {
      folder_id: 'f2', name: 'acme/platform-team', access: 'read_only', bundle_kind: 'team_folder', folder_large: false, held: { facts: 3, pages: 1 }, may_stop: true, set_by_company: false,
      problems: [
        { code: 'TEAM_ITEM_HELD_ELSEWHERE', path: 'rates.md', first_seen_at: '2026-01-10T10:00:00Z' },
        { code: 'TEAM_DESCRIPTION_NOT_TAKEN', path: '', first_seen_at: '2026-01-10T10:00:00Z' },
      ],
    };
    const out = text(clean(html(TeamPanel, { backend: state, workspaceId: WS, initialStatus: status({ folders: [folder] }) })));
    expect(out).toContain(t('team-item-elsewhere', { file: 'rates.md' }));
    expect(out).toContain(t('team-description-not-taken'));
  });
});
