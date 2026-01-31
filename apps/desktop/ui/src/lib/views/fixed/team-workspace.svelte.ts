// A team workspace (ScreensModule.md SM-TW-1 to SM-TW-12, ADR-121): the sentences the share and open
// dialogs, the access card, the panel and Team activity draw, and the state of the waiting grants and of
// the activity list. Three rules on screen: no Git word; the sentence, never the code; what is shared is
// said before it is shared. A name the repository's history does not record is never drawn as fact:
// `team-ws-someone` stands in its place and `team-ws-says-it-was` follows the line.
//
// spec: ordin-desktop/components/ScreensModule#SM-TW-6
// spec: ordin-desktop/components/ScreensModule#SM-TW-8
// spec: ordin-desktop/components/ScreensModule#SM-TW-12
import { answers } from '../../backend/groups';
import { formatList } from '../../format/intl';
import { t } from '../../i18n/fluent';
import type {
  ServiceError,
  TeamAccessLine,
  TeamActivityView,
  TeamAgentFound,
  TeamPendingGrantView,
  TeamPlaceState,
  TeamProblem,
  TeamReach,
  TeamRunOutcome,
  TeamWorkspaceOpenPreview,
  TeamWorkspaceOpened,
} from '../../service-api/types.gen';
import type { BackendState } from '../../store/backend-state.svelte';
import type { ParsedFolder } from './team.svelte';
import { Announcer, type Handle } from './util';

/**
 * SM-TW-12: the ten team-workspace commands are local only, so nothing of SM-TW-1 to SM-TW-11 is drawn
 * for a workspace of a connected server, even when that server answers the team knowledge group.
 */
export function offersTeamWorkspace(backend: Pick<BackendState, 'kind' | 'groups'>, command: string): boolean {
  return backend.kind === 'local' && answers(backend, command);
}

/**
 * The **access phrase** (SM-TW-6): the entries grouped by access, `team-ws-access-change` with the place
 * names of `read_and_change` entries and `team-ws-access-read` with those of `read` entries, joined by
 * `team-ws-access-and`; place names joined by the locale's list format.
 */
export function accessPhrase(lines: readonly TeamAccessLine[]): string {
  const change = lines.filter((l) => l.access === 'read_and_change').map((l) => l.place_name);
  const read = lines.filter((l) => l.access === 'read').map((l) => l.place_name);
  const parts: string[] = [];
  if (change.length > 0) parts.push(t('team-ws-access-change', { places: formatList(change) }));
  if (read.length > 0) parts.push(t('team-ws-access-read', { places: formatList(read) }));
  if (parts.length === 2) return t('team-ws-access-and', { first: parts[0] as string, second: parts[1] as string });
  return parts[0] ?? '';
}

/** A colleague's name as a line draws it, and the `team-ws-says-it-was` note that follows the line when the history does not record it. */
export interface Named {
  person: string;
  says: string | null;
}

/** `login` as fact when the history records it; otherwise `team-ws-someone`, with the login in the note. */
export function named(login: string | null | undefined, confirmed: boolean): Named {
  if (login === null || login === undefined || login === '') return { person: t('team-ws-someone'), says: null };
  if (confirmed) return { person: login, says: null };
  return { person: t('team-ws-someone'), says: t('team-ws-says-it-was', { person: login }) };
}

/** A line with its notes after it, as one text (the notes in order). */
export function withNotes(line: string, notes: readonly (string | null)[]): string {
  return [line, ...notes.filter((n): n is string => n !== null)].join(' ');
}

/** The access card's sentence (SM-TW-6), with `team-ws-says-it-was` after it when `added_by_confirmed` is false. */
export function grantLine(grant: TeamPendingGrantView): string {
  const who = named(grant.added_by, grant.added_by_confirmed);
  const line = t(grant.new_agent ? 'team-ws-grant-new-agent' : 'team-ws-grant-more-access', { person: who.person, agent: grant.agent_name, access: accessPhrase(grant.access) });
  return withNotes(line, [who.says]);
}

