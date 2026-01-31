<script lang="ts">
  // "What Ordin used" under an answer: a quiet link that, only when chosen, opens a panel with the facts
  // and links the answer used (ADR-111 step 7). An answer with no `memory_used` draws nothing.
  //
  // spec: ordin-desktop/components/ScreensModule#ADR-111
  // spec: ordin-desktop/components/ScreensModule#SM-CU-11
  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import FactLine from './FactLine.svelte';
  import LinkLine from './LinkLine.svelte';
  import { UsedController } from './memory-used.svelte';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    /** The answer block's `memory_used`. */
    recallIds: readonly string[] | undefined;
    onopenpage?: (pageId: string) => void;
  }
  let { backend, workspaceId, recallIds, onopenpage }: Props = $props();

  const c = new UsedController(backend.handle, workspaceId, recallIds ?? []);
  let shown = $state(false);
  const pending = $derived(c.memory.pending);

  async function choose(): Promise<void> {
    shown = true;
    await c.load();
  }
</script>

{#if c.offered}
  <p><button type="button" class="link" aria-expanded={shown} onclick={() => void choose()}>{t('memory-used')}</button></p>
  {#if shown}
    <section class="memory-used" aria-labelledby="memory-used-heading">
      <h3 id="memory-used-heading" tabindex="-1">{t('memory-used-title')}</h3>
      <ErrorLine error={c.error} />
      <ul>
        {#each c.facts as fact (fact.id)}
          {@const title = fact.page_id ? c.pageTitles[fact.page_id] : undefined}
          <FactLine {fact} names={c.memory.names} onforget={(f) => void c.memory.askForgetFact(f.id)} onopenpage={fact.page_id && title !== null ? () => onopenpage?.(fact.page_id as string) : undefined} />
          {#if title === null}
            <li class="secondary">{t('memory-page-gone')}</li>
          {:else if title !== undefined}
            <li class="secondary">{fact.page_section ? t('memory-used-page', { page: title, section: fact.page_section }) : title}</li>
          {/if}
        {/each}
      </ul>
      <ul>
        {#each c.links as link (link.id)}
          <LinkLine {link} names={c.memory.names} pageTitles={c.pageTitles} onforget={(l) => c.memory.askForgetLink(l)} />
        {/each}
      </ul>
      {#if c.gone > 0}<p class="secondary">{t('memory-used-gone', { count: c.gone })}</p>{/if}
    </section>
  {/if}
  {#if pending !== null}
    <ConfirmDialog
      title={pending.kind === 'fact' ? t('memory-forget-confirm') : pending.kind === 'link' ? t('memory-link-forget-confirm', { from: pending.link.from.name, kind: pending.link.kind, to: pending.link.to.name }) : ''}
      body={pending.kind === 'fact' && pending.summaries > 0 ? t('memory-forget-confirm-summaries', { count: pending.summaries }) : undefined}
      verb={t('memory-link-forget')}
      onconfirm={() => void c.confirmForget()}
      oncancel={() => c.memory.cancelPending()}
    />
  {/if}
{/if}

<style>
  button.link {
    border: none;
    background: none;
    color: var(--color-text-muted);
    text-decoration: underline;
    padding: 0;
  }
  .secondary {
    color: var(--color-text-muted);
  }
</style>
