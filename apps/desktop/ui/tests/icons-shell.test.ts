// Icons in the window's frame (ScreensModule.md amendment *Icons*, SM-IC-2, SM-IC-3; cross-cutting/
// VisualDesign.md V8 *Where icons are drawn*, *Accessibility*): the rail, the tree, the screen tabs, the
// conversation and the one-line block card. Rendering is server-side with no DOM.
import { beforeAll, describe, expect, it } from 'vitest';

import ApprovalCardLine from '../src/lib/approval/ApprovalCardLine.svelte';
import BlockBody from '../src/lib/blocks/BlockBody.svelte';
import OneLineCard from '../src/lib/blocks/OneLineCard.svelte';
import { blockKindIcon, screenTabIcon } from '../src/lib/icons/kinds';
import ConversationPanel from '../src/lib/shell/ConversationPanel.svelte';
import ScreenTabs from '../src/lib/shell/ScreenTabs.svelte';
import SideRail from '../src/lib/shell/SideRail.svelte';
import { t } from '../src/lib/i18n/fluent';
import { approvalCard, hello, LOCAL, ok, R1_GROUPS } from './support/fake-backend';
import { appWith, html, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const RUN = '0192a1c2-3344-7556-8677-8899aabbccdd';
const noop = () => undefined;
const plain = (markup: string) => markup.replace(/<!--[\s\S]*?-->/g, '').replace(/[⁨⁩]/g, '');

/** Each drawn icon's name, in order. */
const icons = (markup: string): string[] => [...markup.matchAll(/<use href="#i-([a-z]+)"/g)].map((m) => m[1]!);
/** Every `<svg class="icon …">` element's opening tag. */
const iconTags = (markup: string): string[] => [...markup.matchAll(/<svg class="icon[^"]*"[^>]*>/g)].map((m) => m[0]);
/** The buttons whose only content is an icon: no text once tags and comments are removed. */
const iconOnly = (markup: string): string[] =>
  [...plain(markup).matchAll(/<button[^>]*>([\s\S]*?)<\/button>/g)]
    .filter((m) => m[1]!.includes('<svg') && m[1]!.replace(/<[^>]*>/g, '').trim() === '')
    .map((m) => m[0]);

async function treeApp() {
  const { app } = await appWith((tr) => {
    tr.on('workspaces.list', ok([{ id: WS, name: 'Supplier review', created_at: 'x', own_folder_place_id: 'p', state: 'active' }]));
    tr.on('service.hello', ok(hello(LOCAL, R1_GROUPS)));
  });
  return app;
}

describe('SM-IC-3 — the rail and the tree', () => {
  it('Happy path — the tree draws its icons', async () => {
    const app = await treeApp();
    app.prefs.setExpanded([`${LOCAL}:${WS}`]);
    const out = plain(html(SideRail, {}, app));
    const drawn = icons(out);
    for (const name of ['home', 'search', 'plus', 'chev', 'ws', 'docs', 'agent', 'know', 'memory']) expect(drawn, name).toContain(name);
    // The workspace row: the twist, then i-ws; Documents, Agents and Knowledge draw theirs.
    expect(drawn.indexOf('chev')).toBeLessThan(drawn.indexOf('ws'));
    expect(out).toMatch(/<span class="icon-tile"><svg[^>]*><use href="#i-docs"><\/use><\/svg><\/span>Documents/);
    expect(out).toMatch(/<span class="icon-tile" data-tint="memory"><svg[^>]*><use href="#i-know"><\/use><\/svg><\/span>Knowledge/);
    // Each icon is aria-hidden and not focusable.
    for (const tag of iconTags(out)) {
      expect(tag).toContain('aria-hidden="true"');
      expect(tag).toContain('focusable="false"');
    }
  });

  it('Happy path — an icon is decoration: the Home button holds i-home and its name is its words', async () => {
    const app = await treeApp();
    const out = plain(html(SideRail, {}, app));
    const home = /<button[^>]*class="icon-btn rail-home[^"]*"[^>]*>[\s\S]*?<\/button>/.exec(out)?.[0] ?? '';
    expect(home).toContain('<use href="#i-home">');
    expect(home).toContain('aria-label="Home"');
  });

  it('Edge — the tree toggle draws the twist icon and no text character', async () => {
    const app = await treeApp();
    const closed = plain(html(SideRail, {}, app));
    const toggle = /<button[^>]*class="tree-toggle[^"]*"[^>]*>([\s\S]*?)<\/button>/.exec(closed);
    expect(toggle?.[1]).toMatch(/^\s*<svg class="icon icon-twist"[^>]*><use href="#i-chev"><\/use><\/svg>\s*$/);
    expect(toggle?.[0]).toContain('aria-label="Expand"');
    // Opened, the toggle carries `.open`, which the `.icon` rules turn 90°.
    app.prefs.setExpanded([`${LOCAL}:${WS}`]);
    const opened = plain(html(SideRail, {}, app));
    expect(/<button[^>]*class="tree-toggle[^"]*\bopen\b[^"]*"/.test(opened)).toBe(true);
  });
});

