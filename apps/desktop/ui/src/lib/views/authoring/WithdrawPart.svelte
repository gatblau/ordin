<script lang="ts">
  // Withdraw [part] (SM-NS-7.4; instance = the part id).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-7
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import { makeKey, type ScreenProps } from '../../shell/screens';
  import ErrorLine from '../ErrorLine.svelte';
  import Icon from '../../icons/Icon.svelte';
  import { WithdrawController } from './repository.svelte';
  import { closeScreen, openSibling } from './support';
  import { appOrNull } from '../fixed/util';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  const key = makeKey('withdraw-part', instance);
  // The app context is read while the screen is set up; Svelte's getContext answers nothing in a callback.
  const app = appOrNull();
  const c = new WithdrawController(backend.handle, instance ?? '', (next) => {
    openSibling(app, backend, workspaceId, next);
    closeScreen(app, backend, workspaceId, key);
  });

  onMount(() => {
    void c.load();
  });
</script>

<section class="withdraw" aria-labelledby="withdraw-heading">
  <h2 id="withdraw-heading" tabindex="-1" data-region-focus>{t('withdraw-title', { name: c.part?.part.name ?? '' })}</h2>
  <ErrorLine error={c.error} onretry={() => void c.load()} />
  {#if c.part !== null}
    {#if (c.usedBy?.agents.length ?? 0) > 0}
      <p class="small">{t('withdraw-used-by')}</p>
      <ul class="used-by">
        {#each c.usedBy?.agents ?? [] as agent (agent.part.id)}<li><span class="icon-tile" data-tint="accent-ink"><Icon name="agent" /></span>{agent.part.name}</li>{/each}
      </ul>
    {/if}
    <label class="field">
      {t('withdraw-reason')}
      <textarea rows="2" bind:value={c.reason} required maxlength="1000"></textarea>
    </label>
    <label class="field">
      {t('withdraw-replace')}
      <select bind:value={c.replacement}>
        <option value=""></option>
        {#each c.replacements as entry (entry.part.part.id)}<option value={entry.part.part.id}>{entry.part.part.name}</option>{/each}
      </select>
    </label>
    <button type="button" class="danger" disabled={!c.reasonValid} onclick={() => void c.withdraw()}>{t('withdraw-verb')}</button>
  {/if}
</section>

<style>
  /* The prototype's Withdraw [part] (scene `partWithdraw`, `.ptmain.ptwdm`). */
  .withdraw {
    gap: var(--space-14);
    max-inline-size: 820px;
  }
  .withdraw > h2 {
    font-size: 19px;
  }
  .small {
    font-size: 13px;
  }
  .used-by {
    margin: 0;
    padding-inline-start: 18px;
    font-size: 13px;
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
  }
  .used-by > :global(li) {
    display: flex;
    gap: var(--space-04);
    align-items: center;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    font-weight: 700;
  }
  .field :is(textarea, select) {
    inline-size: 100%;
    font-weight: 400;
  }
  .danger {
    align-self: flex-start;
  }
</style>
