<script lang="ts">
  // Settings › Email (11 §5.22, UX-A44): the fixed sentence, the mailbox (with **Sign in** when none is
  // signed in), the signature, the footer line, how often to check and the email links of each workspace,
  // with a line when none links email. A change is sent when the control changes; **Sign out of email** acts
  // at once and says what it did (amended January 2026 (user decision)).
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

<section class="email settings-section" aria-labelledby="settings-email-heading">
  <h3 id="settings-email-heading">{t('settings-section-email')}</h3>
  <p class="fixed"><b>{t('settings-email-fixed')}</b></p>
  <h4>{t('settings-email-mailbox')}</h4>
  <ul class="list-rows">
    <li class="list-row mailbox">
      {#if model.mailbox !== null}
        <span class="grow">
          <b class="account">{model.mailbox.account_label}</b>
          <small>{t(model.passwordKind ? 'settings-email-signed-in-password' : 'settings-email-signed-in-account')}</small>
        </span>
        <span class="row-actions">
          {#if model.passwordKind}
            <button type="button" class="small" onclick={() => model.signIns.openMailDialog(model.mailbox ?? { kind: 'mail_server_password', account_label: '' })}>{t('settings-email-change-password')}</button>
          {/if}
          <button type="button" class="small" onclick={() => void model.signOut(t(model.passwordKind ? 'settings-email-signed-out' : 'settings-email-signed-out-account'))}>{t('settings-email-sign-out')}</button>
        </span>
      {:else}
        <span class="grow">
          <b>{t('settings-email-mailbox')}</b>
          <small>{t('settings-email-not-signed-in')}</small>
        </span>
        <button type="button" class="small" onclick={() => void model.signIn()}>{t('signin-sign-in')}</button>
      {/if}
    </li>
  </ul>
  {#if model.check === 'refused'}<p role="alert">{t('settings-email-check-refused')}</p>{/if}
  {#if model.check === 'not_reachable_securely'}<p role="alert">{t('settings-email-check-not-secure')}</p>{/if}
  <p class="done-line" role="status" aria-live="polite">{model.done}</p>
  <MailSignInDialog model={model.signIns} backend={backend.handle} onsaved={() => void model.load()} />

  {#if settings !== null}
    <label class="switch-line">
      <input type="checkbox" role="switch" checked={settings.add_signature} onchange={(e) => void model.setAddSignature(e.currentTarget.checked)} />
      {t('settings-email-add-signature')}
    </label>
    <label class="signature">
      {t('settings-email-signature')}
      <textarea rows="4" maxlength={SIGNATURE_MAX} bind:value={model.signature} disabled={!settings.add_signature} aria-describedby="settings-email-signature-saved" onblur={() => void model.saveSignature()}></textarea>
    </label>
    <p id="settings-email-signature-saved" class="hint" role="status" aria-live="polite">{model.signatureSaved ? t('settings-saved') : ''}</p>
    <label class="switch-line">
      <input
        type="checkbox"
        role="switch"
        checked={settings.agent_foot_line || settings.agent_foot_line_locked}
        disabled={settings.agent_foot_line_locked}
        onchange={(e) => void model.setFootLine(e.currentTarget.checked)}
      />
      {t('settings-email-foot-line')}
      {#if settings.agent_foot_line_locked}<span class="tag">{t('settings-email-foot-line-required')}</span>{/if}
    </label>
    <fieldset class="choice-group">
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

  <h4>{t('settings-email-workspaces')}</h4>
  {#if model.workspaces.length > 0}
    <ul class="list-rows workspace-links">
      {#each model.workspaces as workspace (workspace.id)}
        <li class="list-row" data-workspace={workspace.id}>
          <span class="grow">
            <b class="workspace">{workspace.name}</b>
            <small>{workspace.links.map((l) => l.display_name).join(', ')}</small>
          </span>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="intro">{t('settings-email-workspaces-none')}</p>
  {/if}
  <ErrorLine error={model.error} onretry={() => void model.load()} />
</section>

<style>
  /* Settings › Email (the prototype's `drawSetMail`): the fixed sentence in bold, the mailbox as a row with
     its buttons at the end, the switches and the choice one per line, then the workspaces' rows. */
  .email {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .signature {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
    font-size: 13px;
  }
  .signature textarea {
    inline-size: 100%;
  }
</style>
