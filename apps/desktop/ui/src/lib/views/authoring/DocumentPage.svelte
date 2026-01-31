<script lang="ts">
  // The document page (SM-NS-4; instance = `<place id>:<path>`): the name and where it lives, at a glance,
  // the page, Versions, Published as and Edit (the editor on the full pane). Icons per V8.
  // Amended January 2026 (user decision; Approved by the user January 2026): the pane shows content only.
  // Ask Ordin about this, Open in its app and Move to Bin… live in the document's ⋯ menu and right-click
  // menu in the tree (TreeFolder.svelte, SM-RL-3); the pane keeps **Open in its app** only after the last
  // page drawn of a long PDF (SM-PDF-1) and the `no_editor` question that press can raise.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-4
  // spec: ordin-desktop/components/ScreensModule#SM-RL-3
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import MarkdownEditor from '../../editor/MarkdownEditor.svelte';
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
  import { DocumentController, originWords } from './document.svelte';
  import { closeScreen, splitDocumentInstance } from './support';
  import { appOrNull } from '../fixed/util';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  const { placeId, path } = splitDocumentInstance(instance);
  const key = makeKey(SCREEN_IDS.document, instance);
  // The app context is read while the screen is set up; Svelte's getContext answers nothing in a callback.
  const app = appOrNull();
  const c = new DocumentController(backend.handle, placeId, path, workspaceId, () => closeScreen(app, backend, workspaceId, key));

  let restoring = $state<number | null>(null);
  const canOpenInApp = $derived(answers(backend, 'craft.open'));
  // V8 *Document heading*: the file kind's icon, drawn only when the name's extension names one.
  const headingIcon = $derived(fileKindIcon(c.name));
  // V8 *Chips*: a copy kept of a document draws its file kind's icon, else `page`; an exact copy is the
  // original's kind.
  const copyIcon = (format: CopyFormat): IconName => (format === 'exact_file' ? (fileKindIcon(c.name) ?? 'page') : format);

  onMount(() => {
    void c.load();
    if (answers(backend, 'workspaces.links')) void c.loadPlaceName();
  });

</script>

<section class="document" aria-labelledby="document-heading">
  <div class="heading">
    {#if headingIcon !== null}<span class="icon-tile" data-tint={fileKindTint(headingIcon)}><Icon name={headingIcon} size="heading" /></span>{/if}
    <h2 id="document-heading" tabindex="-1" data-region-focus>{c.name}</h2>
  </div>
  {#if c.location !== null}<p class="secondary location">{t('doc-location', { location: c.location })}</p>{/if}
  {#if c.editing}
    <!-- Amended January 2026 (user decision; Approved by the user January 2026): a clear way out of the
         editor. **Done** saves and goes back to the page; **Cancel** goes back without saving, asking
         first (ConfirmDialog) when something was changed; Escape is Cancel. -->
    <div
      class="editing"
      role="group"
      aria-label={c.name}
      onkeydown={(e) => {
        if (e.key === 'Escape' && !e.defaultPrevented) {
          e.preventDefault();
          c.cancel();
        }
      }}
    >
      <div class="bar" role="toolbar" aria-label={c.name}>
        <button type="button" class="go" data-default="true" disabled={c.saving} onclick={() => void c.save()}>{t('edit-done')}</button>
        <button type="button" onclick={() => c.cancel()}>{t('edit-cancel')}</button>
      </div>
      <ErrorLine error={c.saveError} onretry={() => void c.save()} />
      <!-- Amended January 2026 (user decision; Approved by the user January 2026): the what-you-see editor
           (UX guidelines, Writing); the person never sees Markdown codes. -->
      <MarkdownEditor bind:value={c.draft} label={c.name} minHeight="60vh" />
    </div>
    {#if c.askingDiscard}
      <ConfirmDialog
        title={t('edit-discard-title')}
        verb={t('edit-discard-verb')}
        cancelLabel={t('edit-keep-editing')}
        onconfirm={() => c.discardChanges()}
        oncancel={() => c.keepChanging()}
      />
    {/if}
  {:else}
    <ErrorLine error={c.error} onretry={() => void c.load()} />
    {#if c.glance !== null}<p class="secondary">{c.glance}</p>{/if}
    {#if c.canEdit}
      <div class="actions">
        <button type="button" onclick={() => c.startEdit()}>{t('doc-edit')}</button>
      </div>
    {/if}
    {#if c.openState === 'no_editor'}
      <p>{t('doc-no-editor')}</p>
      <button type="button" onclick={() => void c.openUsualApp()}><Icon name="open" />{t('doc-open-usual-app')}</button>
    {/if}
    {#if c.savedNotSent}<p class="secondary" role="status">{t('doc-saved-not-sent')}</p>{/if}
    {#if c.preview !== null}
      <PreviewPane preview={c.preview} label={c.name} onDocumentPage onopen={canOpenInApp ? () => void c.openInApp() : undefined} />
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
  .editing {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .actions,
  .bar {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
    align-items: center;
  }
  .live {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    overflow: hidden;
  }
</style>
