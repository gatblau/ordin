<script lang="ts">
  // Settings › Slack. The fixed sentence is always first; every post is shown to the person unless a rule
  // they set says otherwise (decision 82).
  //
  // SPEC-GAP-5: the catalogue gives no words for a rule's kind or state (`settings-slack-rule-line` takes
  // `$kind` and `$state`); the rule line is drawn with those two arguments empty and only rules that can
  // be turned off are listed. **Sign out** and **Allow…** use the ids proposed under SPEC-GAP-7.
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { reloadOnSignIn } from './signins-model.svelte';
  import { SlackModel } from './slack-model.svelte';

  interface Props {
    backend: BackendState;
    model?: SlackModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new SlackModel(backend.handle);
  const status = $derived(model.status);

  onMount(() => {
    void model.load();
    const stopSignIn = reloadOnSignIn(backend, () => model.load());
    const stopWorkspace = backend.subscribe((env) => {
      if (env.kind === 'workspace_changed') void model.load();
    });
    return () => {
      stopSignIn();
      stopWorkspace();
    };
  });
</script>

<section class="slack" aria-labelledby="settings-slack-heading">
  <h3 id="settings-slack-heading">{t('settings-section-slack')}</h3>
  <p class="fixed">{t('settings-slack-fixed')}</p>
  {#if status !== null}
    {#if status.reason_not_offered === 'app_not_approved'}
      <p>{t('settings-slack-not-approved')}</p>
    {/if}
    {#if status.signed_in}
      <div class="setting-row">
        <span class="grow">{t('settings-slack-signed-in', { workspace: status.workspace_name ?? '', name: status.person_name ?? '' })}</span>
        <button type="button" class="small" onclick={() => void model.signOut()}>{t('settings-slack-sign-out')}</button>
      </div>
    {:else if status.offered}
      <div><button type="button" onclick={() => void model.signIn()}>{t('settings-slack-sign-in')}</button></div>
    {/if}
    {#if !status.profile_app && !status.signed_in}
      {#if model.ownAppOpen}
        <form
          onsubmit={(e) => {
            e.preventDefault();
            void model.connectOwnApp();
          }}
        >
          <p>{t('settings-slack-own-app-steps')}</p>
          {#if status.manifest}<textarea readonly rows="8">{status.manifest}</textarea>{/if}
          <input type="text" bind:value={model.clientId} aria-label={t('settings-slack-own-app-steps')} autocomplete="off" />
          <button type="submit" data-default="true">{t('settings-slack-own-app')}</button>
          <button type="button" onclick={() => (model.ownAppOpen = false)}>{t('confirm-cancel')}</button>
        </form>
      {:else}
        <div><button type="button" class="small" onclick={() => (model.ownAppOpen = true)}>{t('settings-slack-own-app')}</button></div>
      {/if}
    {/if}
    {#if !status.rules_allowed}
      <p>{t('settings-slack-rules-off-by-company')}</p>
    {:else if model.activeRules.length > 0}
      <h4>{t('settings-slack-rules')}</h4>
      <ul class="rules">
        {#each model.activeRules as view (view.rule.id)}
          <li class="setting-row">
            <span class="grow">{t('settings-slack-rule-line', { kind: '', channel: view.rule.channel_name, cap: view.rule.daily_cap, state: '' })}</span>
            <button type="button" class="small" onclick={() => void model.turnOff(view.rule.id)}>{t('settings-slack-rule-off')}</button>
          </li>
        {/each}
      </ul>
    {/if}
    {#if status.scope_needed}
      <p>{t('settings-slack-more-permissions')}</p>
      <div><button type="button" onclick={() => void model.signIn()}>{t('settings-slack-allow')}</button></div>
    {/if}
  {/if}
  <ErrorLine error={model.error} onretry={() => void model.load()} />
</section>

<style>
  /* Settings › Slack (scene `settingsSlack`): the fixed sentence in bold, then one bordered row per setting
     with its button at the end (the prototype's `.lrow`). */
  .slack {
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .slack > :is(h3, h4, p) {
    margin: 0;
  }
  .slack > h4 {
    margin-block-start: var(--space-sm);
    font-size: 17px;
  }
  .fixed {
    font-size: 14px;
    font-weight: 700;
  }
  .rules {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .setting-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-sm);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: var(--space-06) var(--space-10);
    font-size: 14px;
  }
  .setting-row .grow {
    flex: 1;
    min-inline-size: 160px;
  }
</style>
