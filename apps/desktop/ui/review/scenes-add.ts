// *Add to this workspace* and removing a link, for the side-by-side review (cross-cutting/VisualDesign.md,
// Internal Logic step 7): scenes `newWs`, `addCode` and `jiraRemove` with the prototype's sample names.
// Sample data is illustrative (12 §2). The person's presses are made on the drawn page, as they would be.
import type { AppContext } from '../src/lib/shell/context';
import { openScreen } from '../src/lib/shell/open-item';
import { ADD_TO_WORKSPACE_SCREEN } from '../src/lib/views/add/add-model.svelte';
import { err, hello, LOCAL, R1_GROUPS, type FakeTransport, type ok as okFn } from '../tests/support/fake-backend';
import { conversation, WS, type Seed } from './scenes';

type Ok = typeof okFn;
const later = (fn: () => void, ms = 80): void => void setTimeout(fn, ms);
const GROUPS = [...R1_GROUPS, 'sharepoint', 'confluence', 'jira', 'mail'];

const link = (id: string, kind: string, display_name: string, over: Record<string, unknown> = {}) => ({
  id: `0192f4a0-0000-7000-8000-00000000000${id}`, kind, display_name, state: 'active', workspace_id: WS.q3,
  added_at: '2026-01-06T09:00:00.000Z', origin_location: display_name, place_id: `place-${id}`, ...over,
});
const LINKS = [
  link('1', 'sharepoint_location', 'Procurement › Contracts 2026'),
  link('2', 'confluence_location', 'Supplier policy'),
  link('3', 'local_folder', 'Supplier quotes'),
];
const JIRA = link('4', 'jira_issue', 'ABC-12 · Kestrel supplier onboarding', { place_id: null });

const base = (links: unknown[]) =>
  (t: FakeTransport, local: string, ok: Ok): void => {
    conversation(t, local, ok);
    t.on('service.hello', ok(hello(LOCAL, GROUPS)));
    t.on('workspaces.links', ok(links));
    t.on('workspaces.link_progress', ok({ source_link_id: 'x', done: true, listed: 0, read: 0, not_read: 0 }));
    t.on('workspaces.working_copy_items', ok([]));
    t.on('files.list_folder', ok({ items: [], next_cursor: null }));
  };

const click = (selector: string, nth = 0): void => document.querySelectorAll<HTMLElement>(selector)[nth]?.click();
/** Opens Q3 supplier review's Documents row in the tree, then *Add to this workspace*. */
const openAdd = (app: AppContext, local: string): void => {
  click('.workspace-tree .tree-group .tree-line > .tree-row');
  later(() => openScreen(app, local as never, WS.q3, ADD_TO_WORKSPACE_SCREEN));
};

/** `newWs`: *Add to this workspace* at SharePoint; the search needs a Microsoft sign-in first. */
const newWs: Seed = Object.assign(
  (t: FakeTransport, local: string, ok: Ok) => {
    base([])(t, local, ok);
    t.on('sharepoint.find', err('SP_SIGN_IN_NEEDED', 'Sign in to SharePoint so Ordin can read this location.', { action: 'sign_in_again' }));
  },
  {
    after: (app: AppContext, t: FakeTransport, local: string) => {
      conversation.after?.(app, t, local);
      later(() => {
        openScreen(app, local as never, WS.q3, ADD_TO_WORKSPACE_SCREEN);
        later(() => {
          click('.add-entry');
          later(() => {
            const input = document.querySelector<HTMLInputElement>('.add-search input');
            if (input === null) return;
            input.value = 'Contracts';
            input.dispatchEvent(new Event('input', { bubbles: true }));
          });
        });
      });
    },
  },
);

/** `addCode`: *Where it lives*, with SharePoint, Confluence and This computer already linked. */
const addCode: Seed = Object.assign(base(LINKS), {
  after: (app: AppContext, t: FakeTransport, local: string) => {
    conversation.after?.(app, t, local);
    later(() => openAdd(app, local), 120);
  },
});

/** `jiraRemove`: the confirmation open on the Jira link's row. */
const jiraRemove: Seed = Object.assign(base([...LINKS, JIRA]), {
  after: (app: AppContext, t: FakeTransport, local: string) => {
    conversation.after?.(app, t, local);
    later(() => {
      click('.workspace-tree .tree-group .tree-line > .tree-row');
      later(() => {
        const rows = document.querySelectorAll<HTMLElement>('.tree-link');
        const jira = [...rows].findIndex((r) => r.textContent?.includes('ABC-12'));
        click('.tree-group .tree-group .tree-action', jira);
        later(() => click('.tree-menu-list [role="menuitem"]'));
      }, 200);
    }, 120);
  },
});

export const SCENES: Record<string, Seed> = { newWs, addCode, jiraRemove };
