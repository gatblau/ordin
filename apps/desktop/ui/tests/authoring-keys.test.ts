// SM-CU-2: screen identifiers of the authoring screens, and their registration (SM-UP-1).
import { beforeAll, describe, expect, it } from 'vitest';

import { answersAll } from '../src/lib/backend/groups';
import { getScreen, makeKey, parseKey, registeredScreens, screenKeyFor } from '../src/lib/shell/screens';
import '../src/lib/views/authoring/register';
import { splitDocumentInstance } from '../src/lib/views/authoring/support';
import { R1_GROUPS } from './support/fake-backend';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

describe('SM-CU-2 — screen identifiers', () => {
  it.each([
    ['document', 'place-1:Plans/rates.md'],
    ['workflow', 'wf-1'],
    ['agent-draft', 'd-1'],
    ['repository-parts', 'r-1'],
    ['part-draft', 'p-1'],
    ['withdraw-part', 'part-9'],
  ])('parseKey(makeKey(%s, %s)) returns the pair', (id, instance) => {
    expect(parseKey(makeKey(id, instance))).toEqual({ id, instance });
  });

  it('Happy path — screenKeyFor of a document target returns document:<place id>:<path>', () => {
    const key = screenKeyFor({ kind: 'document', workspace_id: 'w', place_id: 'place-1', path: 'Plans/rates.md' });
    expect(key).toBe('document:place-1:Plans/rates.md');
    expect(splitDocumentInstance(parseKey(key as string).instance)).toEqual({ placeId: 'place-1', path: 'Plans/rates.md' });
  });

  it('a path that holds ":" keeps it (the instance splits at the first colon only)', () => {
    const key = screenKeyFor({ kind: 'document', workspace_id: 'w', place_id: 'p', path: 'a:b.md' }) as string;
    expect(splitDocumentInstance(parseKey(key).instance)).toEqual({ placeId: 'p', path: 'a:b.md' });
  });

  it('every authoring screen is registered, and offered only when the backend answers its commands (SM-UP-1)', () => {
    for (const id of ['document', 'workflow', 'agent-draft', 'repository-parts', 'part-draft', 'withdraw-part']) {
      const def = getScreen(id);
      expect(def, id).toBeDefined();
      expect(registeredScreens().filter((s) => s.id === id)).toHaveLength(1);
    }
    const draft = getScreen('agent-draft');
    expect(answersAll({ groups: new Set(R1_GROUPS) }, draft?.commands ?? [])).toBe(false);
    expect(answersAll({ groups: new Set([...R1_GROUPS, 'authoring']) }, draft?.commands ?? [])).toBe(true);
  });
});
