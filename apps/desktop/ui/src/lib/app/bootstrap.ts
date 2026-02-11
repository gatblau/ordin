// Start (Internal Logic steps 1 and 2): open the event and stream channels before the first data
// command, create the local handle and one handle per server, load the catalogue, the brand and the
// stores, and keep them current. `main.ts` calls this with the Tauri transport; the tests with a fake.
import { Backends, createHandle, type DesktopEventName, type EventEnvelope, type Transport } from '../backend/backend';
import {
  applyBrandFonts,
  applyBrandToDocument,
  applyLocaleToDocument,
  i18n as windowI18n,
  I18n,
  loadBrand,
  loadLocale,
  type DocumentLike,
} from '../i18n/fluent';
import { setFormatLocale } from '../format/intl';
import { applyBrandIcons, installSprite } from '../icons/sprite';
import { BLOCK_SCREEN_ID } from '../blocks/block-screen';
import { blockTitle } from '../blocks/titles';
import { StreamHub, type AppContext } from '../shell/context';
import { ScreenAreaState } from '../shell/screen-area.svelte';
import { makeKey, SCREEN_IDS } from '../shell/screens';
import { ShellState } from '../shell/shell-state.svelte';
import type { ServerView } from '../service-api/types.gen';
import { BackendState, Stores } from '../store/backend-state.svelte';
import { Diagnostics } from '../store/diagnostics.svelte';
import { browserStorage, ViewPrefs, type StorageLike } from '../store/view-prefs';
import { MemoryModelsModel } from '../views/memory/memory-models-model.svelte';
import { FirstStartModel } from '../views/setup/first-start-model.svelte';

/** The block kinds whose row of step 9's block table says "screen at once": `form` and `crew_plan`. */
export function opensScreenAtOnce(block: { kind: string }): boolean {
  return block.kind === 'form' || block.kind === 'crew_plan';
}

export interface StartOptions {
  storage?: StorageLike | null;
  i18n?: I18n;
  /** The document to set `lang`, `dir` and tokens on; defaults to the WebView's. Pass `null` for none. */
  document?: DocumentLike | null;
  timeoutMs?: number;
}

