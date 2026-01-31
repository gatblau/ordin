// The screens wired to the commands of Phase 11 (ScreensModule.md SM-WR-1 to SM-WR-14): one test per
// added Gherkin scenario where it can be shown without a DOM, and the rules each wiring states.
import { beforeAll, describe, expect, it } from 'vitest';

import { opensScreenAtOnce } from '../src/lib/app/bootstrap';
import { citationsToConfirm, confirmCitation, discardHeld, heldWrites, isNotWired, keepHeld, readFileVersion } from '../src/lib/backend/wiring';
import LinkedChecklist from '../src/lib/blocks/LinkedChecklist.svelte';
import { ReadyListModel } from '../src/lib/blocks/ready-list.svelte';
import { resolveRef } from '../src/lib/blocks/refs';
import EarlierTreeGroup from '../src/lib/shell/EarlierTreeGroup.svelte';
import type { ConversationSummary, SourceLink, WorkingCopyItem } from '../src/lib/service-api/types.gen';
import { offerState } from '../src/lib/views/Settings/updates.svelte';
import { EarlierTreeModel, intendOnOpen, takeIntent } from '../src/lib/views/earlier-tree.svelte';
import { LinkItemsController } from '../src/lib/views/fixed/jira.svelte';
import LinkItems from '../src/lib/views/fixed/LinkItems.svelte';
import { linksCode, readLinksCode } from '../src/lib/views/fixed/links-code';
import { detailFor, InstalledPartPageController } from '../src/lib/views/parts/part-page.svelte';
import { t } from '../src/lib/i18n/fluent';
import { backendWith, WS } from './fixed-support';
import { err, ok, R1_GROUPS } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const text = (markup: string): string => markup.replace(/<!--[\s\S]*?-->/g, '').replace(/<[^>]*>/g, ' ').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ');
const GROUPS = [...R1_GROUPS, 'artefacts', 'confluence'];
const HASH = 'a'.repeat(64);
const VERSION = '0192a1c4-0000-7000-8000-0000000000aa';

describe('SM-WR-1 and SM-WR-2 — the version and held-write commands', () => {
  it('readFileVersion sends files.read_version and returns its outcome', async () => {
    const preview = { kind: 'text', markdown: false, text: 'Rate: 120', truncated: false } as const;
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('files.read_version', ok({ path: 'rates.md', preview })) });
    const out = await readFileVersion(handle, VERSION);
    expect(transport.sentOf('files.read_version')[0]?.input).toEqual({ file_version_id: VERSION });
    expect(out).toEqual({ kind: 'ok', result: { path: 'rates.md', preview } });
  });

  it('Error path — the version is gone: the owner\'s code reaches the card', async () => {
    const { handle } = await backendWith({ setup: (tr) => tr.on('files.read_version', err('FILE_VERSION_NOT_FOUND', 'That version is no longer kept.')) });
    const out = await readFileVersion(handle, VERSION);
    expect(isNotWired(out)).toBe(false);
    expect(out.kind === 'error' && out.error.message).toBe('That version is no longer kept.');
  });

  it('Edge case — an older server lacks the commands: SVC_COMMAND_UNKNOWN is drawn as not_wired', async () => {
    const { handle } = await backendWith({ setup: (tr) => tr.on('memory.held_writes', err('SVC_COMMAND_UNKNOWN', 'Unknown command.')) });
    expect(isNotWired(await heldWrites(handle, WS))).toBe(true);
  });

  it('Happy path — held writes are listed, kept, discarded and citations confirmed with the catalogue inputs', async () => {
    const held = { id: 'h1', statement: 'Pay this supplier first.', things: [], links: [], content_hash: HASH, trust_permille: 400, held_at: '2026-01-26T10:00:00.000Z', needs_check: false };
    const { handle, transport } = await backendWith({
      setup: (tr) =>
        tr
          .on('memory.held_writes', ok([held]))
          .on('memory.keep_held', ok({}))
          .on('memory.discard_held', ok({}))
          .on('memory.citations_to_confirm', ok([uid('f1')]))
          .on('memory.confirm_citation', ok({})),
    });
    const list = await heldWrites(handle, WS);
    expect(list.kind === 'ok' && list.result[0]?.statement).toBe('Pay this supplier first.');
    await keepHeld(handle, WS, 'h1', HASH);
    await discardHeld(handle, WS, 'h1');
    expect(await citationsToConfirm(handle, WS)).toEqual({ kind: 'ok', result: [uid('f1')] });
    await confirmCitation(handle, WS, uid('f1'));
    expect(transport.sentOf('memory.held_writes')[0]?.input).toEqual({ workspace_id: WS });
    expect(transport.sentOf('memory.keep_held')[0]?.input).toEqual({ workspace_id: WS, id: 'h1', content_hash: HASH });
    expect(transport.sentOf('memory.discard_held')[0]?.input).toEqual({ workspace_id: WS, id: 'h1' });
    expect(transport.sentOf('memory.confirm_citation')[0]?.input).toEqual({ workspace_id: WS, id: uid('f1') });
  });
});

