// Releases and updates (ADR-114, ADR-072): Settings › About, the update line, What's new, a personal
// server's update row, and release notes as plain text.
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import { err, event, hello, LOCAL, ok, R1_GROUPS, SERVER, settle } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { loaded, text } from './settings-support';
import About from '../src/lib/views/Settings/About.svelte';
import Notes from '../src/lib/views/Settings/Notes.svelte';
import UpdateLine from '../src/lib/views/Settings/UpdateLine.svelte';
import WhatsNewCard from '../src/lib/views/Settings/WhatsNewCard.svelte';
import { AboutModel, DOWNLOAD_POLL_MS, parseNotes, UpdateStore } from '../src/lib/views/Settings/updates.svelte';
import { ServersModel } from '../src/lib/views/Settings/servers-model.svelte';
import Servers from '../src/lib/views/Settings/Servers.svelte';
import { createHandle } from '../src/lib/backend/backend';
import { BackendState } from '../src/lib/store/backend-state.svelte';

beforeAll(loadWindowCatalogue);
afterEach(() => vi.useRealTimers());

const offer = (state: string, version = '1.1.0', notes = '') => ({ offer: { state, version, notes } });

async function about(setup: (t: Awaited<ReturnType<typeof loaded>>['transport']) => void = () => undefined) {
  const env = await loaded(R1_GROUPS, setup);
  const store = new UpdateStore(env.handle);
  const model = new AboutModel(env.handle, async () => '1.0.0', store);
  return { ...env, store, model };
}

describe('Settings › About (SM-UP-2, SM-UP-8)', () => {
  it('Happy path — checking for updates finds a new version', async () => {
    vi.useFakeTimers();
    let polls = 0;
    const t = await about((tr) => {
      tr.on('setup.check_update', ok(offer('downloading')));
      tr.on('setup.update_offer', () => ok(polls++ === 0 ? { offer: null } : offer(polls > 2 ? 'ready' : 'downloading')));
    });
    await t.model.open();
    expect(t.model.version).toBe('1.0.0');
    const pending = t.model.check();
    expect(t.model.status.kind).toBe('checking');
    expect(t.model.busy).toBe(true);
    await pending;
    expect(t.model.status).toEqual({ kind: 'downloading', version: '1.1.0' });
    expect(text(html(About, { backend: t.state, model: t.model }))).toContain('Getting ordin 1.1.0 ready…');
    await vi.advanceTimersByTimeAsync(DOWNLOAD_POLL_MS * 3);
    expect(t.model.status).toEqual({ kind: 'ready', version: '1.1.0' });
    const markup = html(About, { backend: t.state, model: t.model });
    expect(text(markup)).toContain('ordin 1.1.0 is ready.');
    expect(text(markup)).toContain('Restart to update');
    t.model.close();
  });

  it('the button is disabled while the check runs', async () => {
    const t = await about((tr) => tr.on('setup.check_update', ok(offer('ready'))));
    t.model.status = { kind: 'checking' };
    expect(html(About, { backend: t.state, model: t.model })).toMatch(/<button[^>]*disabled[^>]*>Check for updates/);
  });

  it('Edge case — ordin is up to date', async () => {
    const t = await about((tr) => tr.on('setup.check_update', ok({ offer: null })));
    await t.model.check();
    expect(text(html(About, { backend: t.state, model: t.model }))).toContain('ordin is up to date.');
  });

  it('Error path — the check cannot reach the updates', async () => {
    for (const code of ['SETUP_UPDATE_UNREACHABLE', 'SETUP_UPDATE_SIGNATURE_INVALID']) {
      const t = await about((tr) => tr.on('setup.check_update', err(code, 'internal detail')));
      await t.model.check();
      const out = text(html(About, { backend: t.state, model: t.model }));
      expect(out).toContain("ordin couldn't check for updates. Try again later.");
      expect(out).not.toContain(code);
      expect(out).not.toContain('internal detail');
    }
  });

  it('another refusal is drawn by the error line', async () => {
    const t = await about((tr) => tr.on('setup.check_update', err('SETUP_OTHER', 'Something else.')));
    await t.model.check();
    expect(text(html(About, { backend: t.state, model: t.model }))).toContain('Something else.');
  });

  it('Edge case — About on a whole-computer install', async () => {
    const t = await about((tr) => tr.on('setup.check_update', ok(offer('administrator_needed'))));
    await t.model.check();
    const out = text(html(About, { backend: t.state, model: t.model }));
    expect(out).toContain('ordin 1.1.0 is available. This copy was installed for everyone on this computer, so install the new version the same way.');
    expect(out).not.toContain('Restart to update');
  });

  it('opening reads the offer once, so a download that began in the background is shown', async () => {
    const t = await about((tr) => tr.on('setup.update_offer', ok(offer('ready', '1.2.0'))));
    await t.model.open();
    expect(t.model.status).toEqual({ kind: 'ready', version: '1.2.0' });
    expect(t.transport.sentOf('setup.update_offer').length).toBeGreaterThanOrEqual(1);
    expect(t.transport.sentOf('setup.check_update')).toEqual([]);
  });

  it("What's new reads the notes and never marks them seen", async () => {
    const t = await about((tr) => tr.on('setup.whats_new', ok({ whats_new: { version: '1.1.0', notes: '- A', seen: false } })));
    await t.model.openWhatsNew();
    expect(t.model.panel).toEqual({ version: '1.1.0', notes: '- A' });
    expect(t.transport.sentOf('setup.whats_new_seen')).toEqual([]);
    const none = await about((tr) => tr.on('setup.whats_new', ok({ whats_new: null })));
    await none.model.openWhatsNew();
    expect(text(html(About, { backend: none.state, model: none.model }))).toContain('There are no notes for this version.');
  });

  it('the release notes are drawn inside Updates, with no Close button', async () => {
    const t = await about((tr) => tr.on('setup.whats_new', ok({ whats_new: { version: '1.1.0', notes: '- A', seen: false } })));
    await t.model.openWhatsNew();
    const out = text(html(About, { backend: t.state, model: t.model }));
    expect(out).toContain("What's new in ordin 1.1.0");
    expect(out).not.toContain('Close');
  });
});

