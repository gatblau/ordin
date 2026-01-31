<script lang="ts">
  // An installed agent's part page (SM-NS-15.1): its name, kind, where it is from, what it does, its text
  // (read only, never as instructions to the reader), the file under Details for people who code, **Ask
  // Ordin about this part** and **Check it**. The heading draws the part's kind icon (V8 `ptIco`).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-15
  import AskButton from '../../blocks/AskButton.svelte';
  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import Icon from '../../icons/Icon.svelte';
  import { partKindIcon } from '../../icons/kinds';
  import CheckItPanel from '../fixed/CheckItPanel.svelte';
  import type { InstalledPartPageController } from './part-page.svelte';

  interface Props {
    backend: BackendState;
    workspaceId: string | null;
    agentInstanceId: string;
    c: InstalledPartPageController;
  }
  let { backend, workspaceId, agentInstanceId, c }: Props = $props();
  const paragraphs = (text: string): string[] => text.split(/\n\s*\n/).filter((p) => p.trim() !== '');
</script>

<section class="part-page" aria-labelledby="part-page-heading">
  {#if c.page !== null}
    {@const page = c.page}
    <h2 id="part-page-heading" tabindex="-1" data-region-focus><span class="icon-tile" data-tint={partKindIcon(page.line.part.kind) === 'agent' ? 'accent-ink' : undefined}><Icon name={partKindIcon(page.line.part.kind)} /></span>{page.line.name}</h2>
    <div class="page">
    <p class="kind">{t(page.plain_kind_message_id)}</p>
    <p class="kind">{page.line.from ? t('part-page-from', { name: page.line.from }) : t('part-page-from-ordin')}</p>
    <p>{page.line.what_it_does}</p>
    {#each paragraphs(page.text) as paragraph, i (i)}<p class="text">{paragraph}</p>{/each}
    </div>
    {#if c.detail === 'links_code' && page.file}
      <details>
        <summary>{t('part-page-details')}</summary>
        <p>{page.file}</p>
      </details>
    {/if}
    {#if workspaceId !== null}
      <AskButton {backend} {workspaceId} labelId="repo-ask-about-part" text={t('block-ask-text-this', { title: page.line.name })} />
    {/if}
    <CheckItPanel {backend} {workspaceId} subjectName={page.line.name} currentPart={page.line.part.id} subject={{ kind: 'installed', agentInstanceId }} />
  {/if}
  <ErrorLine error={c.error} onretry={() => void c.load()} />
</section>

<style>
  /* The prototype's part page (scene `partRead`, `.mopage`). */
  .part-page {
    gap: var(--space-12);
  }
  .part-page > h2 {
    font-size: 19px;
    display: flex;
    gap: var(--space-06);
    align-items: center;
  }
  /* The part's words in one surface (the prototype's `.mdv.mdread`). */
  .page {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-lg);
    padding: var(--space-md) 18px;
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .page > .kind {
    color: var(--color-text-muted);
  }

  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>