describe('SM-WR-3 — a block that cites an artefact', () => {
  const artefact = (location: object) => ({ artefact: { id: uid('a1'), path: 'out/summary.md', location, hash: HASH, media_type: 'text/markdown', created_at: '2026-01-26T10:00:00.000Z', version: 1 } });

  it('Happy path — an artefact in a place is previewed: artefacts.get then files.preview with its place and path', async () => {
    const { state, transport } = await backendWith({
      groups: GROUPS,
      setup: (tr) => tr.on('artefacts.get', ok(artefact({ kind: 'place', place_id: uid('p1') }))).on('files.preview', ok({ kind: 'text', markdown: true, text: '# Summary', truncated: false })),
    });
    const resolved = await resolveRef(state, { kind: 'artefact', artefact_id: uid('a1') });
    expect(transport.commandsSent()).toEqual(['artefacts.get', 'files.preview']);
    expect(transport.sentOf('files.preview')[0]?.input).toEqual({ place_id: uid('p1'), path: 'out/summary.md' });
    expect(resolved).toMatchObject({ kind: 'ok', name: 'summary.md' });
  });

  it('an artefact in an agent folder is named and never read', async () => {
    const { state, transport } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('artefacts.get', ok(artefact({ kind: 'agent_folder', agent_instance_id: 'i1' }))) });
    expect(await resolveRef(state, { kind: 'artefact', artefact_id: uid('a1') })).toEqual({ kind: 'ok', name: 'summary.md', preview: { kind: 'none' } });
    expect(transport.sentOf('files.preview')).toEqual([]);
  });

  it('A block cites an artefact no row has: drawn as unresolvable; an error reply as its sentence', async () => {
    const none = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('artefacts.get', ok({})) });
    expect(await resolveRef(none.state, { kind: 'artefact', artefact_id: uid('a1') })).toEqual({ kind: 'unresolvable' });
    const refused = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('artefacts.get', err('JRNL_DATASTORE_UNAVAILABLE', 'Try again.')) });
    expect(await resolveRef(refused.state, { kind: 'artefact', artefact_id: uid('a1') })).toMatchObject({ kind: 'error' });
  });
});

