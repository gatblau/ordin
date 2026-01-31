// Team knowledge (ADR-103, ADR-110): the workspace's header and panel, the share and read dialogs, and
// the address split. Three rules on screen: no Git word; the sentence, never the code; what is shared is
// said before the first share. The status is read when the workspace opens and on every
// `workspace_changed` for it, which is how the outcome of a check, get or share reaches the screen.
//
// spec: ordin-desktop/components/ScreensModule#ADR-103
// spec: ordin-desktop/components/ScreensModule#SM-NS-13
import type { AdoptEdit, AdoptItem, AdoptResult, ServiceError, SourceLink, TeamFolderChoice, TeamKnowledgeStatus, TeamResultsChoice, TeamResultsChoices, TeamWorkspaceSharePreview } from '../../service-api/types.gen';
import { t } from '../../i18n/fluent';
import { Announcer, type Handle } from './util';

export interface ParsedFolder {
  url: string;
  branch: string;
  path: string;
}

/**
 * Splits a folder's web address as the host shows it (`TeamFolderChoice::NewFolder`), so the person never
 * types a branch: GitHub `https://<host>/<owner>/<name>/tree/<branch>/<path>`, GitLab
 * `https://<host>/<group>/<name>/-/tree/<branch>/<path>`. A branch name containing `/` is read as its first
 * segment (chosen here). Any other text is `null`.
 */
export function splitFolderAddress(text: string): ParsedFolder | null {
  let url: URL;
  try {
    url = new URL(text.trim());
  } catch {
    return null;
  }
  if (url.protocol !== 'https:' || url.username !== '' || url.password !== '') return null;
  const segments = url.pathname.split('/').filter((s) => s !== '');
  const dash = segments.indexOf('-');
  let repoEnd: number;
  if (dash >= 2 && segments[dash + 1] === 'tree') repoEnd = dash;
  else if (segments[2] === 'tree') repoEnd = 2;
  else return null;
  const branch = segments[repoEnd + (dash >= 0 && repoEnd === dash ? 2 : 1)];
  if (branch === undefined || branch === '') return null;
  const rest = segments.slice(repoEnd + (dash >= 0 && repoEnd === dash ? 3 : 2));
  if (repoEnd < 2) return null;
  const repo = segments.slice(0, repoEnd).join('/');
  return { url: `https://${url.host}/${repo}`, branch: decodeURIComponent(branch), path: rest.map(decodeURIComponent).join('/') };
}

/**
 * The sentence for a `last_error` code of the status. The status carries the code only; the sentences
 * below are the Team Knowledge Service's (SM-CU-10).
 */
export function teamErrorMessage(code: string): { id: string; action: ServiceError['action'] | null } {
  switch (code) {
    case 'TEAM_NOTHING_TO_SHARE':
      return { id: 'team-nothing-to-share', action: null };
    case 'TEAM_REMOTE_UNREACHABLE':
      return { id: 'team-error-unreachable', action: 'try_again' };
    case 'TEAM_SIGN_IN_NEEDED':
      return { id: 'team-error-sign-in', action: 'sign_in_again' };
    case 'TEAM_READ_ONLY':
      return { id: 'team-error-read-only', action: null };
    // spec: ordin-desktop/components/ScreensModule#SM-TW-11
    case 'TEAM_WS_FOLDER_TAKEN':
      return { id: 'team-ws-error-folder-taken', action: null };
    default:
      return { id: 'error-svc-internal', action: 'try_again' };
  }
}

/** The text of a fact or page as the Adopt dialog holds it. */
export interface AdoptText {
  statement: string;
  title: string;
  body: string;
}

/** `edit` for `team_knowledge.adopt`: only the fields the person changed; `null` when none. */
export function adoptEdit(kind: AdoptItem['kind'], was: AdoptText, now: AdoptText): AdoptEdit | null {
  const edit: AdoptEdit = {};
  if (kind === 'fact') {
    if (now.statement !== was.statement) edit.statement = now.statement;
  } else {
    if (now.title !== was.title) edit.title = now.title;
    if (now.body !== was.body) edit.body = now.body;
  }
  return Object.keys(edit).length === 0 ? null : edit;
}

/** What a finished share or get carried, for `team-share-outcome` and `team-get-outcome`. */
export interface TeamOutcome {
  kind: 'share' | 'get';
  facts: number;
  pages: number;
}

/**
 * The line a finished share or get leaves, from `last_share` or `last_get` of the status that arrives
 * with `busy` cleared; `null` while the command runs, when it failed, when the report did not change
 * (`before` is the report's text when the command was sent) or when it counts no new or changed item.
 * Removed items are on the approval card only.
 */
