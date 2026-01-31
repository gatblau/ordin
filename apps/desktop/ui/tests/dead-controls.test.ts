// @vitest-environment happy-dom
// Plan 04, safety net B: no control on any screen does nothing.
//
// The window is started the way main.ts starts it (startApp, then App mounted, so every screen has the real
// app context) over a backend that answers every command with a sample built from its result schema
// (tests/support/sample-backend.ts). Each registered screen is opened, with no instance and with a sample
// one, and every control a person can operate in the screen area is clicked on a freshly opened copy of the
// screen. A click must change something: send a command, open or bring forward a screen, or change what is
// drawn (a dialog, a menu, a folder's rows). One that changes nothing is a dead control: in test.1 a PDF row
// in Documents was one.
//
// The requests this test sends are also checked by `make ui-contract-check` (safety net A).
import { mount, unmount } from 'svelte';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import App from '../src/App.svelte';
import { startApp } from '../src/lib/app/bootstrap';
import type { AppContext } from '../src/lib/shell/context';
import '../src/lib/shell/load-registrations';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import { makeKey, registeredScreens, SCREEN_IDS } from '../src/lib/shell/screens';
import { LOCAL, settle } from './support/fake-backend';
import inertList from './support/inert-on-purpose.txt?raw';
import tokens from '../src/styles/tokens.css?raw';
import { sampleId, SampleTransport } from './support/sample-backend';
import { SECTIONS } from '../src/lib/views/Settings/sections';

/**
 * How the program opens each screen (ScreensModule.md, step 9's screen table): with no instance only, both ways,
 * or with an instance of a given form. A screen not named here is opened with an id, the commonest form.
 */
const NO_INSTANCE = new Set<string>([
  SCREEN_IDS.home, SCREEN_IDS.settings, SCREEN_IDS.diagnostics, SCREEN_IDS.earlierConversations, SCREEN_IDS.agents,
  SCREEN_IDS.workflows, SCREEN_IDS.dashboards, SCREEN_IDS.telegram, SCREEN_IDS.slack, SCREEN_IDS.memory, SCREEN_IDS.running,
  'add-to-workspace', 'import', 'archived', 'bin',
]);
const BOTH = new Set<string>([SCREEN_IDS.documents, SCREEN_IDS.knowledge, 'lifecycle']);
const INSTANCE: Record<string, string> = {
  [SCREEN_IDS.document]: `${sampleId(900)}:Q3/Quarterly plan.pdf`,
  'code-editor': `${sampleId(902)}:src/main.rs`,
  language: 'en-GB',
  'server-languages': LOCAL,
};
const instancesOf = (id: string): (string | null)[] => {
  const one = INSTANCE[id] ?? sampleId(901);
  if (NO_INSTANCE.has(id)) return [null];
  if (BOTH.has(id)) return [null, one];
  return [one];
};