describe('SM-WR-5 — a linked issue\'s changed item', () => {
  const item = (over: Partial<WorkingCopyItem> = {}): WorkingCopyItem => ({
    id: 'w1',
    source_link_id: uid('l1'),
    origin_path: 'PROJ-12/spec.docx',
    origin_version_read: '3',
    local_version: 1,
    state: 'read',
    read_at: '2026-01-26T09:00:00.000Z',
    ...over,
  });

  it('the items are read with workspaces.working_copy_items; a changed one draws the notice and Bring in the new version', async () => {
    const { state, handle, transport } = await backendWith({ setup: (tr) => tr.on('workspaces.working_copy_items', ok([item({ origin_newer_at: '2026-01-26T10:40:00.000Z' }), item({ id: 'w2', origin_path: 'PROJ-12/notes.md' })])) });
    const c = new LinkItemsController(handle, uid('l1'));
    await c.load();
    expect(transport.sentOf('workspaces.working_copy_items')[0]?.input).toEqual({ source_link_id: uid('l1') });
    const body = text(html(LinkItems, { backend: state, workspaceId: WS, sourceLinkId: uid('l1'), controller: c }));
    expect(body).toContain('spec.docx');
    expect(body).toContain('Changed in Jira,');
    expect((body.match(/Bring in the new version/g) ?? []).length).toBe(1);
  });

  it('Happy path — bringing in a changed Jira item: workspaces.read_item, and the notice is no longer drawn', async () => {
    const { state, handle, transport } = await backendWith({
      setup: (tr) => tr.on('workspaces.working_copy_items', ok([item({ origin_newer_at: '2026-01-26T10:40:00.000Z' })])).on('workspaces.read_item', ok(item({ local_version: 2 }))),
    });
    const c = new LinkItemsController(handle, uid('l1'));
    await c.load();
    expect(await c.bringIn(c.items[0] as WorkingCopyItem)).toBe(true);
    expect(transport.sentOf('workspaces.read_item')[0]?.input).toEqual({ source_link_id: uid('l1'), origin_path: 'PROJ-12/spec.docx' });
    expect(text(html(LinkItems, { backend: state, workspaceId: WS, sourceLinkId: uid('l1'), controller: c }))).not.toContain('Bring in the new version');
  });

  it('Bring in the new version fails: its sentence beside the item, and the notice stays', async () => {
    const { state, handle } = await backendWith({
      setup: (tr) => tr.on('workspaces.working_copy_items', ok([item({ origin_newer_at: '2026-01-26T10:40:00.000Z' })])).on('workspaces.read_item', err('WS_ITEM_TOO_LARGE', 'This document is too large for ordin to read.')),
    });
    const c = new LinkItemsController(handle, uid('l1'));
    await c.load();
    expect(await c.bringIn(c.items[0] as WorkingCopyItem)).toBe(false);
    const body = text(html(LinkItems, { backend: state, workspaceId: WS, sourceLinkId: uid('l1'), controller: c }));
    expect(body).toContain('This document is too large for ordin to read.');
    expect(body).toContain('Bring in the new version');
  });
});

describe('SM-WR-6 — generated types read as they are', () => {
  it('UpdateOffer.state is read from the generated type, administrator_needed included', () => {
    expect(offerState({ state: 'administrator_needed' })).toBe('administrator_needed');
    expect(offerState({ state: 'ready' })).toBe('ready');
    expect(offerState({ state: 'later' } as never)).toBe('other');
  });
});

describe('SM-WR-7 — earlier conversations in the tree', () => {
  const summary = (id: string, started: string, over: Partial<ConversationSummary> = {}): ConversationSummary =>
    ({ conversation_id: id, state: 'earlier', started_at: started, message_count: 2, first_request: `Request ${id}`, ...over }) as ConversationSummary;

  it('Happy path — the tree lists earlier conversations: one node per chain, and the menu holds Read it, Ask Ordin about it and Continue', async () => {
    const page = { items: [summary('A', '2026-01-21T10:00:00.000Z'), summary('B', '2026-01-23T10:00:00.000Z', { continues: 'A' }), summary('C', '2026-01-22T10:00:00.000Z')], next_cursor: null };
    const { state, handle, transport } = await backendWith({ setup: (tr) => tr.on('ordin.list_conversations', ok(page)) });
    const model = new EarlierTreeModel(handle, WS, t);
    await model.load();
    expect(transport.sentOf('ordin.list_conversations')[0]?.input).toEqual({ workspace_id: WS });
    expect(model.nodes.map((n) => n.conversationId)).toEqual(['B', 'C']);
    expect(model.nodes[0]?.name).toBe('Request A');
    const markup = html(EarlierTreeGroup, { backend: state, workspaceId: WS, model, initiallyOpen: true, initialMenu: 'B' }).replace(/[⁨⁩]/g, '');
    expect(markup).toContain('aria-label="More actions for Request A"');
    expect(markup).toContain('title="More actions"');
    const menu = text(markup.slice(markup.indexOf('role="menu"')));
    expect(menu.indexOf('Read it')).toBeLessThan(menu.indexOf('Ask Ordin about it'));
    expect(menu.indexOf('Ask Ordin about it')).toBeLessThan(menu.indexOf('Continue this conversation'));
  });

  it('more than 20 chains: 20 nodes, newest first, then conv-picker-all', async () => {
    const items = Array.from({ length: 21 }, (_, i) => summary(`c${String(i).padStart(2, '0')}`, `2026-10-${String(i + 1).padStart(2, '0')}T10:00:00.000Z`));
    const { handle } = await backendWith({ setup: (tr) => tr.on('ordin.list_conversations', ok({ items, next_cursor: null })) });
    const model = new EarlierTreeModel(handle, WS, t);
    await model.load();
    expect(model.nodes).toHaveLength(21);
    expect(model.nodes[0]?.conversationId).toBe('c20');
    expect(model.nodes[20]).toEqual({ kind: 'all', name: t('conv-picker-all', { count: 21 }) });
  });

  it('Ask and Continue are handed to the read-only tab once', () => {
    intendOnOpen('local', 'B', 'continue');
    expect(takeIntent('local', 'B')).toBe('continue');
    expect(takeIntent('local', 'B')).toBeNull();
  });

  it('the menu items use the catalogue words', () => {
    expect(t('conv-read-it')).toBe('Read it');
    expect(t('tree-node-menu', { name: 'X' }).replace(/[⁨⁩]/g, '')).toBe('Actions for X');
    expect(t('installed-files-show')).toBe('Show the installed files');
  });
});

