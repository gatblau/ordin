import { describe, expect, it } from 'vitest';

import { answers, answersAll, groupOf } from '../src/lib/backend/groups';

const set = (...groups: string[]) => ({ groups: new Set(groups) });

describe('answers (SM-UP-1)', () => {
  it.each([
    [['mail', 'service'], 'mail.list_messages', true],
    [['service'], 'mail.list_messages', false],
    [['mail'], 'slack.link', false],
    [['mail'], 'nodot', false],
    [['mail'], '.leading', false],
    [[], 'service.hello', true],
    [[], 'home.view', false],
    [['home'], 'home.view', true],
  ])('groups %j, command %s -> %s', (groups, command, expected) => {
    expect(answers(set(...groups), command)).toBe(expected);
  });

  it('an empty set hides every entry but those of service', () => {
    for (const c of ['files.preview', 'ordin.send_message', 'search.query']) expect(answers(set(), c)).toBe(false);
    expect(answers(set(), 'service.hello')).toBe(true);
  });

  it('an entry that sends several commands needs every group', () => {
    expect(answersAll(set('files', 'identity'), ['files.add_place', 'identity.list'])).toBe(true);
    expect(answersAll(set('files'), ['files.add_place', 'identity.list'])).toBe(false);
  });

  it('groupOf is the part before the first dot', () => {
    expect(groupOf('workspaces.tree')).toBe('workspaces');
    expect(groupOf('nodot')).toBeNull();
  });
});
