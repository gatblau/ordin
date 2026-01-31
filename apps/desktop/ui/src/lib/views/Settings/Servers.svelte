<script lang="ts">
  // Settings › Servers: each server with its state, Connect to a server…, Disconnect this computer…,
  // Remove this server, Languages… (a connected personal server) and a personal server's update.
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
  const confirmingUpdate = $derived(model.servers.find((s) => s.server_connection_id === model.confirmUpdate));
  const updateOf = $derived(confirmingUpdate === undefined ? null : model.line(confirmingUpdate.server_connection_id));
</script>

<section class="servers" aria-labelledby="settings-servers-heading">
  <h3 id="settings-servers-heading">{t('settings-section-servers')}</h3>
  <ErrorLine error={model.error} onretry={() => void model.load()} />
  <ul>
    {#each model.servers as server (server.server_connection_id)}
      {@const line = model.line(server.server_connection_id)}
      {@const rowError = model.rowErrors[server.server_connection_id]}
      <li>
        <strong>{server.display_name}</strong>
        {#if server.state === 'not_connected'}
          <span>{t('rail-server-not-connected', { time: server.as_of ? formatTime(server.as_of) : '' })}</span>
        {:else if server.state === 'refused_for_version'}
          <span>{t(server.older_side === 'desktop' ? 'server-version-desktop-older' : 'server-version-server-older', { server: server.display_name })}</span>
        {:else if server.state === 'disconnected'}
          <span>{t('server-ended', { server: server.display_name })}</span>
          <button type="button" onclick={() => model.openConnect()}>{t('server-connect-again')}</button>
        {/if}
        {#if server.state === 'connected' && server.backend && answers(backend, 'localisation.list_languages') && app?.stateFor(server.backend)?.hello?.profile === 'personal_server'}
          <button type="button" onclick={() => openLanguages(server)}>{t('server-languages')}</button>
        {/if}
        <button type="button" onclick={() => (model.confirmDisconnect = server.server_connection_id)}>{t('server-disconnect')}</button>
        <button type="button" class="danger" onclick={() => void model.remove(server.server_connection_id)}>{t('server-remove')}</button>

        {#if line.kind === 'available'}
          <p class="update">
            {t('server-update-available', { server: server.display_name, version: line.update.version })}
            {#if line.selfUpdate}
              <button type="button" onclick={() => (model.confirmUpdate = server.server_connection_id)}>{t('server-update-button')}</button>
              <button type="button" onclick={() => (notes = { version: line.update.version, notes: line.update.notes })}>{t('server-update-notes')}</button>
            {/if}
            <!-- SPEC-GAP-3: the sentence of SETUP_SELF_UPDATE_UNAVAILABLE reaches the screens only inside a refusal
                 (the core's ErrorSentences); with self_update false there is no refusal and no catalogue id for it. -->
          </p>
        {:else if line.kind === 'started'}
          <p class="update">{t('server-update-started', { server: server.display_name })}</p>
        {/if}
        {#if rowError !== undefined}
          <ErrorLine
            error={rowError.error}
            onretry={rowError.retry ? () => model.reopenUpdate(server) : undefined}
          />
        {/if}
      </li>
    {/each}
  </ul>

  <div class="polite" role="status" aria-live="polite">
    {#if model.announcement !== ''}{t('server-update-started', { server: model.announcement })}{/if}
  </div>

  {#if model.connecting}
    <form
      class="connect"
      onsubmit={(e) => {
        e.preventDefault();
        void model.connect();
      }}
    >
      <label>{t('server-address')} <input type="text" bind:value={model.address} autocomplete="off" /></label>
      <label>{t('server-code')} <input type="password" bind:value={model.code} autocomplete="off" /></label>
      <button type="submit" data-default="true" disabled={model.busy}>{t('server-connect-verb')}</button>
      <button type="button" onclick={() => model.cancelConnect()}>{t('confirm-cancel')}</button>
      <ErrorLine error={model.connectError} />
    </form>
  {:else}
    <button type="button" onclick={() => model.openConnect()}>{t('server-connect')}</button>
  {/if}

  {#if notes !== null}
    <aside class="side-panel" aria-label={t('server-update-notes')}>
      <h4>{t('server-update-notes')}</h4>
      <Notes notes={notes.notes} />
      <button type="button" onclick={() => (notes = null)}>{t('card-close')}</button>
    </aside>
  {/if}
</section>

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