describe('SM-WR-8 — a block that opens a screen', () => {
  it('form and crew_plan open a screen at once; other kinds do not', () => {
    expect(opensScreenAtOnce({ kind: 'form' })).toBe(true);
    expect(opensScreenAtOnce({ kind: 'crew_plan' })).toBe(true);
    expect(opensScreenAtOnce({ kind: 'message' })).toBe(false);
  });
});

describe('SM-WR-13 — the Ready to publish checklist card', () => {
  it('Happy path — a checklist card for a linked folder: confluence.ready_to_publish, then Publish checked items…', async () => {
    const { state, handle, transport } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('confluence.ready_to_publish', ok([{ kind: 'changed', path: 'Rates/rates.md' }])) });
    const model = new ReadyListModel(handle, uid('L'));
    await model.load();
    expect(transport.sentOf('confluence.ready_to_publish')[0]?.input).toEqual({ confluence_link_id: uid('L') });
    const body = text(html(LinkedChecklist, { backend: state, workspaceId: WS, confluenceLinkId: uid('L'), model }));
    expect(body).toContain('rates.md');
    expect(body).toContain(t('block-publish-checked'));
  });

  it('a refusal draws its sentence in place of the list', async () => {
    const { state, handle } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('confluence.ready_to_publish', err('CONF_LINK_UNKNOWN', 'That folder is no longer linked.')) });
    const model = new ReadyListModel(handle, uid('L'));
    await model.load();
    const body = text(html(LinkedChecklist, { backend: state, workspaceId: WS, confluenceLinkId: uid('L'), model }));
    expect(body).toContain('That folder is no longer linked.');
    expect(body).not.toContain(t('block-publish-checked'));
  });
});

describe('SM-WR-14 — a workspace that links code', () => {
  const link = (kind: SourceLink['kind'], state: SourceLink['state']): SourceLink =>
    ({ id: 'l', workspace_id: WS, kind, state, origin_location: 'x', display_name: 'x', added_at: '2026-01-26T10:00:00.000Z' }) as SourceLink;

  it('Happy path — an active repository folder or remote links code', () => {
    expect(linksCode([link('git_repository_folder', 'active')])).toBe(true);
    expect(linksCode([link('git_remote', 'active')])).toBe(true);
    expect(linksCode([link('local_folder', 'active'), link('jira_issue', 'active')])).toBe(false);
  });

  it('Edge case — a proposed repository link does not count: the part page asks for business_user', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('workspaces.links', ok([link('git_remote', 'proposed')])).on('agents.part_page', ok({})) });
    const c = new InstalledPartPageController(handle, uid('a1'), 'p1');
    await c.load(detailFor(await readLinksCode(handle, WS)));
    expect(transport.sentOf('agents.part_page')[0]?.input).toEqual({ agent_instance_id: uid('a1'), part_id: 'p1', detail: 'business_user' });
  });

  it('a workspace that links code asks for links_code; an error reply is taken as not linking code', async () => {
    const yes = await backendWith({ setup: (tr) => tr.on('workspaces.links', ok([link('git_repository_folder', 'active')])) });
    expect(detailFor(await readLinksCode(yes.handle, WS))).toBe('links_code');
    const no = await backendWith({ setup: (tr) => tr.on('workspaces.links', err('WS_NOT_FOUND', 'Gone.')) });
    expect(await readLinksCode(no.handle, WS)).toBe(false);
  });
});
