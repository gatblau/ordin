<script lang="ts">
  // Settings › Code: **Open code in** (one switch per IDE, with the line saying what ticking does) and
  // **Code checking by language** (one row per language plugin, each control named with its language; a
  // line when there is none; **Restart** says it restarted). The language server's own settings are not
  // drawn (G-CU-7). (amended January 2026 (user decision))
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

<section class="code settings-section" aria-labelledby="settings-code-heading">
  <h3 id="settings-code-heading">{t('settings-section-code')}</h3>
  <h4>{t('settings-code-open-in')}</h4>
  {#if model.ides.length === 0}
    <p>{t('settings-code-no-ide')}</p>
  {:else}
    <p class="intro">{t('settings-code-open-in-line')}</p>
    <ul class="list-rows ides">
      {#each model.ides as ide (ide.id)}
        <li>
          <label class="switch-line">
            <input type="checkbox" role="switch" checked={ide.enabled} onchange={(e) => void model.setIde(ide, e.currentTarget.checked)} />
            {ide.name}
          </label>
        </li>
      {/each}
    </ul>
  {/if}
  <h4>{t('settings-code-languages')}</h4>
  <p class="intro">{t('settings-code-languages-line')}</p>
  {#if model.plugins.length === 0}
    <p>{t('settings-code-no-languages')}</p>
  {/if}
  <ul class="list-rows languages">
    {#each model.plugins as plugin (plugin.id)}
      <li class="list-row" data-language={plugin.language}>
        <span class="grow">
          <b class="language">{plugin.language}</b>
          <small>
            <span class="server">{serverInUse(plugin)}</span>{#if plugin.pinned_version} · {t('settings-code-pinned', { version: plugin.pinned_version })}{/if} · <span class="tag">{t(plugin.source === 'organisation_profile' ? 'settings-code-from-company' : 'settings-code-from-agent')}</span>
          </small>
        </span>
        <span class="row-actions">
          <label class="switch-line">
            <input type="checkbox" role="switch" checked={plugin.enabled} aria-label={t('settings-code-on-for', { language: plugin.language })} onchange={(e) => void model.configure(plugin, { enabled: e.currentTarget.checked })} />
            {t('settings-code-on')}
          </label>
          {#if plugin.approved_servers.length > 1}
            <select value={serverInUse(plugin)} aria-label={t('settings-code-server-for', { language: plugin.language })} onchange={(e) => void model.configure(plugin, { chosen_server: e.currentTarget.value })}>
              {#each plugin.approved_servers as server (server)}<option value={server}>{server}</option>{/each}
            </select>
          {/if}
          {#if plugin.enabled}
            <button type="button" class="small" onclick={() => void model.restart(plugin, t('settings-code-restarted', { language: plugin.language }))}>{t('settings-code-restart')}</button>
          {/if}
        </span>
      </li>
    {/each}
  </ul>
  <p class="done-line" role="status" aria-live="polite">{model.done}</p>
  <ErrorLine error={model.error} onretry={() => void model.load()} />
</section>

<style>
  .code {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
</style>
