<script lang="ts">
  // Settings › Code: **Open code in** (one switch per IDE) and **Code checking by language** (one row per
  // language plugin). The language server's own settings are not drawn (G-CU-7).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-1
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { CodeModel, serverInUse } from './code-model.svelte';

  interface Props {
    backend: BackendState;
    model?: CodeModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new CodeModel(backend.handle);

  onMount(() => void model.load());
</script>

<section class="code" aria-labelledby="settings-code-heading">
  <h3 id="settings-code-heading">{t('settings-section-code')}</h3>
  <h4>{t('settings-code-open-in')}</h4>
  {#if model.ides.length === 0}
    <p>{t('settings-code-no-ide')}</p>
  {:else}
    <ul class="ides">
      {#each model.ides as ide (ide.id)}
        <li>
          <label>
            <input type="checkbox" role="switch" checked={ide.enabled} onchange={(e) => void model.setIde(ide, e.currentTarget.checked)} />
            {ide.name}
          </label>
        </li>
      {/each}
    </ul>
  {/if}
  <h4>{t('settings-code-languages')}</h4>
  <ul class="languages">
    {#each model.plugins as plugin (plugin.id)}
      <li data-language={plugin.language}>
        <span class="language">{plugin.language}</span>
        <span class="server">{serverInUse(plugin)}</span>
        {#if plugin.pinned_version}<span>{t('settings-code-pinned', { version: plugin.pinned_version })}</span>{/if}
        {#if plugin.source === 'organisation_profile'}
          <span class="tag">{t('settings-code-from-company')}</span>
        {:else}
          <span class="tag">{t('settings-code-from-agent')}</span>
        {/if}
        <label>
          <input type="checkbox" role="switch" checked={plugin.enabled} onchange={(e) => void model.configure(plugin, { enabled: e.currentTarget.checked })} />
          {t('settings-code-on')}
        </label>
        {#if plugin.approved_servers.length > 1}
          <select value={serverInUse(plugin)} aria-label={plugin.language} onchange={(e) => void model.configure(plugin, { chosen_server: e.currentTarget.value })}>
            {#each plugin.approved_servers as server (server)}<option value={server}>{server}</option>{/each}
          </select>
        {/if}
        {#if plugin.enabled}
          <button type="button" onclick={() => void model.restart(plugin)}>{t('settings-code-restart')}</button>
        {/if}
      </li>
    {/each}
  </ul>
  <ErrorLine error={model.error} onretry={() => void model.load()} />
</section>
