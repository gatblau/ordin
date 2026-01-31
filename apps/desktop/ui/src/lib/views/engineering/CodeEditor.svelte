<script lang="ts">
  // The code editor screen (Monaco, loaded on first use). One tab per file. The text is the file as the
  // Project Service holds it; Save writes through `projects.save_file`; the code-checking bar sits before
  // the editor in reading order (SM-SF-3).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-3
  import { onDestroy, onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { ScreenProps } from '../../shell/screens';
  import ErrorLine from '../ErrorLine.svelte';
  import { lastSegment } from '../fixed/util';
  import CodeCheckingBar from './CodeCheckingBar.svelte';
  import OpenInIde from './OpenInIde.svelte';
  import { editorText, hubFor } from './editor-hub.svelte';
  import { loadMonaco, languageFor } from './monaco';
  import { projectOf } from './projects';

  let { backend, instance }: ScreenProps = $props();

  const handle = backend.handle;
  const hub = hubFor(handle);
  // The screen instance is `<project id>:<path>`; a project id holds no colon.
  const at = $derived((instance ?? '').indexOf(':'));
  const projectId = $derived(at < 0 ? (instance ?? '') : (instance ?? '').slice(0, at));
  const path = $derived(at < 0 ? '' : (instance ?? '').slice(at + 1));

  let host = $state<HTMLElement | null>(null);
  let error = $state<import('../../service-api/types.gen').ServiceError | null>(null);
  let saveError = $state<import('../../service-api/types.gen').ServiceError | null>(null);
  let truncated = $state(false);
  let dirty = $state(false);
  let saved = '';
  let disposers: Array<() => void> = [];
  let setText: ((text: string) => void) | null = null;
  let getText: (() => string) | null = null;
  let hash: string | null = null;

  const bar = $derived(hub.bars.get(projectId));
  const editor = $derived(hub.editors.get(`${projectId}:${path}`));

  onMount(() => {
    void start();
    return () => {
      for (const d of disposers) d();
      disposers = [];
    };
  });

  onDestroy(() => {
    void hub.close(`${projectId}:${path}`);
  });

  async function start(): Promise<void> {
    await hub.loadProject(projectId);
    const opened = await hub.open(projectId, path);
    if (!opened.ok) {
      error = opened.error;
      return;
    }
    let text = '';
    const placeId = projectOf(handle.id, projectId)?.place_id;
    if (placeId !== undefined) {
      const preview = await handle.call('files.preview', { path, place_id: placeId });
      if (preview.kind === 'ok') {
        const read = editorText(preview.result);
        if (read !== null) {
          text = read.text;
          truncated = read.truncated;
        }
      } else {
        error = preview.error;
      }
    }
    saved = text;
    const monaco = await loadMonaco();
    if (host === null) return;
    const model = monaco.editor.createModel(text, languageFor(path, monaco.languages.getLanguages()));
    const view = monaco.editor.create(host, { model, automaticLayout: true, readOnly: truncated, ariaLabel: lastSegment(path), links: false });
    setText = (next) => model.setValue(next);
    getText = () => model.getValue();
    const sub = model.onDidChangeContent(() => {
      dirty = model.getValue() !== saved;
    });
    disposers.push(() => sub.dispose(), () => view.dispose(), () => model.dispose());
  }

  async function save(): Promise<void> {
    if (getText === null) return;
    const text = getText();
    const out = await handle.call('projects.save_file', { path, project_id: projectId, text, expected_hash: hash });
    if (out.kind === 'ok') {
      hash = out.result;
      saved = text;
      dirty = false;
      saveError = null;
    } else {
      saveError = out.error;
    }
  }

  function discard(): void {
    setText?.(saved);
    dirty = false;
  }

  function onkeydown(event: KeyboardEvent): void {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') {
      event.preventDefault();
      void save();
    }
  }

</script>

<section class="code-editor" aria-labelledby="code-editor-heading">
  <h2 id="code-editor-heading" tabindex="-1" data-region-focus>{lastSegment(path)}</h2>
  <ErrorLine error={hub.projectErrors.get(projectId) ?? null} />
  {#if bar !== undefined}
    <CodeCheckingBar
      project={hub.projectName(projectId)}
      busy={bar.busy}
      error={bar.error}
      onanswer={(allowed) => void hub.answer(projectId, allowed)}
    />
  {/if}
  <ErrorLine {error} />
  {#if editor !== undefined}<p class="server-line">{editor.serverLine}</p>{/if}
  <div class="editor-actions">
    <button type="button" class="small primary" disabled={!dirty} onclick={() => void save()}>{t('engineering-save')}</button>
    <button type="button" class="small" disabled={!dirty} onclick={discard}>{t('engineering-discard-edits')}</button>
    <!-- Open in [IDE] for this file (UX-A25); the Code group of the side tree is not drawn in this build. -->
    {#if path !== ''}<OpenInIde {backend} {projectId} {path} name={lastSegment(path)} />{/if}
  </div>
  <ErrorLine error={saveError} onretry={() => void save()} />
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="editor-host" bind:this={host} {onkeydown}></div>
  <div class="visually-hidden" aria-live="polite" role="status">{hub.announcer.text}</div>
</section>

<style>
  /* The code editor is an editing page (scene `engEdit`; V5 *Tabbed screen body*, an editing page:
     10 px 16 px): its name in Screen title, the Small buttons under it, the editor filling the rest. */
  :global(.screen-panel:has(> .code-editor)) {
    padding: var(--space-10) var(--space-md);
  }
  .code-editor {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    max-inline-size: none;
    min-block-size: 60vh;
    block-size: 100%;
  }
  h2 {
    margin: 0;
    font-size: 18px;
  }
  .editor-host {
    flex: 1;
    min-block-size: 240px;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    overflow: hidden;
  }
  .editor-host:focus-within {
    outline: 3px solid var(--color-focus);
    outline-offset: 2px;
  }
  .server-line {
    color: var(--color-text-muted);
    font-size: 13px;
    margin: 0;
  }
  .editor-actions {
    display: flex;
    gap: var(--space-06);
    align-items: center;
  }
</style>