describe('SM-IC-3 — the screen tabs', () => {
  it('Edge — an icon-only button has a name and a tooltip: a screen tab\'s Close', () => {
    const tabs = [{ key: 'home' }, { key: 'terminal:1' }];
    const out = plain(html(ScreenTabs, { tabs, front: 'home', titleOf: (tab: { key: string }) => (tab.key === 'home' ? 'Home' : 'Terminal'), iconOf: (tab: { key: string }) => screenTabIcon(tab.key.split(':')[0]!), onselect: noop, onclose: noop }));
    expect(icons(out)).toEqual(['docs', 'remove', 'term', 'remove']);
    const closes = iconOnly(out);
    expect(closes).toHaveLength(2);
    expect(closes[0]).toContain('aria-label="Close Home"');
    expect(closes[0]).toContain('title="Close Home"');
    expect(closes[1]).toContain('aria-label="Close Terminal"');
    expect(out).toContain('class="icon icon-close"');
    expect(out).not.toContain('×');
    expect(out).not.toContain('the conversation is kept');
  });

  it.each([
    ['earlier-conversations', undefined, 'history'],
    ['terminal', undefined, 'term'],
    ['code-editor', undefined, 'code'],
    ['changes-waiting', undefined, 'edit'],
    ['workflow', undefined, 'keptcrew'],
    ['agents', undefined, 'agent'],
    ['earlier-conversation', undefined, 'chat'],
    ['agent-draft', undefined, 'agent'],
    ['approval', undefined, 'info'],
    ['block', 'plan_progress', 'run'],
    ['block', undefined, 'docs'],
    ['settings', undefined, 'docs'],
  ])('a screen tab of %s (%s) draws i-%s', (screen, kind, icon) => {
    expect(screenTabIcon(screen, kind)).toBe(icon);
  });
});

describe('SM-IC-3 — the conversation and the one-line block card', () => {
  it('Happy path — a one-line block card draws its kind\'s icon before its title in the accent text colour', async () => {
    const { app } = await appWith();
    const out = plain(html(OneLineCard, { backend: app.local, workspaceId: WS, blockId: 'p1', block: { kind: 'plan_progress', subject: { kind: 'run', run_id: RUN } } }, app));
    expect(out).toMatch(/<span class="icon-tile" data-tint="accent-ink"><svg class="icon icon-default"[^>]*><use href="#i-run"><\/use><\/svg><\/span>\s*<span class="title[^"]*">/);
  });

  it.each([
    ['documents', 'docs'], ['preview', 'word'], ['document', 'md'], ['changes', 'edit'], ['plan_progress', 'run'], ['result', 'task'],
    ['memory', 'memory'], ['knowledge', 'know'], ['agent_library', 'agent'], ['approval', 'info'], ['table', 'docs'], ['chart', 'docs'],
  ])('the one-line card of %s draws i-%s', (kind, icon) => {
    expect(blockKindIcon(kind)).toBe(icon);
  });

  it('an approval\'s line draws i-info in the accent text colour', async () => {
    const { app } = await appWith();
    const out = plain(html(ApprovalCardLine, { backend: app.local, card: approvalCard() }, app));
    expect(out).toMatch(/<span class="icon-tile" data-tint="accent-ink"><svg[^>]*><use href="#i-info">/);
  });

  it('the Needs you tab draws i-info in its size', async () => {
    const { app } = await appWith((tr) => tr.on('service.hello', ok(hello(LOCAL, R1_GROUPS))));
    const out = plain(html(ConversationPanel, {}, app));
    expect(out).toMatch(/id="conv-tab-needs-you"[^>]*>\s*<svg class="icon icon-needs-tab"[^>]*><use href="#i-info">/);
    expect(iconOnly(out)).toEqual([]);
  });

  it('Edge — a run state keeps its glyph: a finished step\'s ✓ is text in an aria-hidden span, and no sprite icon is drawn for it', async () => {
    const { app } = await appWith();
    app.local.runs.set(RUN, {
      state: 'finished',
      progressLines: [],
      folded: 0,
      count: { done: 1, total: 1 },
      plan: { plan_version: 1, steps: [{ id: 's1', position: 1, line: 'Read the supplier quotes', mark: 'done', plan_version: 1, run_id: RUN, events_seen: [], marks_set: [], came_from: 'agent' }] },
      startUp: [],
    } as never);
    const out = plain(html(BlockBody, { backend: app.local, workspaceId: WS, blockId: 'p1', block: { kind: 'plan_progress', subject: { kind: 'run', run_id: RUN } } }, app));
    expect(out).toContain('<span aria-hidden="true">✓</span>');
    const steps = out.slice(out.indexOf('class="steps'));
    expect(steps.slice(0, steps.indexOf('</ol>'))).not.toContain('<use');
  });
});

describe('SM-IC-3 — catalogue messages of the icon-only buttons', () => {
  it.each([
    ['screen-tab-close', { title: 'Q3 rates' }, 'Close Q3 rates'],
    ['conv-tab-close-named', { name: 'Supplier review' }, 'Close the conversation tab for Supplier review'],
    ['conv-tab-close', {}, 'Close tab · the conversation is kept'],
    ['conv-hide', {}, 'Hide the conversation'],
    ['message-dictate', {}, 'Dictate'],
    ['message-dictate-tip', {}, 'Speak instead of typing'],
    ['more-actions-for', { name: 'Q3 rates' }, 'More actions for Q3 rates'],
    ['more-actions-tip', {}, 'More actions'],
  ])('%s', (id, args, words) => {
    expect(t(id, args).replace(/[⁨⁩]/g, '')).toBe(words);
  });
});
