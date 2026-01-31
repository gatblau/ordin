<script lang="ts">
  // **Record…** from the Minutes row's menu (ScreensModule.md SM-MIN-20): one dialog. With OBS ready it is
  // the reminder of SM-MIN-5 (on every press; **Start recording** sends `minutes.record`). Otherwise it says
  // what is wrong with OBS (SM-MIN-6) with the control that fixes it, and **Try again** reads the status
  // again; once OBS is ready the same dialog becomes the reminder.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-20
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-5
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-6
  import { t } from '../../i18n/fluent';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import type { MinutesScreenModel } from './minutes-screen.svelte';
  import ObsPasswordField from './ObsPasswordField.svelte';

  interface Props {
    model: MinutesScreenModel;
  }
  let { model }: Props = $props();
  const recorder = $derived(model.status?.recorder ?? null);
</script>

{#if recorder?.state === 'ready'}
  <ConfirmDialog
    title={t('minutes-record-title')}
    body={t('minutes-record-body')}
    verb={t('minutes-record-start')}
    destructive={false}
    onconfirm={() => void model.startRecording()}
    oncancel={() => (model.askingToRecord = false)}
  >
    {#if !recorder.others_heard}<p data-testid="minutes-record-mic-only">{t('minutes-record-mic-only')}</p>{/if}
  </ConfirmDialog>
{:else}
  <ConfirmDialog
    title={t('minutes-obs-not-ready-title')}
    verb={t('minutes-retry')}
    destructive={false}
    onconfirm={() => void model.readStatus()}
    oncancel={() => (model.askingToRecord = false)}
  >
    <div class="recorder" data-testid="minutes-recorder" data-state={recorder?.state ?? 'unknown'}>
      {#if recorder?.state === 'not_installed'}
        <p>{t('error-obs-not-installed')}</p>
        <button type="button" data-testid="minutes-get-obs" onclick={() => void model.getObs()}>{t('minutes-get-obs')}</button>
      {:else if recorder?.state === 'not_reachable'}
        <p>{t('error-obs-not-reachable')}</p>
        <button type="button" data-testid="minutes-open-obs" onclick={() => void model.openObs()}>{t('minutes-open-obs')}</button>
      {:else if recorder?.state === 'password_needed' || recorder?.state === 'password_refused'}
        <p>{t(recorder.state === 'password_needed' ? 'error-obs-password-needed' : 'error-obs-password-refused')}</p>
        <ObsPasswordField onsave={(p) => model.savePassword(p)} disabled={model.busy} />
      {:else if recorder?.state === 'version_unsupported'}
        <p>{t('error-obs-version-unsupported')}</p>
      {/if}
      <ErrorLine error={model.error ?? model.actionError} />
    </div>
  </ConfirmDialog>
{/if}

<style>
  .recorder {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-sm);
  }
</style>
