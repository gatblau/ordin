<script lang="ts">
  // The OBS password field (ScreensModule.md SM-MIN-6, SM-MIN-9): masked, with **Save**. The password goes
  // to `minutes.save_obs_password` and is cleared from the field once sent; it is never drawn back.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-6
  import { t } from '../../i18n/fluent';

  interface Props {
    onsave: (password: string) => Promise<boolean>;
    disabled?: boolean;
  }
  let { onsave, disabled = false }: Props = $props();
  let password = $state('');

  async function save(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (password === '') return;
    const sent = password;
    password = '';
    await onsave(sent);
  }
</script>

<form class="obs-password" onsubmit={(e) => void save(e)}>
  <label>
    {t('minutes-obs-password')}
    <input type="password" autocomplete="off" data-testid="minutes-obs-password" bind:value={password} {disabled} />
  </label>
  <button type="submit" data-testid="minutes-obs-save" disabled={disabled || password === ''}>{t('minutes-obs-save')}</button>
</form>

<style>
  .obs-password {
    display: flex;
    align-items: end;
    gap: var(--space-sm);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
  }
</style>