export async function startApp(transport: Transport, options: StartOptions = {}): Promise<AppContext> {
  const diagnostics = new Diagnostics();
  const prefs = new ViewPrefs(options.storage === undefined ? browserStorage() : options.storage, diagnostics);
  // The screens read their text from the window's own `i18n` (`t`), so that is the one the catalogue goes into.
  const i18n = options.i18n ?? windowI18n;
  const doc = options.document === null ? undefined : (options.document ?? (typeof document === 'undefined' ? undefined : (document as unknown as DocumentLike)));
  const stores = new Stores(diagnostics);
  const streams = new StreamHub();
  const shell = new ShellState();
  const desktopListeners = new Map<DesktopEventName, Set<(payload: unknown) => void>>();
  const stopFns: Array<() => void> = [];

  // 1. Channels first, before the first data command; events wait until every store exists.
  let routing = false;
  const early: EventEnvelope[] = [];
  const opened = await transport.openEvents({}, (batch) => {
    for (const env of batch.events) {
      if (routing) void stores.receive(env);
      else early.push(env);
    }
  });
  await transport.openStreams((m) => streams.dispatch(m));

  // 2. Backends.
  const localHandle = createHandle({
    id: opened.local_backend,
    kind: 'local',
    transport,
    diagnostics,
    onNotInThisVersion: () => void local.reload(),
    ...(options.timeoutMs !== undefined ? { timeoutMs: options.timeoutMs } : {}),
  });
  const backends = new Backends(localHandle);
  // The tree keeps each expanded workspace as `<backend id>:<workspace id>` (WorkspaceTree `nodeKey`); this
  // computer's are read back as workspace ids. Passing the keys whole sent `workspaces.tree` an id the core
  // refuses, so after a restart an expanded workspace's tree never loaded (plan 04).
  const localPrefix = `${opened.local_backend}:`;
  const expanded = (): string[] =>
    prefs
      .expanded(shell.openWorkspace === null ? [] : [`${localPrefix}${shell.openWorkspace}`])
      .filter((key) => key.startsWith(localPrefix))
      .map((key) => key.slice(localPrefix.length));
  const local = new BackendState({ handle: localHandle, diagnostics, expandedWorkspaces: expanded });
  stores.add(local);
  shell.openBackend = local.id;

  const screens = new ScreenAreaState(prefs, (scope, key) => {
    // A tab "shows running work" when it is a run's or a crew's progress screen that is not finished.
    const [backendId] = scope.split(':');
    const state = backendId === undefined ? undefined : stores.get(backendId);
    const [id, instance] = key.split(/:(.*)/s);
    if (state === undefined || instance === undefined) return false;
    if (id === 'run') return ['queued', 'running', 'paused'].includes(state.runs.get(instance)?.state ?? '');
    if (id === 'crew') return ['starting', 'running', 'paused'].includes(state.crews.get(instance)?.state ?? '');
    return false;
  });

  // Locale, brand and the appearance come from the local core (step 1).
  const applyLocale = async (active?: Parameters<typeof loadLocale>[2]): Promise<void> => {
    const result = await loadLocale(i18n, localHandle, active);
    if (result !== null) {
      setFormatLocale(result.locale);
      applyLocaleToDocument(result, doc);
    }
    return;
  };
  // The icon sprite goes in before the first screen is drawn (ScreensModule.md SM-IC-1); only a real
  // document holds one.
  const iconDoc = doc !== undefined && typeof document !== 'undefined' ? document : undefined;
  if (iconDoc !== undefined) installSprite(iconDoc);
  const applyBrand = async (): Promise<void> => {
    const brand = await loadBrand(i18n, localHandle);
    if (brand === null) return;
    applyBrandToDocument(brand.design_tokens, doc, diagnostics);
    // The pack's icons, by name (cross-cutting/VisualDesign.md Internal Logic step 8). Not awaited, as
    // for fonts: ordin's own drawings stand until the pack's icon sets arrive. The sprite itself
    // follows each change of the effective theme.
    if (iconDoc !== undefined) void applyBrandIcons(localHandle, { brand, diagnostics, doc: iconDoc });
    // The pack's own font files (cross-cutting/VisualDesign.md P4); ordin's own pack names Jost,
    // which tokens.css already declares, and carries no file. Not awaited: a slow file must not hold
    // the first screen, which is drawn in the next family meanwhile.
    if (doc !== undefined) void applyBrandFonts(brand, localHandle, diagnostics);
  };
  await applyLocale();
  await applyBrand();
  await applyAppearance(localHandle, doc);

  // Servers: one handle and one store per ServerView that has a backend.
  const serverViews = new Map<string, ServerView>();
  const syncServers = async (): Promise<void> => {
    const list = await localHandle.call('servers.list', {});
    if (list.kind !== 'ok') return;
    serverViews.clear();
    const known = new Set<string>();
    for (const view of list.result) {
      serverViews.set(view.server_connection_id, view);
      if (view.backend === undefined || view.backend === null) continue;
      known.add(view.backend);
      let state = stores.get(view.backend);
      if (state === undefined) {
        const handle = createHandle({
          id: view.backend,
          kind: 'server',
          transport,
          diagnostics,
          onNotInThisVersion: () => void state?.reload(),
          ...(options.timeoutMs !== undefined ? { timeoutMs: options.timeoutMs } : {}),
        });
        state = new BackendState({ handle, diagnostics, expandedWorkspaces: () => [] });
        stores.add(state);
      }
      const wasConnected = state.connection === 'connected';
      state.setConnection(view.state, view.as_of, { displayName: view.display_name, olderSide: view.older_side ?? null });
      if (view.state === 'connected' && (!state.loaded || !wasConnected) && !state.loading) void state.reload();
    }
    for (const state of stores.all()) {
      if (state.kind === 'server' && !known.has(state.id)) stores.remove(state.id);
    }
    backends.setServers(stores.all().filter((s) => s.kind === 'server').map((s) => s.handle));
  };
  await syncServers();

  // The local store's events that the window itself acts on.
  stopFns.push(
    local.subscribe((env) => {
      switch (env.kind) {
        case 'locale_changed':
          void applyLocale(env.active);
          break;
        case 'language_changed':
          // The edited language may be in use: re-read the catalogue of the active locale.
          if (i18n.locale === env.locale) void applyLocale();
          break;
        case 'brand_changed':
          void applyBrand();
          break;
        case 'server_state_changed': {
          const view = serverViews.get(env.server_connection_id);
          if (view === undefined) {
            void syncServers();
            break;
          }
          const backend = view.backend === undefined || view.backend === null ? undefined : stores.get(view.backend);
          if (backend !== undefined) {
            const wasConnected = backend.connection === 'connected';
            backend.setConnection(env.state, env.as_of, { olderSide: env.state === 'refused_for_version' ? (view.older_side ?? null) : null });
            if (env.state === 'connected' && !wasConnected) void backend.reload();
            if (env.state === 'refused_for_version') void syncServers();
          }
          view.state = env.state;
          view.as_of = env.as_of ?? null;
          break;
        }
        default:
          break;
      }
    }),
  );
  // A new approval arriving on any backend announces itself (polite live region).
  for (const state of stores.all()) {
    stopFns.push(state.subscribe((env) => {
      if (env.kind === 'block_shown') {
        // ScreensModule.md, Internal Logic step 6: a form and a crew plan appear on a screen at once. The
        // tab is marked new, never in front, and the person's place does not move (SM-WR-8, SM-AP-1's rule).
        // spec: ordin-desktop/components/ScreensModule#SM-WR-8
        if (opensScreenAtOnce(env.block)) {
          screens.open(ScreenAreaState.scope(state.id, env.workspace_id), makeKey(BLOCK_SCREEN_ID, env.block_id), { inFront: false, title: blockTitle(env.block) });
        }
        shell.noteNewInConversation();
      }
      // ScreensModule.md, Internal Logic step 7: the card opens in its workspace's screen area
      // as a tab marked new; the person's place does not move.
      if (env.kind === 'approval_requested') {
        screens.open(ScreenAreaState.scope(state.id, env.card.workspace_id), makeKey(SCREEN_IDS.approval, env.card.approval_id), { inFront: false });
        shell.noteNewInConversation();
      }
    }));
  }

  // 3. Load the stores, then release the events that arrived meanwhile.
  await local.load();
  for (const state of stores.all()) if (state.kind === 'server' && state.connection === 'connected' && !state.loaded && !state.loading) void state.reload();
  routing = true;
  for (const env of early) void stores.receive(env);
  early.length = 0;

  // Desktop-only events.
  const dispatch = (name: DesktopEventName) => (payload: unknown) => {
    if (name === 'quit-requested') {
      const count = (payload as { running_sessions?: unknown } | null)?.running_sessions;
      shell.quitRequest = { running_sessions: typeof count === 'number' ? count : 0 };
    }
    // SM-QT-1: the shutdown has started; the quit question, if open, has been answered.
    // spec: ordin-desktop/components/ScreensModule#SM-QT-1
    if (name === 'quitting') {
      const count = (payload as { running_sessions?: unknown } | null)?.running_sessions;
      shell.quitRequest = null;
      shell.quitting = { running_sessions: typeof count === 'number' ? count : 0 };
    }
    // `ordin://appearance` after `settings.change` of `display.appearance` (OrdinDesktop.md): the chosen
    // appearance takes effect at once, as it does at start.
    // spec: ordin-desktop/components/ScreensModule#SM-UP-1
    if (name === 'appearance') setTheme(doc, (payload as { appearance?: unknown } | null)?.appearance);
    for (const l of desktopListeners.get(name) ?? []) l(payload);
  };
  for (const name of ['quit-requested', 'quitting', 'open-target', 'appearance'] as const) {
    stopFns.push(await transport.listenDesktop(name, dispatch(name)));
  }

  // First start (ScreensModule.md SM-FS-1): after the catalogue is loaded and before the three regions
  // are drawn, ask the local backend whether the guided setup must run; `App` draws only `FirstStart`
  // while it does.
  // spec: ordin-desktop/components/ScreensModule#SM-FS-1
  const firstStart = new FirstStartModel(localHandle);
  stopFns.push(local.subscribe((env) => firstStart.receive(env)));
  await firstStart.checkStatus();

  // Getting memory ready (ScreensModule.md SM-MD-1): the local backend's models at start, then each
  // `memory_models_changed` of the local backend only.
  // spec: ordin-desktop/components/ScreensModule#SM-MD-1
  const memoryModels = new MemoryModelsModel(localHandle);
  stopFns.push(local.subscribe((env) => memoryModels.receive(env)));
  await memoryModels.load();

  return {
    transport,
    backends,
    stores,
    local,
    diagnostics,
    i18n,
    prefs,
    screens,
    shell,
    streams,
    firstStart,
    memoryModels,
    stateFor: (id) => stores.get(id),
    onDesktop(name, listener) {
      let set = desktopListeners.get(name);
      if (set === undefined) {
        set = new Set();
        desktopListeners.set(name, set);
      }
      set.add(listener);
      return () => set.delete(listener);
    },
    dispose() {
      for (const stop of stopFns) stop();
      stopFns.length = 0;
    },
  };
}

/** `display.appearance`: light or dark when the person chose one, else the operating system's (chosen here: the setting's key and values are read leniently). */
async function applyAppearance(local: ReturnType<typeof createHandle>, doc: DocumentLike | undefined): Promise<void> {
  const out = await local.call('settings.get', { section: 'display' });
  if (out.kind !== 'ok' || doc === undefined) return;
  const setting = out.result.settings.find((s) => s.key === 'appearance' || s.key === 'display.appearance');
  setTheme(doc, setting?.value);
}

/** `light` or `dark` sets `data-theme`; anything else (`system`) follows the operating system's. */
function setTheme(doc: DocumentLike | undefined, value: unknown): void {
  if (doc === undefined) return;
  const root = doc.documentElement as unknown as { dataset?: Record<string, string> };
  if (root.dataset === undefined) return;
  if (value === 'light' || value === 'dark') root.dataset['theme'] = value;
  else delete root.dataset['theme'];
}
