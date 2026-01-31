// Shared by the team-workspace tests (ScreensModule.md SM-TW-1 to SM-TW-12): a waiting grant, an activity
// entry and a team knowledge status as the backend sends them.
import { R1_GROUPS } from './support/fake-backend';
import { uid } from './support/ids';
import { WS } from './fixed-support';

/** An R1 build that also answers the team knowledge group. */
export const TEAM_GROUPS = [...R1_GROUPS, 'team_knowledge'];

export const grant = (over: Record<string, unknown> = {}) =>
  ({
    grant_id: uid('g1'),
    added_by: 'priya',
    agent_name: 'Supplier analyst',
    new_agent: true,
    access: [{ place_name: 'Contracts', access: 'read_and_change' }],
    left_out: [],
    added_at: '2026-01-20T09:30:02.114Z',
    added_by_confirmed: true,
    ...over,
  }) as never;

export const entry = (over: Record<string, unknown> = {}) =>
  ({
    id: uid('a1'),
    kind: 'run',
    by: 'sam',
    by_confirmed: true,
    mine: false,
    at: '2026-01-20T10:12:40.001Z',
    task: 'Compare documents',
    agent: 'Supplier analyst',
    outcome: 'completed',
    results: [],
    replaced_by_confirmed: false,
    ...over,
  }) as never;

export const teamWs = (over: Record<string, unknown> = {}) => ({
  state: 'team', places: [], agents_not_here: [], results: 'acme/supplier-team › knowledge › results', results_here_only: false,
  pending_grants: 0, unsent_results: 0, activity_waiting: 0, ...over,
});

export const status = (over: Record<string, unknown> = {}) => ({
  workspace_id: WS, folders: [], descriptions: [], folder_large: false, held_for_check: 0, problems: [], workspace_facts: 0, workspace_large: false,
  binding: { folder: 'knowledge', access: 'read_write', source_link_id: uid('sl-own') }, header_line: 'acme/supplier-team › knowledge', ...over,
});

