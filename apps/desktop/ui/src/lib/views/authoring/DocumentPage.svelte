<script lang="ts">
  // The document page (SM-NS-4; instance = `<place id>:<path>`): at a glance, the page, Versions, Published as,
  // Edit (the editor on the full pane) and, under More actions, Move to Bin… Icons per V8.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-4
  import { tick, onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import AskButton from '../../blocks/AskButton.svelte';
  import PreviewPane from '../../blocks/PreviewPane.svelte';
  import { formatDate } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import { makeKey, SCREEN_IDS, type ScreenProps } from '../../shell/screens';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import Icon from '../../icons/Icon.svelte';
  import { fileKindIcon, fileKindTint } from '../../icons/kinds';
  import type { IconName } from '../../icons/sprite';
  import type { CopyFormat } from '../../service-api/types.gen';
  import { applyTool, type EditTool } from './markdown-edit';
  import { DocumentController, originWords } from './document.svelte';
  import { closeScreen, splitDocumentInstance } from './support';
  import { appOrNull } from '../fixed/util';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  const { placeId, path } = splitDocumentInstance(instance);
  const key = makeKey(SCREEN_IDS.document, instance);
  // The app context is read while the screen is set up; Svelte's getContext answers nothing in a callback.
  const app = appOrNull();
  const c = new DocumentController(backend.handle, placeId, path, workspaceId, () => closeScreen(app, backend, workspaceId, key));

  let confirmingBin = $state(false);
  let restoring = $state<number | null>(null);
  let editor = $state<HTMLTextAreaElement | null>(null);
  const canOpenInApp = $derived(answers(backend, 'craft.open'));
  // V8 *Document heading*: the file kind's icon, drawn only when the name's extension names one.
  const headingIcon = $derived(fileKindIcon(c.name));
  // V8 *Chips*: a copy kept of a document draws its file kind's icon, else `page`; an exact copy is the
  // original's kind.
  const copyIcon = (format: CopyFormat): IconName => (format === 'exact_file' ? (fileKindIcon(c.name) ?? 'page') : format);

  onMount(() => {
    void c.load();
  });

  async function tool(which: EditTool): Promise<void> {
    if (editor === null) return;
    const edit = applyTool(c.draft, editor.selectionStart, editor.selectionEnd, which);
    c.draft = edit.text;
    await tick();
    editor.focus();
    editor.setSelectionRange(edit.start, edit.end);
  }
  const TOOLS: ReadonlyArray<{ tool: EditTool; label: string }> = [
    { tool: 'heading', label: 'doc-tool-heading' },
    { tool: 'bold', label: 'doc-tool-bold' },
    { tool: 'italic', label: 'doc-tool-italic' },
    { tool: 'bullets', label: 'doc-tool-bullets' },
    { tool: 'numbers', label: 'doc-tool-numbers' },
  ];
</script>

<section class="document" aria-labelledby="document-heading">
  <div class="heading">
    {#if headingIcon !== null}<span class="icon-tile" data-tint={fileKindTint(headingIcon)}><Icon name={headingIcon} size="heading" /></span>{/if}
    <h2 id="document-heading" tabindex="-1" data-region-focus>{c.name}</h2>
  </div>
  {#if c.editing}
    <div class="bar" role="toolbar" aria-label={c.name}>
      <button type="button" data-default="true" disabled={c.saving} onclick={() => void c.save()}>{t('doc-save')}</button>
      <button type="button" onclick={() => c.cancel()}>{t('doc-cancel')}</button>
      {#each TOOLS as item (item.tool)}
        <button type="button" onclick={() => void tool(item.tool)}>{t(item.label)}</button>
      {/each}
      {#if c.askingDiscard}
        <span class="question" role="group" aria-label={t('doc-discard-question')}>
          {t('doc-discard-question')}
          <button type="button" onclick={() => c.discardChanges()}>{t('doc-discard-verb')}</button>
          <button type="button" onclick={() => c.keepChanging()}>{t('doc-keep-changing')}</button>
        </span>
      {/if}
    </div>
    <ErrorLine error={c.saveError} onretry={() => void c.save()} />
    <textarea bind:this={editor} bind:value={c.draft} class="editor" aria-label={c.name} spellcheck="true"></textarea>
  {:else}
    <ErrorLine error={c.error} onretry={() => void c.load()} />
    {#if c.glance !== null}<p class="secondary">{c.glance}</p>{/if}
    <div class="actions">
      {#if canOpenInApp}
        <button type="button" onclick={() => void c.openInApp()}><Icon name="open" />{t('doc-open-in-app')}</button>
      {/if}
      {#if workspaceId !== null}
        <AskButton {backend} {workspaceId} labelId="block-ask-about-this" text={t('block-ask-text-this', { title: c.name })} attachments={[{ kind: 'place_file', place_id: c.placeId, relative_path: c.path }]} />
      {/if}
      {#if c.canEdit}<button type="button" onclick={() => c.startEdit()}>{t('doc-edit')}</button>{/if}
      <details class="more">
        <summary aria-label={t('more-actions-for', { name: c.name })} title={t('more-actions-tip')}><Icon name="more" /></summary>
        <button type="button" onclick={() => (confirmingBin = true)}><Icon name="remove" />{t('doc-move-to-bin')}</button>
      </details>
    </div>
    {#if c.openState === 'no_editor'}
      <p>{t('doc-no-editor')}</p>
      <button type="button" onclick={() => void c.openUsualApp()}><Icon name="open" />{t('doc-open-usual-app')}</button>
    {/if}
    {#if c.savedNotSent}<p class="secondary" role="status">{t('doc-saved-not-sent')}</p>{/if}
    {#if c.preview !== null}
      <PreviewPane preview={c.preview} label={c.name} onDocumentPage />
      {#if c.truncated}<p class="secondary">{t('doc-cut')}</p>{/if}
    {/if}
    {#if c.ordered.length > 0}
      <details class="versions">
        <summary>{t('doc-versions')}</summary>
        <ul>
          {#each c.ordered as version, index (version.id)}
            <li>
              {t('doc-version-line', { version: version.version, date: formatDate(version.created_at), who: originWords(version.origin) })}
              {#if index > 0}<button type="button" onclick={() => (restoring = version.version)}>{t('doc-restore')}</button>{/if}
            </li>
          {/each}
        </ul>
      </details>
    {/if}
    {#if c.isMarkdown}
      <details class="copies">
        <summary>{t('doc-published-as', { count: c.copies.length })}</summary>
        <ul>
          {#each c.copies as copy (copy.id)}
            <li><span class="icon-tile" data-tint={fileKindTint(copyIcon(copy.format))}><Icon name={copyIcon(copy.format)} size="small" /></span>{copy.audience}{#if copy.keep_up_to_date} · {t('doc-copy-kept')}{/if}</li>
          {/each}
        </ul>
      </details>
    {/if}
  {/if}
  {#if confirmingBin}
    <ConfirmDialog
      title={t('doc-bin-title', { name: c.name })}
      body={t('doc-bin-body')}
      verb={t('doc-bin-verb')}
      onconfirm={() => {
        confirmingBin = false;
        void c.moveToBin();
      }}
      oncancel={() => (confirmingBin = false)}
    />
  {/if}
  {#if restoring !== null}
    {@const target = c.ordered.find((v) => v.version === restoring)}
    <ConfirmDialog
      title={t('doc-restore-title', { version: restoring })}
      body={t('doc-restore-body')}
      verb={t('doc-restore')}
      destructive={false}
      onconfirm={() => {
        restoring = null;
        if (target !== undefined) void c.restore(target);
      }}
      oncancel={() => (restoring = null)}
    />
  {/if}
  <div class="live" aria-live="polite">{c.announcer.text}</div>
</section>

<style>
  .secondary {
    color: var(--color-text-muted);
  }
  .heading {
    display: flex;
    align-items: center;
    gap: var(--space-12);
  }
  .actions,
  .bar {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
    align-items: center;
  }
  .editor {
    inline-size: 100%;
    min-block-size: 60vh;
    font: inherit;
  }
  .live {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    overflow: hidden;
  }
</style>
