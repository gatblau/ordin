<script lang="ts">
  // Settings › Email (11 §5.22, UX-A44): the fixed sentence, the mailbox, the signature, the footer line,
  // how often to check and the email links of each workspace. A change is sent when the control changes.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-1
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { EmailModel, SIGNATURE_MAX } from './email-model.svelte';
  import MailSignInDialog from './MailSignInDialog.svelte';
  import { reloadOnSignIn } from './signins-model.svelte';

  interface Props {
    backend: BackendState;
    model?: EmailModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new EmailModel(backend.handle);
  const settings = $derived(model.settings);

  onMount(() => {
    void model.load();
    return reloadOnSignIn(backend, () => model.load());
  });
</script>

<section class="email" aria-labelledby="settings-email-heading">
  <h3 id="settings-email-heading">{t('settings-section-email')}</h3>
  <p class="fixed">{t('settings-email-fixed')}</p>
  {#if model.mailbox !== null}
    <h4>{t('settings-email-mailbox')}</h4>
    <p class="mailbox">
      <span class="account">{model.mailbox.account_label}</span>
      {t(model.passwordKind ? 'settings-email-signed-in-password' : 'settings-email-signed-in-account')}
    </p>
    {#if model.passwordKind}
      <button type="button" onclick={() => model.signIns.openMailDialog(model.mailbox ?? { kind: 'mail_server_password', account_label: '' })}>{t('settings-email-change-password')}</button>
    {/if}
    <button type="button" onclick={() => void model.signOut()}>{t('settings-email-sign-out')}</button>
    {#if model.check === 'refused'}<p role="alert">{t('settings-email-check-refused')}</p>{/if}
    {#if model.check === 'not_reachable_securely'}<p role="alert">{t('settings-email-check-not-secure')}</p>{/if}
  {/if}
  <MailSignInDialog model={model.signIns} backend={backend.handle} onsaved={() => void model.load()} />

  {#if settings !== null}
    <p>
      <label>
        <input type="checkbox" role="switch" checked={settings.add_signature} onchange={(e) => void model.setAddSignature(e.currentTarget.checked)} />
        {t('settings-email-add-signature')}
      </label>
    </p>
    <label>
      {t('settings-email-signature')}
      <textarea rows="4" maxlength={SIGNATURE_MAX} bind:value={model.signature} onblur={() => void model.saveSignature()}></textarea>
    </label>
    <p>
      <label>
        <input
          type="checkbox"
          role="switch"
          checked={settings.agent_foot_line || settings.agent_foot_line_locked}
          disabled={settings.agent_foot_line_locked}
          onchange={(e) => void model.setFootLine(e.currentTarget.checked)}
        />
        {t('settings-email-foot-line')}
      </label>
      {#if settings.agent_foot_line_locked}<span class="tag">{t('settings-email-foot-line-required')}</span>{/if}
    </p>
    <fieldset>
      <legend>{t('settings-email-check')}</legend>
      <label>
        <input type="radio" name="email-check" value="every15_minutes" checked={settings.check === 'every15_minutes'} onchange={() => void model.setCheck('every15_minutes')} />
        {t('settings-email-check-every-15')}
      </label>
      <label>
        <input type="radio" name="email-check" value="only_on_workspace_open" checked={settings.check === 'only_on_workspace_open'} onchange={() => void model.setCheck('only_on_workspace_open')} />
        {t('settings-email-check-on-open')}
      </label>
    </fieldset>
  {/if}

  {#if model.workspaces.length > 0}
    <h4>{t('settings-email-workspaces')}</h4>
    <ul class="workspace-links">
      {#each model.workspaces as workspace (workspace.id)}
        <li data-workspace={workspace.id}>
          <span class="workspace">{workspace.name}</span>
          <ul>
            {#each workspace.links as link (link.id)}<li>{link.display_name}</li>{/each}
          </ul>
        </li>
      {/each}
    </ul>
  {/if}
  <ErrorLine error={model.error} onretry={() => void model.load()} />
</section>