// spec: ordin-desktop/components/ScreensModule#SM-TK-3
export function teamOutcome(kind: TeamOutcome['kind'], before: string, status: TeamKnowledgeStatus): TeamOutcome | null {
  if (status.busy != null || status.last_error) return null;
  const report = kind === 'share' ? status.last_share : status.last_get;
  if (report == null || JSON.stringify(report) === before) return null;
  const facts = report.counts.new_facts + report.counts.changed_facts;
  const pages = report.counts.new_pages + report.counts.changed_pages;
  return facts + pages === 0 ? null : { kind, facts, pages };
}

export class TeamController {
  status = $state.raw<TeamKnowledgeStatus | null>(null);
  candidates = $state.raw<SourceLink[]>([]);
  error = $state.raw<ServiceError | null>(null);
  /** SM-TK-3: what the person's latest share or get carried, once it has finished. */
  outcome = $state.raw<TeamOutcome | null>(null);
  /** The share or get the person started, waiting for the status that ends it. */
  #pending: { kind: TeamOutcome['kind']; before: string } | null = null;
  /** The command a Try again re-sends. */
  retry: (() => Promise<void>) | null = null;
  readonly announcer = new Announcer();
  readonly #handle: Handle;
  readonly workspaceId: string;

  constructor(handle: Handle, workspaceId: string) {
    this.#handle = handle;
    this.workspaceId = workspaceId;
  }

  get busy(): boolean {
    return this.status?.busy != null;
  }
  get bound(): boolean {
    return this.status?.binding != null;
  }
  /** Adopt is offered only when the workspace has an own folder with read-write access (step 6). */
  get canAdopt(): boolean {
    return this.status?.binding?.access === 'read_write';
  }
  /** The own folder's name as `team-adopt` and `team-adopt-title` take it: the header line, else the folder. */
  get ownFolderName(): string {
    return this.status?.header_line ?? this.status?.binding?.folder ?? '';
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('team_knowledge.status', { ws: this.workspaceId });
    if (out.kind === 'ok') this.#take(out.result);
    else this.error = out.error;
  }

  /** Keeps a status and, once a pending share or get has finished, the line it leaves (SM-TK-3). */
  #take(status: TeamKnowledgeStatus): void {
    this.status = status;
    if (this.#pending === null || status.busy != null) return;
    const { kind, before } = this.#pending;
    this.#pending = null;
    this.outcome = teamOutcome(kind, before, status);
    if (this.outcome !== null) {
      this.announcer.say(t(kind === 'share' ? 'team-share-outcome' : 'team-get-outcome', { facts: this.outcome.facts, pages: this.outcome.pages }));
    }
  }

