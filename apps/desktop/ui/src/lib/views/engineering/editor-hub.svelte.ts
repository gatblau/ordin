// The open code editors of one backend and the code-checking question (SM-SF-3).
//
// A language server runs the project's own build scripts, so none starts until the person says yes.
// `projects.editor_open` returns `asks_to_start_servers = true` on an unanswered project; the editors
// of that project then show one bar. The screens keep no answer of their own: a later open that asks
// again draws the bar again (SM-SF-3.4).
//
// spec: ordin-desktop/components/ScreensModule#SM-SF-3
import { SvelteMap } from 'svelte/reactivity';

import { t } from '../../i18n/fluent';
import type { EditorSession, Preview, ServiceError } from '../../service-api/types.gen';
import { Announcer, type Handle } from '../fixed/util';
import { ensureProject, projectOf } from './projects';

export interface OpenEditor {
  /** `<project id>:<path>`; the key the screen was opened under. */
  key: string;
  projectId: string;
  path: string;
  stream: string;
  /** The line saying where completion comes from, or that it is off (from the Project Service). */
  serverLine: string;
  language: string | null;
}

export interface CodeCheckingBar {
  busy: boolean;
  error: ServiceError | null;
}

export type OpenResult = { ok: true; editor: OpenEditor; asks: boolean } | { ok: false; error: ServiceError };

function toEditor(projectId: string, path: string, s: EditorSession): OpenEditor {
  return { key: `${projectId}:${path}`, projectId, path, stream: s.stream, serverLine: s.server_line, language: s.language ?? null };
}

export class EditorHub {
  /** The projects whose editors show the code-checking bar. */
  readonly bars = new SvelteMap<string, CodeCheckingBar>();
  readonly editors = new SvelteMap<string, OpenEditor>();
  /** A project that could not be read (`PROJ_NOT_FOUND`): its sentence is drawn in place of its name. */
  readonly projectErrors = new SvelteMap<string, ServiceError>();
  /** Bumped when a project is read, so the name the bar draws is read again. */
  #seen = $state(0);
  readonly announcer = new Announcer();
  readonly #handle: Handle;

  constructor(handle: Handle) {
    this.#handle = handle;
  }

  /** `projects.editor_open`; draws the bar when the project asks (SM-SF-3.1, 3.4). */
  async open(projectId: string, path: string): Promise<OpenResult> {
    const out = await this.#handle.call('projects.editor_open', { path, project_id: projectId });
    if (out.kind === 'error') return { ok: false, error: out.error };
    const editor = toEditor(projectId, path, out.result);
    this.editors.set(editor.key, editor);
    if (out.result.asks_to_start_servers) this.bars.set(projectId, { busy: false, error: null });
    return { ok: true, editor, asks: out.result.asks_to_start_servers };
  }

  async close(key: string): Promise<void> {
    const editor = this.editors.get(key);
    if (editor === undefined) return;
    this.editors.delete(key);
    await this.#handle.call('projects.editor_close', { session: editor.stream });
  }

  editorsOf(projectId: string): OpenEditor[] {
    return [...this.editors.values()].filter((e) => e.projectId === projectId);
  }

  /** SM-WR-4: reads the project with `projects.get` when this backend has not seen it. */
  async loadProject(projectId: string): Promise<void> {
    const read = await ensureProject(this.#handle, projectId);
    if (read.ok) this.projectErrors.delete(projectId);
    else this.projectErrors.set(projectId, read.error);
    this.#seen++;
  }

  /** The name the bar's question uses: the project's display name, empty until it is read (SM-WR-4). */
  projectName(projectId: string): string {
    void this.#seen;
    return projectOf(this.#handle.id, projectId)?.display_name ?? '';
  }

  /**
   * Sends the person's answer (SM-SF-3.2). Both buttons are disabled while it is in flight. `ok` removes
   * the bar from every editor of the project and announces the result; after a yes, each open editor of
   * the project is closed and opened again so the Project Service's own check starts code checking.
   * A refusal is kept on the bar, which stays.
   */
  async answer(projectId: string, allowed: boolean): Promise<boolean> {
    const bar = this.bars.get(projectId);
    if (bar === undefined || bar.busy) return false;
    this.bars.set(projectId, { busy: true, error: null });
    const out = await this.#handle.call('projects.answer_language_servers', { allowed, project_id: projectId });
    if (out.kind === 'error') {
      this.bars.set(projectId, { busy: false, error: out.error });
      return false;
    }
    this.bars.delete(projectId);
    this.announcer.say(t(allowed ? 'code-checking-on' : 'code-checking-off'));
    if (allowed) {
      for (const editor of this.editorsOf(projectId)) {
        await this.#handle.call('projects.editor_close', { session: editor.stream });
        const again = await this.#handle.call('projects.editor_open', { path: editor.path, project_id: projectId });
        if (again.kind === 'ok') {
          this.editors.set(editor.key, toEditor(projectId, editor.path, again.result));
          // A project that still asks after a yes (a folder with nothing to bind the answer to) asks again.
          if (again.result.asks_to_start_servers) this.bars.set(projectId, { busy: false, error: null });
        } else {
          this.editors.delete(editor.key);
        }
      }
    }
    return true;
  }
}

/** The text of a file preview for the editor: text only; `null` for anything else. */
export function editorText(preview: Preview): { text: string; truncated: boolean } | null {
  return preview.kind === 'text' ? { text: preview.text, truncated: preview.truncated } : null;
}

const hubs = new WeakMap<object, EditorHub>();
/** One hub per backend handle, so every editor screen of a project shares one bar. */
export function hubFor(handle: Handle): EditorHub {
  let hub = hubs.get(handle);
  if (hub === undefined) {
    hub = new EditorHub(handle);
    hubs.set(handle, hub);
  }
  return hub;
}
