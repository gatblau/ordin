import { describe, expect, it } from 'vitest';

import { startApp } from '../src/lib/app/bootstrap';
import { t } from '../src/lib/i18n/fluent';
import type { ApprovalCard } from '../src/lib/service-api/types.gen';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import { makeKey, SCREEN_IDS } from '../src/lib/shell/screens';
import { answers } from '../src/lib/backend/groups';
import { approvalCard, event, FakeTransport, hello, LOCAL, ok, R1_GROUPS, SERVER, settle } from './support/fake-backend';
import { uid } from './support/ids';

const noDoc = { document: null, storage: null } as const;

describe('startApp', () => {
  // Plan 04: the tree saves an expanded workspace as `<backend id>:<workspace id>`; start-up passed the key
  // whole to `workspaces.tree`, which the core refused, so after a restart the tree never loaded.
  it('a workspace expanded before the restart has its tree read with its own id', async () => {
    const ws = uid('ws-expanded');
    const saved = new Map<string, string>([['ordin.view.tree.expanded', JSON.stringify([`${LOCAL}:${ws}`, `${SERVER}:${uid('elsewhere')}`])]]);
    const transport = new FakeTransport();
    transport.on('workspaces.tree', ok({}));
    await startApp(transport, { document: null, storage: { getItem: (k) => saved.get(k) ?? null, setItem: (k, v) => void saved.set(k, v) } });
    expect(transport.sentOf('workspaces.tree').map((e) => e.input)).toEqual([{ workspace_id: ws }]);
  });

  // The window stayed in message ids ("rail-home") because the catalogue went into an I18n the screens never read.
  it('loads the catalogue into the text the screens read, when no i18n is given', async () => {
    await startApp(new FakeTransport(), noDoc);
    expect(t('rail-home')).toBe('Home');
  });

  it('opens the event and stream channels before the first data command, with an empty after', async () => {
    const transport = new FakeTransport();
    const order: string[] = [];
    const open = transport.openEvents.bind(transport);
    transport.openEvents = async (...args) => {
      order.push(`events:${transport.sent.length}`);
      return open(...args);
    };
    await startApp(transport, noDoc);
    expect(order).toEqual(['events:0']);
    expect(transport.opened[0]?.after).toEqual({});
  });

  it('loads the local backend and keeps service.hello groups (SM-UP-1)', async () => {
    const transport = new FakeTransport({ localGroups: ['service', 'home', 'search'] });
    const app = await startApp(transport, noDoc);
    expect(app.local.loaded).toBe(true);
    expect(answers(app.local, 'home.view')).toBe(true);
    expect(answers(app.local, 'mail.list_messages')).toBe(false);
    expect(app.i18n.t('rail-home')).toBe('Home');
  });

  it('events that arrive before the stores exist are applied once they do', async () => {
    const transport = new FakeTransport();
    const open = transport.openEvents.bind(transport);
    transport.openEvents = async (after, onBatch) => {
      const opened = await open(after, onBatch);
      onBatch({ events: [event(LOCAL, 5, { kind: 'run_state_changed', run_id: 'r1', state: 'running' })] });
      return opened;
    };
    transport.on('service.hello', ok(hello(LOCAL, R1_GROUPS, 4)));
    const app = await startApp(transport, noDoc);
    await settle();
    expect(app.local.runs.get('r1')?.state).toBe('running');
    expect(app.local.lastSeq).toBe(5);
  });

  it('events pushed later reach the store', async () => {
    const transport = new FakeTransport();
    const app = await startApp(transport, noDoc);
    transport.emit(event(LOCAL, 1, { kind: 'approval_requested', card: approvalCard() }));
    await settle();
    expect(app.local.pendingApprovals.size).toBe(1);
  });

  it('FU-134 — an approval request opens its card as a new tab in its workspace and the card is kept after it is decided', async () => {
    const transport = new FakeTransport();
    const app = await startApp(transport, noDoc);
    const card: ApprovalCard = approvalCard() as ApprovalCard;
    transport.emit(event(LOCAL, 1, { kind: 'approval_requested', card }));
    await settle();
    const scope = ScreenAreaState.scope(LOCAL, card.workspace_id);
    expect(app.screens.tabs(scope)).toEqual([{ key: makeKey(SCREEN_IDS.approval, card.approval_id), isNew: true }]);
    expect(app.screens.front(scope)).toBe(makeKey(SCREEN_IDS.approval, card.approval_id));
    transport.emit(event(LOCAL, 2, { kind: 'approval_decided', approval_id: card.approval_id, state: 'approved', decided_on: 'here' }));
    await settle();
    expect(app.local.pendingApprovals.size).toBe(0);
    expect(app.local.seenCards.get(card.approval_id)?.verb).toBe(card.verb);
  });

  it('Edge case — server link drops and returns', async () => {
    const transport = new FakeTransport();
    transport.on('servers.list', ok([{ server_connection_id: 'c1', display_name: 'Northwind server', state: 'connected', backend: SERVER }]));
    transport.on('service.hello', (_input, envelope) => ok(hello(envelope.backend, R1_GROUPS, 0, envelope.backend === SERVER ? 'personal_server' : 'desktop')));
    const app = await startApp(transport, noDoc);
    await settle();
    const server = app.stateFor(SERVER);
    expect(server?.loaded).toBe(true);
    expect(app.backends.servers().map((h) => h.id)).toEqual([SERVER]);

    transport.emit(event(LOCAL, 1, { kind: 'server_state_changed', server_connection_id: 'c1', state: 'not_connected', as_of: '10:42' } as never));
    await settle();
    expect(server?.connection).toBe('not_connected');
    expect(server?.asOf).toBe('10:42');

    const loads = transport.sent.filter((e) => e.backend === SERVER && e.command === 'service.hello').length;
    transport.emit(event(LOCAL, 2, { kind: 'server_state_changed', server_connection_id: 'c1', state: 'connected' } as never));
    await settle();
    expect(server?.connection).toBe('connected');
    expect(transport.sent.filter((e) => e.backend === SERVER && e.command === 'service.hello').length).toBe(loads + 1);
  });

  it('Edge case — server of a different version: the state keeps the side that is older', async () => {
    const transport = new FakeTransport();
    transport.on('servers.list', ok([{ server_connection_id: 'c1', display_name: 'Northwind server', state: 'refused_for_version', older_side: 'desktop', backend: SERVER }]));
    const app = await startApp(transport, noDoc);
    const server = app.stateFor(SERVER);
    expect(server?.connection).toBe('refused_for_version');
    expect(server?.olderSide).toBe('desktop');
    expect(server?.displayName).toBe('Northwind server');
    expect(server?.loaded).toBe(false);
  });

  it('Happy path — a locale change redraws: the catalogue is asked again for the new locale', async () => {
    const transport = new FakeTransport();
    const app = await startApp(transport, noDoc);
    transport.emit(event(LOCAL, 1, { kind: 'locale_changed', active: { locale: 'fr-FR', source: 'person', direction: 'ltr', fallback_chain: ['fr-FR', 'en-GB'] } }));
    await settle();
    expect(transport.sentOf('localisation.get_catalogue').at(-1)?.input).toEqual({ locale: 'fr-FR' });
    expect(app.i18n.locale).toBe('fr-FR');
  });

  it('a brand change reads brand.get again', async () => {
    const transport = new FakeTransport();
    await startApp(transport, noDoc);
    const before = transport.sentOf('brand.get').length;
    transport.emit(event(LOCAL, 1, { kind: 'brand_changed', brand_identifier: 'acme', version: '3', source: 'build' }));
    await settle();
    expect(transport.sentOf('brand.get').length).toBe(before + 1);
  });

  it('Edge case — quitting with agents working: the shell holds the request', async () => {
    const transport = new FakeTransport();
    const app = await startApp(transport, noDoc);
    transport.desktop('quit-requested', { running_sessions: 2 });
    expect(app.shell.quitRequest).toEqual({ running_sessions: 2 });
  });

  // spec: ordin-desktop/components/ScreensModule#SM-QT-1
  it('ordin://quitting closes the quit question and marks the window as closing', async () => {
    const transport = new FakeTransport();
    const app = await startApp(transport, noDoc);
    transport.desktop('quit-requested', { running_sessions: 2 });
    transport.desktop('quitting', { running_sessions: 2 });
    expect(app.shell.quitRequest).toBeNull();
    expect(app.shell.quitting).toEqual({ running_sessions: 2 });
  });

  it('dictation and terminal stream messages are fanned out by stream id', async () => {
    const transport = new FakeTransport();
    const app = await startApp(transport, noDoc);
    const seen: unknown[] = [];
    app.streams.subscribe('s1', (m) => seen.push(m.payload));
    transport.stream({ stream: 's1', kind: 'dictation', payload: { kind: 'listening' } });
    transport.stream({ stream: 's2', kind: 'dictation', payload: { kind: 'other' } });
    expect(seen).toEqual([{ kind: 'listening' }]);
  });

  it('SVC_NOT_IN_THIS_VERSION reloads the backend so the entry disappears (SM-UP-1)', async () => {
    const transport = new FakeTransport();
    const app = await startApp(transport, noDoc);
    expect(answers(app.local, 'mail.list_messages')).toBe(false);
    transport.on('service.hello', ok(hello(LOCAL, ['service', 'mail'])));
    transport.on('mail.list_messages', { kind: 'error', error: { code: 'SVC_NOT_IN_THIS_VERSION', message: '' } });
    const hellos = transport.sentOf('service.hello').length;
    await app.local.handle.call('mail.list_messages', {} as never);
    await settle();
    expect(transport.sentOf('service.hello').length).toBe(hellos + 1);
  });
});

describe('Settings › Display takes effect at once (SM-UP-1)', () => {
  const docWith = () => {
    const dataset: Record<string, string> = {};
    return { dataset, doc: { documentElement: { dataset } } as never };
  };
  // spec: ordin-desktop/components/ScreensModule#SM-UP-1
  it('Happy path — ordin://appearance after a change sets the theme without a restart, and "system" follows the computer again', async () => {
    const transport = new FakeTransport();
    transport.on('settings.get', ok({ section: 'display', settings: [{ key: 'display.appearance', value: 'system', set_by: 'person', person_may_change: true }] }));
    const { dataset, doc } = docWith();
    await startApp(transport, { document: doc, storage: null });
    expect(dataset['theme']).toBeUndefined();
    transport.desktop('appearance', { appearance: 'dark' });
    expect(dataset['theme']).toBe('dark');
    transport.desktop('appearance', { appearance: 'light' });
    expect(dataset['theme']).toBe('light');
    transport.desktop('appearance', { appearance: 'system' });
    expect(dataset['theme']).toBeUndefined();
  });
});
