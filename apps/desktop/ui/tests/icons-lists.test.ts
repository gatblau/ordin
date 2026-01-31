// Icons in the fixed screens' lists and the mail screens (ScreensModule.md amendment *Icons*, SM-IC-3;
// cross-cutting/VisualDesign.md V8 *Where icons are drawn*, *Actions*). Rendering is server-side with no
// DOM; a list a screen reads in `onMount` is checked in its source, where the rows cannot be drawn.
import { beforeAll, describe, expect, it } from 'vitest';

import EmailListView from '../src/lib/views/mail/EmailListView.svelte';
import EmailMessageView from '../src/lib/views/mail/EmailMessageView.svelte';
import { EmailListController, EmailMessageController } from '../src/lib/views/mail/mail-controller.svelte';
import FactLine from '../src/lib/views/fixed/FactLine.svelte';
import { AgentNames } from '../src/lib/views/fixed/facts';
import FixedList from '../src/lib/views/fixed/FixedList.svelte';
import HandoverJira from '../src/lib/views/fixed/HandoverJira.svelte';
import SlackChannel from '../src/lib/views/fixed/SlackChannel.svelte';
import TeamPanel from '../src/lib/views/fixed/TeamPanel.svelte';
import { ok } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, WS } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const plain = (markup: string) => markup.replace(/<!--[\s\S]*?-->/g, '').replace(/[⁨⁩]/g, '');
/** Each drawn icon's name, in order. */
const icons = (markup: string): string[] => [...markup.matchAll(/<use href="#i-([a-z]+)"/g)].map((m) => m[1]!);
/** A tile (with its tint, when given) holding `name`, directly followed by `after`. */
const tile = (name: string, tint: string | null, after: string): RegExp =>
  new RegExp(`<span class="icon-tile[^"]*"${tint === null ? '' : ` data-tint="${tint}"`}><svg[^>]*><use href="#i-${name}"></use></svg></span>\\s*${after}`);
/** A button whose content starts with icon `name` and then its words. */
const iconButton = (name: string, words: string): RegExp => new RegExp(`<button[^>]*><svg[^>]*><use href="#i-${name}"></use></svg>\\s*${words}`);