describe('The update line (SM-UP-3, SM-UP-7)', () => {
  it('Happy path — the update line restarts ordin', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('setup.update_offer', ok(offer('ready')));
      tr.on('setup.install_update', ok({}));
    });
    const store = new UpdateStore(t.handle);
    await store.refresh();
    const out = text(html(UpdateLine, { backend: t.state, store }));
    expect(out).toContain('ordin 1.1.0 is ready.');
    expect(out).toContain('Restart to update');
    expect(out).toContain('Not now');
    await store.restart();
    expect(t.transport.sentOf('setup.install_update')).toHaveLength(1);
    expect(text(html(UpdateLine, { backend: t.state, store }))).toContain('Restarting to update…');
    expect(html(UpdateLine, { backend: t.state, store })).toMatch(/<button[^>]*disabled/);
  });

  it('Edge case — Not now hides the line until the next start', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('setup.update_offer', ok(offer('ready'))));
    const store = new UpdateStore(t.handle);
    await store.refresh();
    store.notNow();
    expect(text(html(UpdateLine, { backend: t.state, store }))).not.toContain('is ready');
    await store.refresh();
    expect(store.line).toBeNull();
    expect(t.transport.sentOf('setup.install_update')).toEqual([]);
    // "until the store is re-created": a new store shows the line again.
    const next = new UpdateStore(t.handle);
    await next.refresh();
    expect(next.line).not.toBeNull();
  });

  it('SETUP_UPDATE_NONE removes the line', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('setup.update_offer', ok(offer('ready')));
      tr.on('setup.install_update', err('SETUP_UPDATE_NONE', 'No update.'));
    });
    const store = new UpdateStore(t.handle);
    await store.refresh();
    await store.restart();
    expect(store.line).toBeNull();
    expect(store.restarting).toBe(false);
  });

  it('SETUP_UPDATE_INSTALL_FAILED is drawn by the error line with Try again', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('setup.update_offer', ok(offer('ready')));
      tr.on('setup.install_update', err('SETUP_UPDATE_INSTALL_FAILED', 'The update could not be installed.', { action: 'try_again' }));
    });
    const store = new UpdateStore(t.handle);
    await store.refresh();
    await store.restart();
    const out = text(html(UpdateLine, { backend: t.state, store }));
    expect(out).toContain('The update could not be installed.');
    expect(out).toContain('Try again');
    expect(store.restarting).toBe(false);
  });

  it('Happy path — a whole-computer install asks for the administrator', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('setup.update_offer', ok(offer('administrator_needed'))));
    const store = new UpdateStore(t.handle);
    await store.refresh();
    const out = text(html(UpdateLine, { backend: t.state, store }));
    expect(out).toContain('ordin 1.1.0 is available. This copy was installed for everyone on this computer, so install the new version the same way.');
    expect(out).toContain('Not now');
    expect(out).not.toContain('Restart to update');
    expect(t.transport.sentOf('setup.install_update')).toEqual([]);
  });

  it('a downloading offer draws no line', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('setup.update_offer', ok(offer('downloading'))));
    const store = new UpdateStore(t.handle);
    await store.refresh();
    expect(store.line).toBeNull();
  });

  it('the line is drawn for the local backend only and only when the group is answered', async () => {
    const t = await loaded(R1_GROUPS.filter((g) => g !== 'setup'), (tr) => tr.on('setup.update_offer', ok(offer('ready'))));
    const store = new UpdateStore(t.handle);
    await store.refresh();
    expect(text(html(UpdateLine, { backend: t.state, store }))).not.toContain('is ready');
  });

  it('starts once: the offer and the notes are read, and the offer is read again on the interval', async () => {
    vi.useFakeTimers();
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('setup.update_offer', ok({ offer: null }));
      tr.on('setup.whats_new', ok({ whats_new: null }));
    });
    const store = new UpdateStore(t.handle);
    await store.start(1000);
    await store.start(1000);
    expect(t.transport.sentOf('setup.update_offer')).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(2100);
    expect(t.transport.sentOf('setup.update_offer')).toHaveLength(3);
    store.stop();
  });
});

