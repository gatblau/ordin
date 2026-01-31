<script lang="ts">
  // Settings › Servers: each server with its state, Connect to a server…, Disconnect this computer…,
  // Remove this server… (a question first: what is forgotten, and that a new code is needed to come back),
  // Languages… (a connected personal server) and a personal server's update. A line when there is no
  // server; Connect again… opens the form under that server's row (amended January 2026 (user decision)).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-CU-7
  // spec: ordin-desktop/components/ScreensModule#SM-CU-8
  // spec: ordin-desktop/components/ScreensModule#SM-SW-12
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import { formatTime } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import { getApp } from '../../shell/context';
  import { makeKey } from '../../shell/screens';
  import { openScreen } from '../../shell/open-item';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import Notes from './Notes.svelte';
  import { ServersModel } from './servers-model.svelte';

  interface Props {
    backend: BackendState;
    model?: ServersModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const app = given === undefined ? getApp() : null;
  // svelte-ignore state_referenced_locally
  const model = given ?? new ServersModel(backend.handle, (id) => app?.stateFor(id));
  let notes = $state<{ version: string; notes: string } | null>(null);

  onMount(() => {
    void model.load();
    // After `server_state_changed` for a server: read the list and that server's update again (SM-UP-5).
    return backend.subscribe((env) => {
      if (env.kind === 'server_state_changed') void model.onServerStateChanged(env.server_connection_id);
    });
  });

  function openLanguages(server: { backend?: string | null; display_name: string }): void {
    if (app === null || server.backend === undefined || server.backend === null) return;
    openScreen(app, app.local.id, null, makeKey('server-languages', server.backend), { title: server.display_name });
  }
  const confirmingDisconnect = $derived(model.servers.find((s) => s.server_connection_id === model.confirmDisconnect));
  const confirmingRemove = $derived(model.servers.find((s) => s.server_connection_id === model.confirmRemove));
  const confirmingUpdate = $derived(model.servers.find((s) => s.server_connection_id === model.confirmUpdate));
  const updateOf = $derived(confirmingUpdate === undefined ? null : model.line(confirmingUpdate.server_connection_id));
</script>

<section class="servers settings-section" aria-labelledby="settings-servers-heading">
  <h3 id="settings-servers-heading">{t('settings-section-servers')}</h3>
  <ErrorLine error={model.error} onretry={() => void model.load()} />
  {#if model.servers.length === 0}
    <p class="intro">{t('server-none')}</p>
  {:else}
    <ul class="list-rows">
      {#each model.servers as server (server.server_connection_id)}
        {@const line = model.line(server.server_connection_id)}
        {@const rowError = model.rowErrors[server.server_connection_id]}
        <li class="list-row">
          <span class="grow">
            <b>{server.display_name}</b>
            <small>
              {#if server.state === 'connected'}
                {t('server-state-connected')}
              {:else if server.state === 'not_connected'}
                {server.as_of ? t('rail-server-not-connected', { time: formatTime(server.as_of) }) : t('server-state-not-connected')}
              {:else if server.state === 'refused_for_version'}
                {t(server.older_side === 'desktop' ? 'server-version-desktop-older' : 'server-version-server-older', { server: server.display_name })}
              {:else if server.state === 'disconnected'}
                {t('server-ended', { server: server.display_name })}
              {/if}
            </small>
            {#if line.kind === 'available'}
              <span class="update">
                {t('server-update-available', { server: server.display_name, version: line.update.version })}
                {#if line.selfUpdate}
                  <button type="button" class="small" onclick={() => (model.confirmUpdate = server.server_connection_id)}>{t('server-update-button')}</button>
                  <button type="button" class="small" onclick={() => (notes = { version: line.update.version, notes: line.update.notes })}>{t('server-update-notes')}</button>
                {:else}
                  {t('server-update-self-unavailable')}
                {/if}
              </span>
            {:else if line.kind === 'started'}
              <span class="update">{t('server-update-started', { server: server.display_name })}</span>
            {/if}
          </span>
          <span class="row-actions">
            {#if server.state === 'disconnected'}
              <button type="button" class="small" onclick={() => model.openConnect(server.server_connection_id)}>{t('server-connect-again')}</button>
            {/if}
            {#if server.state === 'connected' && server.backend && answers(backend, 'localisation.list_languages') && app?.stateFor(server.backend)?.hello?.profile === 'personal_server'}
              <button type="button" class="small" onclick={() => openLanguages(server)}>{t('server-languages')}</button>
            {/if}
            <button type="button" class="small" onclick={() => (model.confirmDisconnect = server.server_connection_id)}>{t('server-disconnect')}</button>
            <button type="button" class="small" onclick={() => (model.confirmRemove = server.server_connection_id)}>{t('server-remove')}</button>
          </span>
          {#if rowError !== undefined}
            <div class="row-error">
              <ErrorLine
                error={rowError.error}
                onretry={rowError.retry ? () => model.reopenUpdate(server) : undefined}
              />
            </div>
          {/if}
          {#if model.connecting && model.connectingFor === server.server_connection_id}
            {@render connectForm(server.display_name)}
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  <div class="polite" role="status" aria-live="polite">
    {#if model.announcement !== ''}{t('server-update-started', { server: model.announcement })}{:else}{model.done}{/if}
  </div>

  {#if model.connecting && model.connectingFor === null}
    {@render connectForm(null)}
  {:else if !model.connecting}
    <p><button type="button" onclick={() => model.openConnect()}>{t('server-connect')}</button></p>
  {/if}

  {#if notes !== null}
    <aside class="side-panel" aria-label={t('server-update-notes')}>
      <h4>{t('server-update-notes')}</h4>
      <Notes notes={notes.notes} />
      <button type="button" onclick={() => (notes = null)}>{t('card-close')}</button>
    </aside>
  {/if}
</section>

{#snippet connectForm(name: string | null)}
  <form
    class="connect field-stack"
    aria-label={name === null ? t('server-connect') : t('server-connect-again-title', { server: name })}
    onsubmit={(e) => {
      e.preventDefault();
      void model.connect();
    }}
  >
    {#if name !== null}<h4>{t('server-connect-again-title', { server: name })}</h4>{/if}
    <label>
      {t('server-address')}
      <!-- svelte-ignore a11y_autofocus -->
      <input type="text" bind:value={model.address} autocomplete="off" aria-describedby="server-address-hint" autofocus />
    </label>
    <p id="server-address-hint" class="hint">{t('server-address-hint')} {t('server-paste-line-hint')}</p>
    <label>{t('server-code')} <input type="password" bind:value={model.code} autocomplete="off" /></label>
    <div class="row-actions">
      <button type="submit" data-default="true" disabled={model.busy}>{t('server-connect-verb')}</button>
      <button type="button" onclick={() => model.cancelConnect()}>{t('confirm-cancel')}</button>
    </div>
    <ErrorLine error={model.connectError} />
  </form>
{/snippet}

<style>
  .servers {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .servers .list-row {
    flex-wrap: wrap;
    align-items: center;
  }
  .servers .list-row > :is(.row-error, form) {
    flex-basis: 100%;
  }
  .update {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-06);
    align-items: center;
    margin-block-start: var(--space-04);
    font-size: 13px;
  }
</style>

{#if confirmingDisconnect !== undefined}
  <ConfirmDialog
    title={t('server-disconnect-title', { server: confirmingDisconnect.display_name })}
    body={t('server-disconnect-body')}
    verb={t('server-disconnect')}
    initialFocus="cancel"
    onconfirm={() => void model.disconnect(confirmingDisconnect.server_connection_id)}
    oncancel={() => (model.confirmDisconnect = null)}
  />
{/if}
{#if confirmingRemove !== undefined}
  <ConfirmDialog
    title={t('server-remove-title', { server: confirmingRemove.display_name })}
    body={t('server-remove-body', { server: confirmingRemove.display_name })}
    verb={t('server-remove-verb')}
    initialFocus="cancel"
    onconfirm={() => void model.remove(confirmingRemove.server_connection_id, t('server-removed', { server: confirmingRemove.display_name }))}
    oncancel={() => (model.confirmRemove = null)}
  />
{/if}
{#if confirmingUpdate !== undefined && updateOf !== null && updateOf.kind === 'available'}
  <ConfirmDialog
    title={t('server-update-confirm-title', { server: confirmingUpdate.display_name, version: updateOf.update.version })}
    body={t('server-update-confirm-body', { server: confirmingUpdate.display_name })}
    verb={t('server-update-confirm-verb')}
    initialFocus="cancel"
    onconfirm={() => void model.updateServer(confirmingUpdate)}
    oncancel={() => (model.confirmUpdate = null)}
  />
{/if}
