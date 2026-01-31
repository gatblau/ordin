<script lang="ts">
  // Settings › Slack. The fixed sentence is always first; every post is shown to the person unless a rule
  // they set says otherwise (decision 82). A rule's line names its kind and state; Slack not offered says
  // why in one sentence; the own-app form labels its two boxes (amended January 2026 (user decision)).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-CU-9
  // spec: ordin-desktop/components/ScreensModule#SM-SW-10
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
  const REASON: Record<string, string> = {
    tools_turned_off: 'settings-slack-tools-off',
    no_slack_app: 'settings-slack-no-app',
    sign_in_not_offered_here: 'settings-slack-sign-in-not-here',
    app_not_approved: 'settings-slack-not-approved',
  };

  async function copy(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      model.copied = true;
    } catch {
      // The clipboard is not available: the description stays on screen to select.
    }
  }

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

<section class="slack settings-section" aria-labelledby="settings-slack-heading">
  <h3 id="settings-slack-heading">{t('settings-section-slack')}</h3>
  <p class="fixed">{t('settings-slack-fixed')}</p>
  {#if status !== null}
    {#if status.reason_not_offered}
      <p>{t(REASON[status.reason_not_offered] ?? 'settings-slack-not-approved')}</p>
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
          class="field-stack own-app"
          onsubmit={(e) => {
            e.preventDefault();
            void model.connectOwnApp();
          }}
        >
          <p>{t('settings-slack-own-app-steps')}</p>
          {#if status.manifest}
            <label>
              {t('settings-slack-description')}
              <textarea readonly rows="8">{status.manifest}</textarea>
            </label>
            <button type="button" class="small" onclick={() => void copy(status.manifest ?? '')}>{t('settings-slack-copy')}</button>
            <p class="hint" role="status" aria-live="polite">{model.copied ? t('settings-slack-copied') : ''}</p>
          {/if}
          <label>
            {t('settings-slack-client-id')}
            <input type="text" bind:value={model.clientId} autocomplete="off" />
          </label>
          <div class="row-actions">
            <button type="submit" data-default="true">{t('settings-slack-connect')}</button>
            <button type="button" onclick={() => (model.ownAppOpen = false)}>{t('confirm-cancel')}</button>
          </div>
        </form>
      {:else}
        <div><button type="button" class="small" onclick={() => (model.ownAppOpen = true)}>{t('settings-slack-own-app')}</button></div>
      {/if}
    {/if}
    {#if !status.rules_allowed}
      <p>{t('settings-slack-rules-off-by-company')}</p>
    {:else if model.orderedRules.length > 0}
      <h4>{t('settings-slack-rules')}</h4>
      <ul class="rules">
        {#each model.orderedRules as view (view.rule.id)}
          <li class="setting-row">
            <span class="grow">{t('settings-slack-rule-line', { kind: t(`settings-slack-rule-kind-${view.rule.kind.replaceAll('_', '-')}`), channel: view.rule.channel_name, cap: view.rule.daily_cap, state: t(`settings-slack-rule-state-${view.rule.state}`) })}</span>
            {#if view.rule.state === 'on'}
              <button type="button" class="small" onclick={() => void model.turnOff(view.rule.id)}>{t('settings-slack-rule-off')}</button>
            {/if}
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
  /* A sub-heading is smaller than the section heading (15 px). */
  .slack > h4 {
    margin-block-start: var(--space-sm);
    font-size: 13px;
    font-weight: 600;
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
  .own-app textarea {
    inline-size: 100%;
  }
</style>