const sources = import.meta.glob('../src/lib/views/{fixed,mail}/*.svelte', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const source = (file: string): string => {
  const hit = Object.entries(sources).find(([path]) => path.endsWith(`/${file}`));
  if (hit === undefined) throw new Error(`no source ${file}`);
  return hit[1];
};

describe('SM-IC-3 — lists', () => {
  it('a fixed list draws its icon and tint in a tile before each row\'s words', async () => {
    const { state } = await backendWith();
    const rows = [{ id: 'a', title: 'Quote checker' }, { id: 'b', title: 'Rates helper' }];
    const out = plain(html(FixedList, { backend: state, heading: 'Agents', rows, icon: 'agent', tint: 'accent-ink', onopen: () => undefined }));
    expect(icons(out)).toEqual(['agent', 'agent']);
    expect(out).toMatch(tile('agent', 'accent-ink', '<button[^>]*>Quote checker'));
    // With no icon the rows draw none.
    expect(icons(plain(html(FixedList, { backend: state, heading: 'x', rows })))).toEqual([]);
  });

  it.each([
    ['Agents.svelte', 'icon="agent" tint="accent-ink"'],
    ['Workflows.svelte', 'icon="keptcrew"'],
    ['Dashboards.svelte', 'icon="chart" tint="sheet"'],
    ['Telegram.svelte', 'icon="chat"'],
    ['Slack.svelte', 'icon="chat"'],
  ])('%s gives its list rows %s', (file, props) => {
    expect(source(file)).toContain(props);
  });

  it('an email message row draws i-mail before its subject', async () => {
    const msg = { attachment_names: [], cc: [], date: '2026-01-16T10:00:00Z', email_link_id: uid('l1'), from: { address: 'bo@acme.test', outside_company: false }, message_ref: 'm1', subject: 'New prices', to: [] };
    const { handle, state } = await backendWith({
      groups: ['service', 'mail', 'workspaces', 'ordin'],
      setup: (tr) => tr.on('workspaces.email_links', ok([{ id: uid('l1'), display_name: 'Inbox', workspace_id: WS }])).on('mail.mark_looked', ok({})).on('mail.list_messages', ok({ items: [msg] })),
    });
    const c = new EmailListController(handle, WS, uid('l1'));
    await c.load();
    const out = plain(html(EmailListView, { backend: state, c }));
    // The whole row is the button: the icon, then the sender, then the subject (scene `mailRead`).
    expect(out).toMatch(tile('mail', null, '<span class="grow">\\s*<b>bo@acme.test</b>[\\s\\S]*New prices'));
  });

  it('a Slack message row draws i-chat before its sender', () => {
    expect(source('SlackChannel.svelte')).toMatch(/<span class="icon-tile"><Icon name="chat" \/><\/span>\s*<span class="grow">\s*<b>\{message.sender_name\}/);
  });

  it('a Jira issue draws i-task: a search result, and each issue a handover can ride on', async () => {
    expect(source('JiraAddDialog.svelte')).toContain('<span class="icon-tile"><Icon name="task" /></span>{issue.issue.key}');
    const { state } = await backendWith();
    const jira = { line: '', options: [{ key: 'ABC-12', summary: 'Rates' }], project: 'Supplier onboarding', selection: { kind: 'create_new' } };
    const out = plain(html(HandoverJira, { backend: state, draftId: 'd', jira: jira as never }));
    expect(icons(out)).toEqual(['task']);
    expect(out).toMatch(tile('task', null, 'ABC-12 Rates'));
  });

  it('a knowledge page row draws i-know in the memory colour', () => {
    const src = source('Knowledge.svelte');
    const knowTile = '<span class="icon-tile" data-tint="memory"><Icon name="know" /></span>';
    // This workspace's pages, other teams' pages, Pages that link here and the links out with a target.
    expect(src.split(knowTile)).toHaveLength(5);
    expect(src).toContain(`${knowTile}<button type="button" class="link" onclick={() => void c.open(ref.page_id)}>`);
  });
});

describe('SM-IC-3 — What Ordin remembers', () => {
  const fact = (cameFrom: Record<string, unknown>) => ({
    id: 'f1', statement: 'Pay Kestrel first.', kind: 'known', true_from: '2026-01-10T00:00:00Z', written_at: '2026-01-10T00:00:00Z',
    came_from: cameFrom, fading: false, may_be_out_of_date: false, needs_check: false,
  });

  it.each([
    [{ kind: 'agent', agent_instance_id: 'a1', run_id: 'r1' }],
    [{ kind: 'agent_citing', agent_instance_id: 'a1', origin_ref: 'quote.pdf', version: '1' }],
  ])('an entry from an agent (%o) draws i-agent in the memory colour before its statement', (cameFrom) => {
    const out = plain(html(FactLine, { fact: fact(cameFrom) as never, names: new AgentNames() }));
    expect(out).toMatch(tile('agent', 'memory', 'Pay Kestrel first\\.'));
  });

  it('an entry not from an agent draws no icon', () => {
    expect(icons(plain(html(FactLine, { fact: fact({ kind: 'person' }) as never, names: new AgentNames() })))).toEqual([]);
  });

  it('a held write from an agent draws i-agent in the memory colour', () => {
    expect(source('Memory.svelte')).toContain('{#if entry.written_by != null}<span class="icon-tile" data-tint="memory"><Icon name="agent" /></span>{/if}{entry.statement}');
  });
});

describe('SM-IC-3 — chips', () => {
  it.each([
    ['rates.xlsx', 'sheet'],
    ['offer.pdf', 'pdf'],
    ['notes.txt', 'word'],
  ])('the attachment %s draws i-%s (small) before its name', async (fileName, name) => {
    const message = {
      attachment_names: [fileName], attachments: [{ bytes: 10, file_name: fileName, index: 0, media_type: 'x' }], cc: [], date: '2026-01-16T10:00:00Z', email_link_id: uid('l1'),
      from: { address: 'bo@acme.test', outside_company: false }, hidden_text: false, message_ref: 'ref-1', subject: 'S', text: 'T', to: [], truncated: false,
    };
    const { handle, state } = await backendWith({ groups: ['service', 'mail', 'ordin'], setup: (tr) => tr.on('mail.open_message', ok(message)) });
    const c = new EmailMessageController(handle, WS, 'ref-1');
    await c.load();
    const out = plain(html(EmailMessageView, { backend: state, workspaceId: WS, c }));
    expect(out).toMatch(new RegExp(`<svg class="icon icon-small"[^>]*><use href="#i-${name}"></use></svg>\\s*${fileName.replace('.', '\\.')}`));
  });
});

describe('SM-IC-3 — actions', () => {
  it('Slack: Remove from this workspace draws i-remove; Open in Slack draws i-open', async () => {
    const { state } = await backendWith();
    expect(plain(html(SlackChannel, { backend: state, workspaceId: WS, instance: uid('l1') }))).toMatch(iconButton('remove', 'Remove from this workspace'));
    expect(source('SlackChannel.svelte')).toMatch(/<Icon name="open" \/>\s*\{t\('slack-open-in-slack'\)\}/);
  });

  it('Team knowledge: Share what we\'ve learned and Share knowledge with a team… draw i-publish; Stop sharing with the team i-stop', async () => {
    const { state } = await backendWith();
    const status = { workspace_id: WS, folders: [], descriptions: [], folder_large: false, held_for_check: 0, problems: [], workspace_facts: 0, workspace_large: false };
    const bound = plain(html(TeamPanel, { backend: state, workspaceId: WS, initialStatus: { ...status, binding: { folder: 'k', access: 'read_write' }, header_line: 'k' } as never }));
    expect(bound).toMatch(iconButton('stop', 'Stop sharing with the team'));
    expect(bound).toMatch(iconButton('publish', 'Share what we'));
    const unbound = plain(html(TeamPanel, { backend: state, workspaceId: WS, initialStatus: status as never }));
    expect(unbound).toMatch(iconButton('publish', 'Share knowledge with a team…'));
  });

  it('Check it: Read it draws i-eye', () => {
    expect(source('CheckItPanel.svelte')).toContain(`<Icon name="eye" />{t('repo-read-it')}`);
  });

  it('the automatic-publishing line draws i-info, labelled by its sentence, in place of the glyph ℹ', () => {
    const src = source('PublishingLine.svelte');
    expect(src).toContain('<span class="icon-tile" role="img" aria-label={sentence}><Icon name="info" /></span>');
    expect(src).not.toContain('ℹ</span>');
  });
});
