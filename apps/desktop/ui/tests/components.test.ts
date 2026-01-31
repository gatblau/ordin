import { beforeAll, beforeEach, describe, expect, it } from 'vitest';

import ConversationPanel from '../src/lib/shell/ConversationPanel.svelte';
import NeedsYouTab from '../src/lib/shell/NeedsYouTab.svelte';
import ScreenArea from '../src/lib/shell/ScreenArea.svelte';
import ScreenTabs from '../src/lib/shell/ScreenTabs.svelte';
import SideRail from '../src/lib/shell/SideRail.svelte';
import { clearScreens, registerScreen } from '../src/lib/shell/screens';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import ConfirmDialog from '../src/lib/views/ConfirmDialog.svelte';
import ErrorLine from '../src/lib/views/ErrorLine.svelte';
import QuitDialog from '../src/lib/views/QuitDialog.svelte';
import { createHandle } from '../src/lib/backend/backend';
import { approvalCard, event, LOCAL, ok, R1_GROUPS, hello, FakeTransport, settle } from './support/fake-backend';
import Probe from './support/Probe.svelte';
import { appWith, html, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const noop = () => undefined;
const text = (markup: string) => markup.replace(/<[^>]*>/g, ' ').replace(/[\u2068\u2069]/g, '').replace(/\s+/g, ' ');
const buttons = (markup: string) => [...markup.matchAll(/<button[^>]*>/g)].map((m) => m[0]);

describe('ConfirmDialog', () => {
  it('is a modal dialog labelled by its heading, which can take focus', () => {
    const out = html(ConfirmDialog, { title: 'Remove workspace?', body: 'Its links stay.', verb: 'Remove', onconfirm: noop, oncancel: noop });
    expect(out).toMatch(/role="dialog"/);
    expect(out).toMatch(/aria-modal="true"/);
    const id = /aria-labelledby="([^"]+)"/.exec(out)?.[1];
    expect(out).toMatch(new RegExp(`<h2 id="${id}"[^>]*tabindex="-1"`));
  });

  it('a verb that removes, discards or stops is never the default button; Cancel is', () => {
    const out = html(ConfirmDialog, { title: 'Stop crew?', verb: 'Stop', onconfirm: noop, oncancel: noop });
    const [verb, cancel] = buttons(out); // the verb first, then Cancel (scene `removeWs`)
    expect(cancel).toContain('data-default="true"');
    expect(verb).not.toContain('data-default');
  });

  it('a harmless verb may be the default', () => {
    const out = html(ConfirmDialog, { title: 'Continue?', verb: 'Continue', destructive: false, onconfirm: noop, oncancel: noop });
    const [verb, cancel] = buttons(out); // the verb first, then Cancel (scene `removeWs`)
    expect(cancel).not.toContain('data-default');
    expect(verb).toContain('data-default="true"');
  });
});

describe('QuitDialog', () => {
  it('Edge case — quitting with agents working: asks "Quit ordin?", Quit is not the default, Cancel sends nothing', async () => {
    const transport = new FakeTransport();
    const backend = createHandle({ id: LOCAL, kind: 'local', transport });
    const out = html(QuitDialog, { count: 2, backend, oncancel: noop });
    expect(text(out)).toContain('Quit ordin?');
    expect(text(out)).toContain('2 agents are still working.');
    const [quit, cancel] = buttons(out);
    expect(cancel).toContain('data-default="true"');
    expect(quit).not.toContain('data-default');
    expect(transport.sent).toEqual([]);
  });
});

