// Team knowledge (ADR-103, ADR-110): the workspace's header and panel, the share and read dialogs, and
// the address split. Three rules on screen: no Git word; the sentence, never the code; what is shared is
// said before the first share. The status is read when the workspace opens and on every
// `workspace_changed` for it, which is how the outcome of a check, get or share reaches the screen.
//
// spec: ordin-desktop/components/ScreensModule#ADR-103
// spec: ordin-desktop/components/ScreensModule#SM-NS-13
import type { AdoptEdit, AdoptItem, AdoptResult, ServiceError, SourceLink, TeamFolderChoice, TeamKnowledgeStatus } from '../../service-api/types.gen';
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

export class TeamController {
  status = $state.raw<TeamKnowledgeStatus | null>(null);
  candidates = $state.raw<SourceLink[]>([]);
  error = $state.raw<ServiceError | null>(null);
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
    if (out.kind === 'ok') this.status = out.result;
    else this.error = out.error;
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
      return false;
    }
    this.error = null;
    this.status = out.result;
    return true;
  }

  turnOn(choice: TeamFolderChoice): Promise<boolean> {
    return this.#send(() => this.#handle.call('team_knowledge.turn_on', { ws: this.workspaceId, choice }));
  }
  addReadFolder(choice: TeamFolderChoice): Promise<boolean> {
    return this.#send(() => this.#handle.call('team_knowledge.add_read_folder', { ws: this.workspaceId, choice }));
  }
  get(folderId?: string): Promise<boolean> {
    return this.#send(() => this.#handle.call('team_knowledge.get', { ws: this.workspaceId, ...(folderId === undefined ? {} : { folder_id: folderId }) }));
  }
  share(): Promise<boolean> {
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