describe("What's new at start (SM-UP-4)", () => {
  const whatsNew = (seen: boolean, notes = '- Copilot can now run your work') => ok({ whats_new: { version: '1.1.0', notes, seen } });

  it("Happy path — What's new appears once after an update", async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('setup.whats_new', whatsNew(false));
      tr.on('setup.whats_new_seen', ok({}));
    });
    const store = new UpdateStore(t.handle);
    await store.loadWhatsNew();
    const markup = html(WhatsNewCard, { backend: t.state, store });
    expect(text(markup)).toContain("What's new in ordin 1.1.0");
    expect(markup).toContain('<li>Copilot can now run your work</li>');
    expect(text(markup)).toContain('Got it');
    await store.gotIt();
    expect(t.transport.sentOf('setup.whats_new_seen')[0]?.input).toEqual({ version: '1.1.0' });
    expect(text(html(WhatsNewCard, { backend: t.state, store }))).not.toContain("What's new in");
  });

  it('is not drawn when the notes were seen (About still shows them)', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('setup.whats_new', whatsNew(true)));
    const store = new UpdateStore(t.handle);
    await store.loadWhatsNew();
    expect(text(html(WhatsNewCard, { backend: t.state, store }))).not.toContain("What's new in");
    const model = new AboutModel(t.handle, async () => '1.1.0', store);
    await model.openWhatsNew();
    expect(model.panel).toEqual({ version: '1.1.0', notes: '- Copilot can now run your work' });
  });

  it('a refusal of Got it keeps the card and is drawn by the error line', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('setup.whats_new', whatsNew(false));
      tr.on('setup.whats_new_seen', err('SETUP_X', 'Could not save that.'));
    });
    const store = new UpdateStore(t.handle);
    await store.loadWhatsNew();
    await store.gotIt();
    const out = text(html(WhatsNewCard, { backend: t.state, store }));
    expect(out).toContain("What's new in ordin 1.1.0");
    expect(out).toContain('Could not save that.');
  });

  it('is not counted in the waiting count of Needs you', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('setup.whats_new', whatsNew(false)));
    const store = new UpdateStore(t.handle);
    await store.loadWhatsNew();
    expect(t.state.needsYouCount).toBe(0);
  });
});

