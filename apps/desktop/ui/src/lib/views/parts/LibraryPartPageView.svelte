<script lang="ts">
  // A library part's page (SM-NS-15.2): name, kind, where it is from, its summary, the agents that use it
  // and **Check it**. The part's text laid out by kind is not drawn in R1 (G-CU-2). The heading draws the
  // part's kind icon (V8 `ptIco`).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-15
  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import Icon from '../../icons/Icon.svelte';
  import { partKindIcon } from '../../icons/kinds';
  import CheckItPanel from '../fixed/CheckItPanel.svelte';
  import { PART_KIND_MESSAGE } from './part-kinds';
  import type { LibraryPartPageController } from './part-page.svelte';

  interface Props {
    backend: BackendState;
    workspaceId: string | null;
    c: LibraryPartPageController;
  }
  let { backend, workspaceId, c }: Props = $props();
</script>

<section class="part-page" aria-labelledby="part-page-heading">
  {#if c.part !== null}
    {@const view = c.part}
    <h2 id="part-page-heading" tabindex="-1" data-region-focus><span class="icon-tile" data-tint={partKindIcon(view.part.kind) === 'agent' ? 'accent-ink' : undefined}><Icon name={partKindIcon(view.part.kind)} /></span>{view.part.name}</h2>
    <div class="page">
    <p class="kind">{t(PART_KIND_MESSAGE[view.part.kind])}</p>
    <p class="kind">{t('part-page-from', { name: view.from })}</p>
    <p>{view.summary}</p>
    {#if c.usedBy.length > 0}
      <h3>{t('part-page-used-by')}</h3>
      <ul>{#each c.usedBy as agent (agent.part.id)}<li><span class="icon-tile" data-tint="accent-ink"><Icon name="agent" /></span>{agent.part.name}</li>{/each}</ul>
    {/if}
    </div>
    <CheckItPanel
      {backend}
      {workspaceId}
      currentPart={view.part.identifier}
      subjectName={view.part.name}
      subject={{ kind: 'library', agentRepositoryId: view.part.agent_repository_id, partKind: view.part.kind, identifier: view.part.identifier }}
    />
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
  .page > h3 {
    font-size: 17px;
    margin-block-start: var(--space-sm);
  }
  .page > ul {
    margin: 0;
    padding-inline-start: 22px;
  }
  .page > ul > :global(li) {
    display: flex;
    gap: var(--space-04);
    align-items: center;
  }
</style>
