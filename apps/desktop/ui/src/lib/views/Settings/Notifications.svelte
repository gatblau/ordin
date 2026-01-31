<script lang="ts">
  // Settings › Notifications: the line, then one switch per setting (SM-NS-1.7).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-1
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { NotificationsModel } from './notifications-model.svelte';

  interface Props {
    backend: BackendState;
    model?: NotificationsModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new NotificationsModel(backend.handle);

  onMount(() => void model.load());
</script>

<section class="notifications" aria-labelledby="settings-notifications-heading">
  <h3 id="settings-notifications-heading">{t('settings-section-notifications')}</h3>
  <p>{t('settings-notify-intro')}</p>
  <ul>
    {#each model.rows as row (row.key)}
      <li data-key={row.key}>
        <label>
          <input type="checkbox" role="switch" checked={model.isOn(row.key)} disabled={model.locked(row.view)} onchange={(e) => void model.change(row.key, e.currentTarget.checked)} />
          {t(row.id)}
        </label>
        {#if model.locked(row.view)}<span class="tag">{t('settings-set-by-company')}</span>{/if}
      </li>
    {/each}
  </ul>
  <ErrorLine error={model.error} onretry={() => (model.error?.code === 'DSK_PREFERENCES_WRITE_FAILED' ? model.retry() : void model.load())} />
</section>