describe('Release notes are plain text (SM-UP-6)', () => {
  it('Security — release notes are never markup', () => {
    const markup = html(Notes, { notes: '<img src=x onerror=alert(1)>\n[link](https://example.org)' });
    expect(markup).not.toContain('<img');
    expect(markup).not.toContain('<a ');
    expect(markup).toContain('&lt;img src=x onerror=alert(1)>');
    expect(markup).toContain('[link](https://example.org)');
  });

  it('lines starting with "- " or "* " become list items and line breaks are kept', () => {
    expect(parseNotes('Intro\nmore\n- one\n* two\nafter')).toEqual([
      { kind: 'text', lines: ['Intro', 'more'] },
      { kind: 'list', items: ['one', 'two'] },
      { kind: 'text', lines: ['after'] },
    ]);
    expect(html(Notes, { notes: '- one\n- two' }).replace(/<!--.*?-->/g, '')).toContain('<ul><li>one</li><li>two</li></ul>');
    expect(html(Notes, { notes: 'a\nb' })).toContain('a\nb');
  });
});

// ---- Settings › Servers (SM-UP-5) ----------------------------------------------------------------------

const NORTHWIND = 'Northwind server';
const view = (over: Record<string, unknown> = {}) => ({ server_connection_id: 's1', display_name: NORTHWIND, state: 'connected', backend: SERVER, ...over });

async function servers(update: ReturnType<typeof ok> | ReturnType<typeof err>, listed = [view()]) {
  const env = await loaded(R1_GROUPS, (t) => t.on('servers.list', ok(listed)));
  // The server's own backend: loaded, answering `setup`.
  const serverTransport = env.transport;
  serverTransport.on('setup.server_update', update);
  serverTransport.on('setup.update_server', ok({ accepted: true }));
  const handle = createHandle({ id: SERVER, kind: 'server', transport: serverTransport });
  serverTransport.on('service.hello', (_i, e) => ok(hello(e.backend, R1_GROUPS)));
  const state = new BackendState({ handle });
  await state.load();
  state.setConnection('connected');
  const model = new ServersModel(env.handle, (id) => (id === SERVER ? state : undefined));
  serverTransport.sent.length = 0;
  await model.load();
  return { ...env, model, serverState: state };
}
const update = (self_update: boolean) => ok({ update: { version: '1.1.0', notes: '- New', self_update } });