/** One entry of Team activity as its line (SM-TW-8): `run` or `replaced`, each unconfirmed name followed by its own note. */
export function activityLine(entry: TeamActivityView): string {
  if (entry.kind === 'replaced') {
    const by = named(entry.by, entry.by_confirmed);
    const other = named(entry.replaced_by, entry.replaced_by_confirmed);
    return withNotes(t('team-ws-activity-replaced', { person: by.person, other: other.person, subject: entry.subject ?? '' }), [by.says, other.says]);
  }
  // `by` absent: this computer's entry, not yet shared ("You"); otherwise the login, confirmed or not.
  const who: Named = (entry.by === null || entry.by === undefined) && entry.mine ? { person: t('team-ws-activity-you'), says: null } : named(entry.by, entry.by_confirmed);
  const task = entry.task ?? t('team-ws-activity-untitled');
  // SPEC-GAP-21: SM-TW-8 draws `agent` in every `run` line but TeamActivityView.agent is optional; an absent agent is drawn as empty text.
  return withNotes(t('team-ws-activity-run', { person: who.person, task, agent: entry.agent ?? '' }), [who.says]);
}

/** An entry of this computer not yet shared carries `team-ws-activity-not-shared`. */
export function notSharedYet(entry: TeamActivityView): boolean {
  return entry.mine && (entry.by === null || entry.by === undefined);
}

export const OUTCOME_MESSAGE: Readonly<Record<TeamRunOutcome, string>> = {
  completed: 'team-ws-outcome-completed',
  failed: 'team-ws-outcome-failed',
  stopped: 'team-ws-outcome-stopped',
};

/** A place's reach in the open preview (SM-TW-3); `will_link` says nothing. */
export const REACH_MESSAGE: Readonly<Record<TeamReach, string | null>> = {
  will_link: null,
  no_access: 'team-ws-reach-no-access',
  sign_in_needed: 'team-ws-reach-sign-in',
  not_set_up: 'team-ws-reach-not-set-up',
  cannot_tell: 'team-ws-reach-cannot-tell',
};

/** An agent found or not in the open preview (SM-TW-3); one found in the library says nothing. */
export const FOUND_MESSAGE: Readonly<Record<TeamAgentFound, string | null>> = {
  in_your_library: null,
  already_here: 'team-ws-agent-already-here',
  not_in_your_repositories: 'team-ws-agent-not-found',
};

/** A place's state in the panel (SM-TW-5); a linked place draws no line. */
export const PLACE_STATE_MESSAGE: Readonly<Record<TeamPlaceState, string | null>> = {
  linked: null,
  no_access: 'team-ws-place-no-access',
  waiting: 'team-ws-place-waiting',
  removal_waits: 'team-ws-place-removal-waits',
};

/**
 * SM-TW-11: the sentence for a team-workspace problem of the **Not taken in** list, or `null` for any
 * other code (which keeps the Team knowledge amendment's rule). The problem's `path` is the file it names.
 */
export function teamWorkspaceProblem(problem: Pick<TeamProblem, 'code' | 'path'>): string | null {
  switch (problem.code) {
    case 'TEAM_WS_RESULTS_NO_ACCESS':
      return t('team-ws-problem-results-no-access');
    case 'TEAM_WS_RESULT_TOO_LARGE':
      return t('team-ws-problem-result-too-large', { file: problem.path });
    case 'TEAM_WS_RESULT_KEPT':
      return t('team-ws-problem-result-kept', { file: problem.path });
    case 'TEAM_WS_RESULT_NOT_TAKEN':
      return t('team-ws-problem-result-not-taken', { file: problem.path });
    case 'TEAM_WS_DESCRIPTION_UNREADABLE':
      return t('team-ws-problem-description');
    case 'TEAM_WS_FORMAT_NEWER':
      return t('team-ws-problem-format-newer');
    case 'TEAM_WS_NAME_IN_USE':
      return t('team-ws-problem-name-in-use');
    default:
      return null;
  }
}

export type Outcome<T> = { ok: true; value: T } | { ok: false; error: ServiceError };

/** **Continue** of *Open a team workspace…* (SM-TW-3): reads the description with the person's own sign-in; stores nothing. */
export async function openPreview(handle: Handle, folder: ParsedFolder): Promise<Outcome<TeamWorkspaceOpenPreview>> {
  const out = await handle.call('team_knowledge.workspace_open_preview', { url: folder.url, branch: folder.branch, path: folder.path });
  return out.kind === 'ok' ? { ok: true, value: out.result } : { ok: false, error: out.error };
}

