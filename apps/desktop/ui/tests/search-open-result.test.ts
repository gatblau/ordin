// Choosing a *Search or go to…* result opens what it names, the way the side tree opens it (DSK-REC-20).
import { beforeAll, describe, expect, it } from 'vitest';

import { openTarget } from '../src/lib/shell/open-item';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import { screenKeyFor } from '../src/lib/shell/screens';
import type { OpenTarget } from '../src/lib/service-api/types.gen';
import { appWith, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';

describe('a search result opens its screen', () => {
  it.each<[string, OpenTarget, string | null]>([
    ['an agent', { kind: 'item', item_kind: 'agent', id: 'agent-1' }, 'agents:agent-1'],
    ['a workflow', { kind: 'item', item_kind: 'workflow', id: 'wf-1', workspace_id: WS }, 'workflow:wf-1'],
    ['a conversation', { kind: 'item', item_kind: 'conversation', id: 'c-1', workspace_id: WS }, 'earlier-conversation:c-1'],
    ['a knowledge page', { kind: 'knowledge_page', page_id: 'p-1', workspace_id: WS }, 'knowledge:p-1'],
    ['a document', { kind: 'document', path: 'a.md', place_id: 'pl', workspace_id: WS }, 'document:pl:a.md'],
    ['an item kind with no screen', { kind: 'item', item_kind: 'unknown', id: 'x' }, null],
  ])('%s', (_name, target, key) => {
    expect(screenKeyFor(target)).toBe(key);
  });

  it('clicking the agent result opens that agent’s page in front, titled with its name', async () => {
    const { app } = await appWith();
    const local = app.local.id;
    const key = openTarget(app, local, null, { kind: 'item', item_kind: 'agent', id: 'agent-1' }, 'Software Engineer');
    expect(key).toBe('agents:agent-1');
    const scope = ScreenAreaState.scope(local, app.shell.openWorkspace);
    expect(app.screens.front(scope)).toBe('agents:agent-1');
    expect(app.screens.tabs(scope).find((t) => t.key === 'agents:agent-1')?.title).toBe('Software Engineer');
  });
});
