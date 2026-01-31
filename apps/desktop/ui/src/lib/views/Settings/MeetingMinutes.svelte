<script lang="ts">
  // Settings › Meeting minutes (ScreensModule.md SM-MIN-9), local backend only: **Speech model** (recommended
  // for this computer, larger or smaller — SM-MIN-12 — and the state of the model in use with **Remove the
  // speech model**), **Recordings** (delete after the
  // minutes are saved, off by default) and **OBS** (the port, 1024–65535, and the password field of SM-MIN-6).
  // Each change sends one `minutes.change_settings`; its answer replaces the view.
  //
  // `minutes.status` takes a workspace (SVC-MIN-1); the section reads it for the open workspace, else the
  // first active one, and draws only its heading while there is none (plan 07 FU-018).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-9
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-12
  import { onMount } from 'svelte';

  import type { ServiceError } from '../../backend/backend';
  import { t } from '../../i18n/fluent';
  import type { MinutesSettingsChange, MinutesStatus, SpeechModelSetting } from '../../service-api/minutes.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { appOrNull } from '../fixed/util';
  import ObsPasswordField from '../minutes/ObsPasswordField.svelte';

  interface Props {
    backend: BackendState;
    /** The workspace to read the status for (the tests); else the open one, else the first. */
    workspaceId?: string | null;
  }
  let { backend, workspaceId = null }: Props = $props();
  const app = appOrNull();

  let status = $state.raw<MinutesStatus | null>(null);
  let error = $state<ServiceError | null>(null);
  let portError = $state<ServiceError | null>(null);
  let port = $state('');
  let busy = $state(false);

  function workspace(): string | null {
    if (workspaceId !== null) return workspaceId;
    if (app !== null && app.shell.openBackend === backend.id && app.shell.openWorkspace !== null) return app.shell.openWorkspace;
    return backend.workspaces[0]?.id ?? null;
  }

  function show(next: MinutesStatus): void {
    status = next;
    port = String(next.settings.obs_port);
  }

  async function load(): Promise<void> {
    const ws = workspace();
    if (ws === null) return;
    const out = await backend.handle.call('minutes.status', { workspace_id: ws });
    if (out.kind === 'error') {
      error = out.error;
      return;
    }
    error = null;
    show(out.result);
  }

  onMount(() => void load());

  async function change(input: MinutesSettingsChange): Promise<void> {
    busy = true;
    const out = await backend.handle.call('minutes.change_settings', input);
    busy = false;
    if (out.kind === 'error') {
      if (input.obs_port != null) portError = out.error;
      else error = out.error;
      return;
    }
    error = null;
    portError = null;
    show(out.result);
  }

  function changePort(): void {
    const value = Number(port);
    if (status !== null && value === status.settings.obs_port) return;
    void change({ obs_port: Number.isInteger(value) ? value : 0 });
  }

  async function removeModel(): Promise<void> {
    busy = true;
    const out = await backend.handle.call('minutes.remove_model', {});
    busy = false;
    if (out.kind === 'error') error = out.error;
    else await load();
  }

  async function savePassword(password: string): Promise<boolean> {
    const out = await backend.handle.call('minutes.save_obs_password', { password });
    if (out.kind === 'error') error = out.error;
    await load();
    return out.kind === 'ok';
  }

  // SM-MIN-12: three options in this order; the recommendation is the default.
  const CHOICES: readonly SpeechModelSetting[] = ['recommended', 'larger', 'smaller'];
  const LABELS: Record<SpeechModelSetting, string> = {
    recommended: 'minutes-model-recommended',
    larger: 'minutes-settings-model-larger',
    smaller: 'minutes-settings-model-smaller',
  };
</script>

<section class="meeting-minutes settings-section" aria-labelledby="settings-meeting-minutes-heading">
  <h3 id="settings-meeting-minutes-heading">{t('settings-section-meeting-minutes')}</h3>
  <ErrorLine {error} onretry={() => void load()} />
  {#if status !== null}
    <fieldset class="card" data-testid="minutes-settings-model">
      <legend>{t('minutes-settings-model')}</legend>
      {#each CHOICES as choice (choice)}
        <label><input type="radio" name="speech-model" value={choice} checked={status.settings.speech_model === choice} disabled={busy} onchange={() => void change({ speech_model: choice })} />{t(LABELS[choice])}</label>
        {#if choice === 'recommended'}
          <p class="secondary" data-testid="minutes-settings-recommended-line">{t('minutes-model-recommended-line', { model: t(`minutes-model-name-${status.recommended_model}`) })}</p>
        {/if}
      {/each}
      {#if status.settings.speech_model === 'larger' && status.recommended_model === 'smaller'}
        <p data-testid="minutes-settings-larger-slow">{t('minutes-model-larger-slow')}</p>
      {/if}
      {#if status.model.present}
        <p>{t('minutes-settings-model-downloaded')}</p>
        <button type="button" data-testid="minutes-settings-remove-model" disabled={busy} onclick={() => void removeModel()}>{t('minutes-settings-remove-model')}</button>
      {:else}
        <p>{t('minutes-settings-model-not-downloaded')}</p>
      {/if}
    </fieldset>
    <fieldset class="card" data-testid="minutes-settings-recordings">
      <legend>{t('minutes-settings-recordings')}</legend>
      <label><input type="checkbox" role="switch" checked={status.settings.delete_recordings_after_save} disabled={busy} onchange={(e) => void change({ delete_recordings_after_save: e.currentTarget.checked })} />{t('minutes-settings-delete-after')}</label>
      <p class="secondary">{t('minutes-settings-recordings-kept')}</p>
    </fieldset>
    <fieldset class="card" data-testid="minutes-settings-obs">
      <legend>{t('minutes-settings-obs')}</legend>
      <label>{t('minutes-settings-obs-port')}<input type="text" inputmode="numeric" data-testid="minutes-settings-port" bind:value={port} disabled={busy} onchange={changePort} /></label>
      <ErrorLine error={portError} />
      <ObsPasswordField onsave={savePassword} disabled={busy} />
    </fieldset>
  {/if}
</section>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-card);
    padding: var(--space-md);
  }
</style>