  /** Remembers the report a share or get will replace, so only its own outcome is said. */
  #expect(kind: TeamOutcome['kind']): void {
    const report = kind === 'share' ? this.status?.last_share : this.status?.last_get;
    this.#pending = { kind, before: JSON.stringify(report ?? null) };
    this.outcome = null;
  }

  async loadCandidates(): Promise<void> {
    const out = await this.#handle.call('team_knowledge.candidate_links', { ws: this.workspaceId });
    if (out.kind === 'ok') this.candidates = out.result;
  }

  /** The command answers at once with the status (`busy` set); the screen waits for `workspace_changed`. */
  async #send(run: () => Promise<{ kind: 'ok'; result: TeamKnowledgeStatus } | { kind: 'error'; error: ServiceError }>): Promise<boolean> {
    const out = await run();
    if (out.kind === 'error') {
      this.error = out.error;
      this.retry = async () => void (await this.#send(run));
      this.#pending = null;
      return false;
    }
    this.error = null;
    this.#take(out.result);
    return true;
  }

  addReadFolder(choice: TeamFolderChoice): Promise<boolean> {
    return this.#send(() => this.#handle.call('team_knowledge.add_read_folder', { ws: this.workspaceId, choice }));
  }
  /** A get of the own folder ends in `team-get-outcome` (SM-TK-3); `last_get` reports the own folder only. */
  get(folderId?: string): Promise<boolean> {
    if (folderId === undefined) this.#expect('get');
    return this.#send(() => this.#handle.call('team_knowledge.get', { ws: this.workspaceId, ...(folderId === undefined ? {} : { folder_id: folderId }) }));
  }
  /** A share ends in `team-share-outcome` once its card is raised (SM-TK-3). */
  share(): Promise<boolean> {
    this.#expect('share');
    return this.#send(() => this.#handle.call('team_knowledge.share', { ws: this.workspaceId }));
  }
  checkNow(): Promise<boolean> {
    return this.#send(() => this.#handle.call('team_knowledge.check_now', { ws: this.workspaceId }));
  }
  stopReading(folderId: string): Promise<boolean> {
    return this.#send(() => this.#handle.call('team_knowledge.stop_reading', { ws: this.workspaceId, folder_id: folderId }));
  }

  /** The recall check answers at once with the status (`busy` = `checking_recall`); `workspace_changed` brings the result. */
  checkRecall(): Promise<boolean> {
    return this.#send(() => this.#handle.call('team_knowledge.check_recall', { ws: this.workspaceId }));
  }

  /** Saves one description of the own folder; an emptied field sends `text: null` (SM-NS-13, step 4). */
  setDescription(path: string, text: string): Promise<boolean> {
    return this.#send(() => this.#handle.call('team_knowledge.set_description', { ws: this.workspaceId, path, text: text === '' ? null : text }));
  }

  /**
   * **Adopt** (and **Adopt again** with `again`): `edit` carries only the fields the person changed, none
   * giving `null`. A refusal is returned to the dialog, which keeps the text.
   */
  async adopt(item: AdoptItem, edit: AdoptEdit | null, again = false): Promise<{ ok: true; result: AdoptResult } | { ok: false; error: ServiceError }> {
    const input = { ws: this.workspaceId, item, edit };
    const out = await (again ? this.#handle.call('team_knowledge.adopt_again', input) : this.#handle.call('team_knowledge.adopt', input));
    if (out.kind === 'error') return { ok: false, error: out.error };
    this.announcer.say(t('team-adopted'));
    return { ok: true, result: out.result };
  }

  /** **Keep as it is**. */
  async keepAsIs(item: AdoptItem): Promise<boolean> {
    const out = await this.#handle.call('team_knowledge.keep_as_is', { ws: this.workspaceId, item });
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.error = null;
    return true;
  }

  /** SM-TW-2: what the share dialog (the tree's **Share**, SM-TW-13) would share for `choice`; a refusal is kept for the dialog. */
  // spec: ordin-desktop/components/ScreensModule#SM-TW-2
  async sharePreview(choice: TeamFolderChoice): Promise<TeamWorkspaceSharePreview | null> {
    const out = await this.#handle.call('team_knowledge.workspace_share_preview', { ws: this.workspaceId, choice });
    if (out.kind === 'error') {
      this.error = out.error;
      this.retry = async () => void (await this.sharePreview(choice));
      return null;
    }
    this.error = null;
    return out.result;
  }

  /** **Share** of the share dialog: answers at once with `busy: sharing`; the card arrives in *Needs you*. */
  shareWorkspace(choice: TeamFolderChoice, results: TeamResultsChoice): Promise<boolean> {
    return this.#send(() => this.#handle.call('team_knowledge.share_workspace', { ws: this.workspaceId, choice, results }));
  }

  /**
   * SM-TW-5 **Change…** (amended January 2026, the user's decision): where results may be saved and
   * where they are saved now, read from `team_knowledge.results_choices` — the share preview refuses a
   * team workspace. A refusal is kept for the dialog.
   */
  // spec: ordin-desktop/components/ScreensModule#SM-TW-5
  async resultsChoices(): Promise<TeamResultsChoices | null> {
    const out = await this.#handle.call('team_knowledge.results_choices', { ws: this.workspaceId });
    if (out.kind === 'error') {
      this.error = out.error;
      this.retry = async () => void (await this.resultsChoices());
      return null;
    }
    this.error = null;
    return out.result;
  }

  /** SM-TW-5 **Change…**: where results are saved, shared at the next share as a workspace change. */
  // spec: ordin-desktop/components/ScreensModule#SM-TW-5
  setResultsPlace(results: TeamResultsChoice): Promise<boolean> {
    return this.#send(() => this.#handle.call('team_knowledge.set_results_place', { ws: this.workspaceId, results }));
  }

  /** SM-TW-10 **Leave the team workspace**, after the confirmation; `team-ws-left` is announced. */
  // spec: ordin-desktop/components/ScreensModule#SM-TW-10
  async leaveWorkspace(): Promise<boolean> {
    const out = await this.#handle.call('team_knowledge.leave_workspace', { ws: this.workspaceId });
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.error = null;
    this.announcer.say(t('team-ws-left'));
    await this.load();
    return true;
  }

  /** Stop sharing, after the confirmation. */
  async turnOff(): Promise<boolean> {
    const out = await this.#handle.call('team_knowledge.turn_off', { ws: this.workspaceId });
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.error = null;
    await this.load();
    return true;
  }
}