/** **Open** (SM-TW-3): creates the workspace as previewed, under `name`. */
export async function openWorkspace(handle: Handle, folder: ParsedFolder, name: string): Promise<Outcome<TeamWorkspaceOpened>> {
  const out = await handle.call('team_knowledge.open_workspace', { url: folder.url, branch: folder.branch, path: folder.path, name });
  return out.kind === 'ok' ? { ok: true, value: out.result } : { ok: false, error: out.error };
}

/** `name` of the open preview: 1–80 characters after trimming (SM-TW-3, ServiceApi.md SVC-TW-1). */
export function nameAllowed(name: string): boolean {
  const n = [...name.trim()].length;
  return n >= 1 && n <= 80;
}

/**
 * The waiting grants of one workspace (SM-TW-6): read with `team_knowledge.pending_grants`, answered only
 * by the person's **Allow** or **Don't allow**. A card closes on the reply and the outcome is announced
 * politely; a refusal stays in the card. After `TEAM_WS_GRANT_NOT_FOUND` the list is read again.
 */
export class TeamGrants {
  list = $state.raw<TeamPendingGrantView[]>([]);
  loaded = $state(false);
  /** A refusal per card, by grant id. */
  errors = $state.raw<Record<string, ServiceError>>({});
  /**
   * The refusal of a card that the list no longer holds once read again (`TEAM_WS_GRANT_NOT_FOUND`): drawn
   * where the cards are, so the sentence is not lost with its card (chosen here).
   */
  goneError = $state.raw<ServiceError | null>(null);
  readonly announcer = new Announcer();
  readonly #handle: Handle;
  readonly workspaceId: string;

  constructor(handle: Handle, workspaceId: string) {
    this.#handle = handle;
    this.workspaceId = workspaceId;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('team_knowledge.pending_grants', { ws: this.workspaceId });
    if (out.kind === 'ok') {
      this.list = out.result;
      const held = new Set(out.result.map((g) => g.grant_id));
      this.errors = Object.fromEntries(Object.entries(this.errors).filter(([id]) => held.has(id)));
    }
    this.loaded = true;
  }

  /** **Allow** (`allow` true) or **Don't allow**. Returns true when the card closed. */
  async decide(grant: TeamPendingGrantView, allow: boolean): Promise<boolean> {
    const input = { ws: this.workspaceId, grant_id: grant.grant_id };
    const out = await (allow ? this.#handle.call('team_knowledge.allow_grant', input) : this.#handle.call('team_knowledge.decline_grant', input));
    if (out.kind === 'error') {
      if (out.error.code === 'TEAM_WS_GRANT_NOT_FOUND') {
        this.goneError = out.error;
        await this.load();
      } else {
        this.errors = { ...this.errors, [grant.grant_id]: out.error };
      }
      return false;
    }
    this.goneError = null;
    const { [grant.grant_id]: _closed, ...rest } = this.errors;
    void _closed;
    this.errors = rest;
    this.list = this.list.filter((g) => g.grant_id !== grant.grant_id);
    this.announcer.say(allow ? t('team-ws-grant-allowed', { agent: grant.agent_name }) : t('team-ws-grant-declined'));
    return true;
  }
}

/** The entries of Team activity (SM-TW-8): newest first, 50 a page; **Show earlier** reads the next page with `cursor`. */
export class TeamActivityList {
  entries = $state.raw<TeamActivityView[]>([]);
  next = $state<string | null>(null);
  loaded = $state(false);
  error = $state.raw<ServiceError | null>(null);
  readonly #handle: Handle;
  readonly workspaceId: string;

  constructor(handle: Handle, workspaceId: string) {
    this.#handle = handle;
    this.workspaceId = workspaceId;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('team_knowledge.activity', { ws: this.workspaceId });
    this.loaded = true;
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    this.error = null;
    this.entries = out.result.items;
    this.next = out.result.next_cursor ?? null;
  }

  async more(): Promise<void> {
    if (this.next === null) return;
    const out = await this.#handle.call('team_knowledge.activity', { ws: this.workspaceId, cursor: this.next });
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    this.error = null;
    this.entries = [...this.entries, ...out.result.items];
    this.next = out.result.next_cursor ?? null;
  }
}