describe('A personal server’s update (SM-UP-5)', () => {
  it('Happy path — updating a personal server asks first', async () => {
    const t = await servers(update(true));
    const markup = html(Servers, { backend: t.state, model: t.model });
    expect(text(markup)).toContain('Northwind server can update to ordin 1.1.0.');
    expect(text(markup)).toContain('Update server…');
    expect(text(markup)).toContain("What's new in this version");
    t.model.confirmUpdate = 's1';
    const dialog = html(Servers, { backend: t.state, model: t.model });
    expect(text(dialog)).toContain('Update Northwind server to ordin 1.1.0?');
    expect(text(dialog)).toContain('Work running on Northwind server stops and is marked as interrupted. The server restarts within a minute and then continues.');
    // Cancel is the default button, the verb is not.
    expect(dialog).toMatch(/<button[^>]*data-default="true"[^>]*>\s*Cancel/);
    await t.model.updateServer(t.model.servers[0]!);
    const sent = t.transport.sentOf('setup.update_server');
    expect(sent).toHaveLength(1);
    expect(sent[0]?.input).toEqual({ version: '1.1.0' });
    expect(sent[0]?.backend).toBe(SERVER);
    expect(t.model.line('s1')).toEqual({ kind: 'started' });
    expect(text(html(Servers, { backend: t.state, model: t.model }))).toContain('Northwind server is updating. It will be back within a minute.');
  });

  it('Edge case — a server that cannot update itself', async () => {
    const t = await servers(update(false));
    const out = text(html(Servers, { backend: t.state, model: t.model }));
    expect(out).toContain('Northwind server can update to ordin 1.1.0.');
    expect(out).not.toContain('Update server…');
  });

  it('an absent update, or the refusal SETUP_SELF_UPDATE_UNAVAILABLE, draws nothing under the row', async () => {
    for (const reply of [ok({ update: null }), err('SETUP_SELF_UPDATE_UNAVAILABLE', "This server can't update itself.")]) {
      const t = await servers(reply);
      const out = text(html(Servers, { backend: t.state, model: t.model }));
      expect(out).not.toContain('can update to');
      expect(out).not.toContain("can't update itself");
    }
  });

  it('any other refusal is drawn by the error line under the row', async () => {
    const t = await servers(err('SETUP_X', 'The server did not answer.'));
    expect(text(html(Servers, { backend: t.state, model: t.model }))).toContain('The server did not answer.');
  });

  it('Error path — a newer version came out meanwhile', async () => {
    const t = await servers(update(true));
    t.transport.on('setup.update_server', err('SETUP_UPDATE_VERSION_CHANGED', 'A newer version came out meanwhile. Check again.'));
    t.transport.sent.length = 0;
    await t.model.updateServer(t.model.servers[0]!);
    expect(text(html(Servers, { backend: t.state, model: t.model }))).toContain('A newer version came out meanwhile. Check again.');
    expect(t.transport.sentOf('setup.server_update')).toHaveLength(1);
  });

  it('SETUP_UPDATE_REPLACE_FAILED offers Try again, which opens the confirmation again', async () => {
    const t = await servers(update(true));
    t.transport.on('setup.update_server', err('SETUP_UPDATE_REPLACE_FAILED', 'The server could not replace itself.'));
    await t.model.updateServer(t.model.servers[0]!);
    expect(t.model.rowErrors['s1']?.retry).toBe(true);
    expect(text(html(Servers, { backend: t.state, model: t.model }))).toContain('Try again');
    t.model.reopenUpdate(t.model.servers[0]!);
    await settle();
    expect(t.model.confirmUpdate).toBe('s1');
  });

  it('the update is read once on opening and again after server_state_changed for that server', async () => {
    const t = await servers(update(true));
    expect(t.transport.sentOf('setup.server_update')).toHaveLength(1);
    await t.model.onServerStateChanged('s1');
    expect(t.transport.sentOf('setup.server_update')).toHaveLength(2);
  });

  it('a server that is not connected is not asked', async () => {
    const t = await servers(update(true), [view({ state: 'not_connected', as_of: '2026-01-10T10:42:00.000Z' })]);
    expect(t.transport.sentOf('setup.server_update')).toEqual([]);
  });

  it('refused_for_version keeps the row sentence of its older side', async () => {
    const t = await servers(update(true), [view({ state: 'refused_for_version', older_side: 'server' })]);
    expect(text(html(Servers, { backend: t.state, model: t.model }))).toContain("Northwind server is older than ordin. Some things won't work until it's updated.");
    const d = await servers(update(true), [view({ state: 'refused_for_version', older_side: 'desktop' })]);
    expect(text(html(Servers, { backend: d.state, model: d.model }))).toContain('Update ordin on this computer to use Northwind server.');
  });

  it('the server row announces its started line in a polite live region', async () => {
    const t = await servers(update(true));
    await t.model.updateServer(t.model.servers[0]!);
    expect(html(Servers, { backend: t.state, model: t.model }).replace(/<!--.*?-->/g, '')).toMatch(/aria-live="polite">[^<]*It will be back within a minute/);
  });
});
void event;