const OPERABLE = [
  'a[href]',
  'button',
  'summary',
  'input[type=checkbox]',
  'input[type=radio]',
  '[role=button]',
  '[role=link]',
  '[role=tab]',
  '[role=menuitem]',
  '[role=treeitem]',
  '[role=option]',
  '[role=switch]',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

/** At most this many controls per screen are clicked, so one long list does not dominate the run. */
const PER_SCREEN = 40;

/** `<screen id> <selector> <reason>` lines: controls that do nothing on purpose. */
const INERT = inertList
  .split('\n')
  .map((l) => l.trim())
  .filter((l) => l !== '' && !l.startsWith('#'))
  .map((l) => {
    const [screen, selector, ...reason] = l.split(/\s+/);
    if (screen === undefined || selector === undefined || reason.length === 0) throw new Error(`inert-on-purpose.txt: ${l}`);
    return { screen, selector };
  });

const name = (el: Element): string => {
  const label = el.getAttribute('aria-label') ?? el.textContent?.trim().replace(/\s+/g, ' ') ?? '';
  return `<${el.tagName.toLowerCase()}${el.getAttribute('role') !== null ? ` role=${el.getAttribute('role')}` : ''}> "${label.slice(0, 50)}"`;
};

const disabled = (el: Element): boolean =>
  (el as HTMLButtonElement).disabled === true || el.getAttribute('aria-disabled') === 'true' || el.closest('[inert]') !== null;

let transport: SampleTransport;
let app: AppContext;
let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;
const errors: string[] = [];

beforeAll(async () => {
  window.addEventListener('error', (e) => errors.push(String(e.error ?? e.message)));
  window.addEventListener('unhandledrejection', (e) => errors.push(String(e.reason)));
  document.addEventListener(
    'click',
    (e) => {
      if (e.target instanceof HTMLInputElement && e.target.type === 'file') filePickers++;
    },
    true,
  );
  // The window's own design tokens, so a screen that reads them (the code editor's colours) reads what the
  // program reads.
  const style = document.createElement('style');
  style.textContent = tokens;
  document.head.appendChild(style);
  transport = new SampleTransport();
  app = await startApp(transport, { storage: null });
  target = document.createElement('div');
  document.body.appendChild(target);
  component = mount(App, { target, props: { app } });
  await settle(40);
});

afterAll(() => {
  if (component !== null) unmount(component);
});

const scope = (): string => ScreenAreaState.scope(LOCAL, transport.workspaceId);
// The screen's own content; the tab bar above it is the shell's and is the same on every screen.
const area = (): Element | null => target.querySelector('#screen-panel');
/** Clicks that reached a file input: a control that opens the system's file picker does it this way, and the
 * picker itself is not drawn in the page. */
let filePickers = 0;
const noWorkspace = (): string => ScreenAreaState.scope(LOCAL, null);
const snapshot = (): string =>
  JSON.stringify({
    sent: transport.sent.length,
    tabs: app.screens.tabs(scope()).map((t) => t.key),
    front: app.screens.front(scope()),
    backend: app.shell.openBackend,
    workspace: app.shell.openWorkspace,
    filePickers,
    body: document.body.innerHTML,
  });

/** The control that shows where the person already is, or the choice already made (a ticked radio button):
 * clicking it rightly changes nothing. */
const current = (el: Element): boolean =>
  (el instanceof HTMLInputElement && el.type === 'radio' && el.checked) ||
  el.getAttribute('aria-current') !== null ||
  el.getAttribute('aria-selected') === 'true' ||
  el.closest('[aria-selected=true]') !== null;

/** Opens `key` afresh in the sample workspace: closed first, so its state is new. */
async function openFresh(key: string): Promise<Element[]> {
  for (const tab of app.screens.tabs(scope())) app.screens.close(scope(), tab.key);
  for (const tab of app.screens.tabs(noWorkspace())) app.screens.close(noWorkspace(), tab.key);
  await settle(5);
  app.shell.openWorkspaceTab(LOCAL, transport.workspaceId);
  app.screens.open(scope(), key);
  await settle(20);
  const region = area();
  return region === null ? [] : Array.from(region.querySelectorAll(OPERABLE));
}

/** Clicks every control of `key` (only those inside `within` when given) (a screen of `screenId`), each on a freshly opened copy; adds the dead ones to `dead`. */
async function sweepKey(screenId: string, key: string, dead: string[], within?: string): Promise<{ clicked: number; skipped: number }> {
  let clicked = 0;
  let skipped = 0;
  const open = async (): Promise<Element[]> => (await openFresh(key)).filter((c) => within === undefined || c.closest(within) !== null);
  let controls = await open();
  const count = Math.min(controls.length, PER_SCREEN);
  if (process.env.ORDIN_SWEEP_TRACE) console.log(`sweep ${key}: ${count} controls at ${Math.round(performance.now())}ms`);
  // The screen is opened afresh only after a click changed something, so the next control is clicked on
  // a screen in its first state.
  let dirty = false;
  for (let i = 0; i < count; i++) {
    if (dirty) {
      controls = await open();
      dirty = false;
    }
    const control = controls[i];
    if (control === undefined || !control.isConnected) continue;
    if (disabled(control) || current(control) || INERT.some((x) => x.screen === screenId && control.matches(x.selector))) {
      skipped++;
      continue;
    }
    const before = snapshot();
    const errorsBefore = errors.length;
    if (process.env.ORDIN_SWEEP_TRACE) console.log(`  click ${i} ${name(control)} at ${Math.round(performance.now())}ms`);
    (control as HTMLElement).click();
    await settle(15);
    clicked++;
    const after = snapshot();
    dirty = after !== before;
    if (errors.length > errorsBefore) {
      dead.push(`${key}: ${name(control)} threw ${errors.slice(errorsBefore).join('; ')}`);
      dirty = true;
    } else if (!dirty) dead.push(`${key}: ${name(control)} does nothing`);
  }
  return { clicked, skipped };
}

describe('dead controls', () => {
  it('every control on every registered screen does something when clicked', { timeout: 600_000 }, async () => {
    const screens = registeredScreens();
    expect(screens.length).toBeGreaterThan(20);
    const dead: string[] = [];
    let clicked = 0;
    let skipped = 0;
    for (const screen of screens) {
      for (const instance of instancesOf(screen.id)) {
        const n = await sweepKey(screen.id, makeKey(screen.id, instance), dead);
        clicked += n.clicked;
        skipped += n.skipped;
      }
    }
    console.log(`dead-controls: ${screens.length} screens, ${clicked} controls clicked, ${skipped} disabled, current or inert on purpose`);
    expect(dead).toEqual([]);
  });

  // Settings opens on its first section, so the sweep above reaches only that one (and the section list).
  // Each section is opened here on its own, by the instance Settings takes as the section to open first.
  it('every control of every Settings section does something when clicked', { timeout: 600_000 }, async () => {
    const dead: string[] = [];
    const empty: string[] = [];
    let clicked = 0;
    for (const section of SECTIONS) {
      const key = makeKey(SCREEN_IDS.settings, section.id);
      await openFresh(key);
      // The open section is drawn: something follows the section list.
      const body = area()?.querySelector('.section-body');
      if (body === null || body === undefined || body.children.length === 0 || (body.textContent ?? '').trim() === '') empty.push(section.id);
      clicked += (await sweepKey(SCREEN_IDS.settings, key, dead, '.section-body')).clicked;
    }
    console.log(`dead-controls: ${SECTIONS.length} Settings sections, ${clicked} controls clicked`);
    expect(empty).toEqual([]);
    expect(dead).toEqual([]);
  });
});