describe('ErrorLine', () => {
  const error = (e: object) => ({ error: { code: 'X_CODE', message: 'Something happened.', ...e } });

  it('Error path — command error with an action: the sentence and one Try again button, never the code', () => {
    const out = html(ErrorLine, error({ message: "Can't reach the company's agent library right now", action: 'try_again' }));
    expect(text(out)).toContain("Can't reach the company's agent library right now");
    expect(buttons(out).length).toBe(1);
    expect(text(out)).toContain('Try again');
    expect(out).not.toContain('X_CODE');
  });

  it('Happy path — an error with a reference shows the message and the reference, and no Send to IT even when the backend names it (SM-ER-3)', () => {
    const out = html(ErrorLine, error({ action: 'send_to_it' as never, reference: 'r-81' }));
    expect(text(out)).toContain('Something happened.');
    expect(text(out)).not.toContain('Send to IT');
    expect(text(out)).toContain('Reference r-81');
  });

  it('an error without a reference draws no reference line; without an action, no button', () => {
    const out = html(ErrorLine, error({}));
    expect(out).not.toContain('Reference');
    expect(buttons(out)).toEqual([]);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-ER-1
  it('Error path — the bridge cannot be reached on this computer: its sentence and one Try again (SM-ER-1)', () => {
    const out = html(ErrorLine, { error: { code: 'UI_IPC_FAILED', message: '' } });
    expect(text(out)).toContain("The window couldn't reach the rest of ordin. Close ordin and open it again.");
    expect(buttons(out).length).toBe(1);
    expect(out).not.toContain('UI_IPC_FAILED');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-ER-1
  it('Error path — a local command with no answer says so (SM-ER-1)', () => {
    const out = html(ErrorLine, { error: { code: 'UI_COMMAND_TIMEOUT', message: '' } });
    expect(text(out)).toContain('That took too long and got no answer. Try again.');
    expect(out).not.toContain('UI_COMMAND_TIMEOUT');
  });

  it('Error path — the bridge cannot be reached for a server draws the server sentence', () => {
    const out = html(ErrorLine, { error: { code: 'UI_COMMAND_TIMEOUT', message: '' }, serverName: 'Northwind server' });
    expect(text(out)).toContain("Can't reach Northwind server right now. ordin will keep trying while it's open.");
    expect(text(out)).toContain('Try again');
  });

  it('Error path — a command meets an older backend: the sentence with no action', () => {
    const out = html(ErrorLine, { error: { code: 'SVC_NOT_IN_THIS_VERSION', message: 'raw', action: 'try_again' } });
    expect(text(out)).toContain('This needs a newer version of ordin.');
    expect(buttons(out)).toEqual([]);
  });

  it('Error path — an approval already decided elsewhere shows that sentence with no code', () => {
    const out = html(ErrorLine, error({ code: 'APPR_CARD_ALREADY_DECIDED', message: "This was already decided on Asha's laptop." }));
    expect(text(out)).toContain("This was already decided on Asha's laptop.");
    expect(out).not.toContain('APPR_CARD_ALREADY_DECIDED');
  });

  it('draws nothing for no error', () => {
    expect(html(ErrorLine, { error: null }).replace(/<!--.*?-->/g, '').trim()).toBe('');
  });
});

describe('ScreenTabs', () => {
  it('is a WAI-ARIA tab list with one tab in the tab order', () => {
    const tabs = [{ key: 'home' }, { key: 'settings' }];
    const out = html(ScreenTabs, { tabs, front: 'settings', titleOf: (t: { key: string }) => t.key, iconOf: () => 'docs', onselect: noop, onclose: noop });
    expect(out).toMatch(/role="tablist"/);
    const tabButtons = [...out.matchAll(/<button[^>]*role="tab"[^>]*>/g)].map((m) => m[0]);
    expect(tabButtons.length).toBe(2);
    expect(tabButtons[0]).toContain('aria-selected="false"');
    expect(tabButtons[0]).toContain('tabindex="-1"');
    expect(tabButtons[1]).toContain('aria-selected="true"');
    expect(tabButtons[1]).toContain('tabindex="0"');
  });
});

describe('SideRail and WorkspaceTree (SM-UP-1)', () => {
  const workspaces = [{ id: WS, name: 'Q4 forecast', created_at: 'x', own_folder_place_id: 'p', state: 'active' }];

  it('Behavioural check — a recorded hello without mail, telegram or slack: no such entry in the rail or the tree', async () => {
    const { app } = await appWith((t) => {
      t.on('workspaces.list', ok(workspaces));
      t.on('service.hello', ok(hello(LOCAL, R1_GROUPS)));
    });
    app.prefs.setExpanded([`${LOCAL}:${WS}`]);
    const out = html(SideRail, {}, app);
    const words = text(out);
    expect(words).toContain('Q4 forecast');
    expect(words).toContain('Documents');
    expect(words).toContain('Knowledge');
    for (const absent of ['Email', 'Telegram', 'Slack']) expect(words).not.toContain(absent);
    expect(R1_GROUPS).not.toContain('mail');
  });

  it('Happy path — an entry of a group the build has is drawn; the same hello with mail adds Email', async () => {
    const { app } = await appWith((t) => {
      t.on('workspaces.list', ok(workspaces));
      t.on('service.hello', ok(hello(LOCAL, [...R1_GROUPS, 'mail', 'slack'])));
    });
    app.prefs.setExpanded([`${LOCAL}:${WS}`]);
    const words = text(html(SideRail, {}, app));
    expect(words).toContain('Email');
    expect(words).toContain('Slack');
    expect(words).not.toContain('Telegram');
  });

  it('with an empty set of groups only the entries that need no group remain', async () => {
    const { app } = await appWith((t) => t.on('service.hello', ok(hello(LOCAL, ['service']))));
    const words = text(html(SideRail, {}, app));
    expect(words).not.toContain('Search or go to');
    expect(words).not.toContain('New workspace');
    expect(words).not.toContain('Home');
  });

  it('draws a server group "On [server]" and, when not connected, "Not connected · as of"', async () => {
    const SERVER = '0192f100-0000-7000-8000-0000000000b1';
    const { app } = await appWith((t) => {
      t.on('servers.list', ok([{ server_connection_id: 'c1', display_name: 'Northwind server', state: 'not_connected', as_of: '10:42', backend: SERVER }]));
    });
    expect(text(html(SideRail, {}, app))).toContain('Not connected · as of 10:42');
    app.stateFor(SERVER)?.setConnection('connected', null, { displayName: 'Northwind server' });
    expect(text(html(SideRail, {}, app))).toContain('On Northwind server');
  });

  it('Edge case — server of a different version: the sentence names the side to update', async () => {
    const SERVER = '0192f100-0000-7000-8000-0000000000b1';
    const { app } = await appWith((t) => {
      t.on('servers.list', ok([{ server_connection_id: 'c1', display_name: 'Northwind server', state: 'refused_for_version', older_side: 'desktop', backend: SERVER }]));
    });
    expect(text(html(SideRail, {}, app))).toContain('Update ordin on this computer to use Northwind server.');
    app.stateFor(SERVER)?.setConnection('refused_for_version', null, { olderSide: 'server' });
    expect(text(html(SideRail, {}, app))).toContain("Northwind server is older than ordin. Some things won't work until it's updated.");
  });
});

describe('ScreenArea', () => {
  beforeEach(clearScreens);

  it('draws the registered screen in front for the open workspace, by its key', async () => {
    registerScreen({ id: 'probe', titleId: 'rail-home', component: Probe });
    const { app } = await appWith();
    app.shell.openWorkspaceTab(LOCAL, WS);
    app.screens.open(ScreenAreaState.scope(LOCAL, WS), 'probe:abc');
    const out = html(ScreenArea, {}, app);
    expect(text(out)).toContain(`probe ${WS} abc`);
    expect(out).toMatch(/role="tabpanel"/);
  });

  it('SM-UP-1 — a screen whose commands the backend does not answer is neither listed nor drawn', async () => {
    registerScreen({ id: 'mailbox', titleId: 'tree-email', component: Probe, commands: ['mail.list_messages'] });
    const { app } = await appWith();
    app.screens.open(ScreenAreaState.scope(LOCAL, null), 'mailbox');
    const out = html(ScreenArea, {}, app);
    expect(text(out)).not.toContain('Email');
    expect(out).not.toContain('class="probe"');
  });

  it('an unregistered fixed screen draws its placeholder with the catalogue title', async () => {
    const { app } = await appWith();
    app.screens.open(ScreenAreaState.scope(LOCAL, null), 'settings');
    expect(text(html(ScreenArea, {}, app))).toContain('Settings');
  });
});

describe('Conversation panel and Needs you', () => {
  it('Happy path — an approval is listed under its workspace in Needs you with Open', async () => {
    const { app, transport } = await appWith((t) => t.on('workspaces.list', ok([{ id: WS, name: 'Q4 forecast', created_at: 'x', own_folder_place_id: 'p', state: 'active' }])));
    transport.emit(event(LOCAL, 1, { kind: 'approval_requested', card: approvalCard() }));
    await settle();
    const out = text(html(NeedsYouTab, { backends: app.stores.all() }, app));
    expect(out).toContain('Q4 forecast');
    expect(out).toContain('Publish the revised account brief?');
    expect(out).toContain('Open');
    expect(text(html(ConversationPanel, {}, app))).toMatch(/Needs you\s+1/);
  });

  it('Error path — markup in data is shown as text: no element is created', async () => {
    const { app } = await appWith();
    const block = { kind: 'message', text: '<img src=x onerror=alert(1)>', attachments: [] } as never;
    app.local.conversation.set(WS, [{ block_id: 'b1', workspace_id: WS, block, seq: 1, at: 'x', waiting: false }]);
    app.shell.openWorkspaceTab(LOCAL, WS);
    const out = html(ConversationPanel, {}, app);
    expect(out).toContain('&lt;img src=x onerror=alert(1)>');
    expect(out).not.toContain('<img');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-MSG-1
  it('SM-MSG-1 — Ordin\'s message is formatted; the person\'s own is shown as typed', async () => {
    const { app } = await appWith();
    const message = (text: string) => ({ kind: 'message', text, attachments: [] }) as never;
    app.local.conversation.set(WS, [
      { block_id: 'mine', workspace_id: WS, block: message('**not bold**'), seq: 1, at: 'x', waiting: false, from: 'person' },
      { block_id: 'reply', workspace_id: WS, block: message('**Done.**'), seq: 2, at: 'x', waiting: false },
    ]);
    app.shell.openWorkspaceTab(LOCAL, WS);
    const out = html(ConversationPanel, {}, app);
    expect(out).toContain('**not bold**');
    expect(out).toMatch(/<strong[^>]*>Done\.<\/strong>/);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-CL-1
  it('no item can be closed: no Close, no Show again, no Show closed items, no Close finished', async () => {
    const { app } = await appWith();
    const q = { kind: 'question', question: 'Which?', choices: ['a', 'b'], allow_other: false } as never;
    const m = { kind: 'message', text: 'done', attachments: [] } as never;
    const table = { kind: 'table', title: 'Suppliers', columns: [], rows: [] } as never;
    const offer = { kind: 'result', of: 'offer', offer: 'save_as_workflow', run_id: 'r' } as never;
    app.local.conversation.set(WS, [
      { block_id: 'q', workspace_id: WS, block: q, seq: 1, at: 'x', waiting: true },
      { block_id: 'm', workspace_id: WS, block: m, seq: 2, at: 'x', waiting: false },
      { block_id: 't', workspace_id: WS, block: table, seq: 3, at: 'x', waiting: false },
      { block_id: 'o', workspace_id: WS, block: offer, seq: 4, at: 'x', waiting: true },
    ]);
    app.shell.openWorkspaceTab(LOCAL, WS);
    const out = html(ConversationPanel, {}, app);
    expect(out).not.toContain('item-close');
    expect(out).not.toMatch(/aria-label="Close"/);
    expect(out).not.toContain('Show again');
    expect(out).not.toContain('Show closed items');
    expect(out).not.toContain('Close finished items');
  });

  it('the Clear confirmation is asked before anything is sent', async () => {
    const { app, transport } = await appWith();
    app.shell.openWorkspaceTab(LOCAL, WS);
    html(ConversationPanel, {}, app);
    expect(transport.commandsSent()).not.toContain('ordin.start_fresh');
  });
});
