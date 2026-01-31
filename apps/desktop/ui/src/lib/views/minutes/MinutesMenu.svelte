<script lang="ts">
  // The menu of one minutes item (ScreensModule.md SM-MIN-13), wherever the item is drawn: the tree row, the
  // Minutes screen's list and the minutes page. A ⋯ icon button opens it; a row also opens it on right-click
  // by setting `open`. It holds **Export** (opening to **PDF…** and **Word…**), **Delete recording…** (the
  // page only, while there is a recording) and **Remove…**; both removals ask first. Escape closes it and the
  // first item takes focus, as in the tree's menus.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-13
  import type { ServiceError } from '../../backend/backend';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { Minutes } from '../../service-api/minutes.gen';
  import type { KnowledgeExportFormat } from '../../service-api/types.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import { appOrNull } from '../fixed/util';

  interface Props {
    backend: BackendState;
    minutesId: string;
    title: string;
    /** Open or closed; a row sets it on right-click. */
    open?: boolean;
    /** The page only: Delete recording… is offered while this is true. */
    hasRecording?: boolean;
    /** The ⋯ button shows only on hover or focus, as in the tree; the page shows it always. */
    shown?: boolean;
    onerror: (error: ServiceError | null) => void;
    onremoved?: () => void;
    onrecordingdeleted?: (minutes: Minutes) => void;
  }
  let { backend, minutesId, title, open = $bindable(false), hasRecording = false, shown = false, onerror, onremoved, onrecordingdeleted }: Props = $props();
  const app = appOrNull();
  let exportOpen = $state(false);
  let asking = $state<'delete-recording' | 'remove' | null>(null);
  let busy = $state(false);

  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }

  function close(): void {
    open = false;
    exportOpen = false;
  }

  async function exportAs(format: KnowledgeExportFormat): Promise<void> {
    close();
    onerror(null);
    const out = await backend.handle.call('minutes.export', { minutes_id: minutesId, format });
    if (out.kind === 'ok') app?.shell.announce(t('minutes-exported'));
    else if (out.error.code !== 'DSK_DIALOG_CANCELLED') onerror(out.error);
  }

  function ask(what: 'delete-recording' | 'remove'): void {
    close();
    asking = what;
  }

  async function confirm(): Promise<void> {
    const what = asking;
    if (what === null || busy) return;
    busy = true;
    if (what === 'delete-recording') {
      const out = await backend.handle.call('minutes.delete_recording', { minutes_id: minutesId });
      busy = false;
      asking = null;
      if (out.kind === 'error') onerror(out.error);
      else {
        onerror(null);
        onrecordingdeleted?.(out.result);
      }
      return;
    }
    const out = await backend.handle.call('minutes.remove', { minutes_id: minutesId });
    busy = false;
    asking = null;
    if (out.kind === 'error') {
      onerror(out.error);
      return;
    }
    onerror(null);
    app?.screens.closeScreensFor('minutes', minutesId);
    onremoved?.();
  }
</script>

<span class="minutes-menu">
  <span class="tree-actions" class:shown={shown || open}>
    <button type="button" class="tree-action" data-testid="minutes-menu" aria-haspopup="menu" aria-expanded={open} aria-label={t('more-actions-for', { name: title })} title={t('more-actions-tip')} onclick={() => (open ? close() : (open = true))}><Icon name="more" /></button>
  </span>
  {#if open}
    <div role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('tree-node-menu', { name: title })} use:focusFirst onkeydown={(e) => { if (e.key === 'Escape') { e.preventDefault(); close(); } }}>
      <button type="button" role="menuitem" data-testid="minutes-export" aria-haspopup="menu" aria-expanded={exportOpen} onclick={() => (exportOpen = !exportOpen)}><Icon name="save" />{t('minutes-export')}</button>
      {#if exportOpen}
        <div role="menu" class="export-list" aria-label={t('minutes-export')}>
          <button type="button" role="menuitem" data-testid="minutes-export-pdf" onclick={() => void exportAs('pdf')}><Icon name="pdf" />{t('minutes-export-pdf')}</button>
          <button type="button" role="menuitem" data-testid="minutes-export-word" onclick={() => void exportAs('docx')}><Icon name="word" />{t('minutes-export-word')}</button>
        </div>
      {/if}
      {#if hasRecording}<button type="button" role="menuitem" data-testid="minutes-delete-recording" onclick={() => ask('delete-recording')}><Icon name="bin" />{t('minutes-delete-recording')}</button>{/if}
      <button type="button" role="menuitem" class="danger" data-testid="minutes-remove" onclick={() => ask('remove')}><Icon name="remove" />{t('minutes-remove')}</button>
    </div>
  {/if}
</span>

{#if asking === 'delete-recording'}
  <ConfirmDialog title={t('minutes-delete-recording-title')} body={t('minutes-delete-recording-body')} verb={t('minutes-delete-recording-verb')} onconfirm={() => void confirm()} oncancel={() => (asking = null)} />
{:else if asking === 'remove'}
  <ConfirmDialog title={t('minutes-remove-title', { title })} body={t('minutes-remove-body')} verb={t('minutes-remove-verb')} onconfirm={() => void confirm()} oncancel={() => (asking = null)} />
{/if}

<style>
  .minutes-menu {
    position: relative;
    display: inline-flex;
  }
  .export-list {
    display: flex;
    flex-direction: column;
    padding-inline-start: var(--space-md);
  }
</style>
