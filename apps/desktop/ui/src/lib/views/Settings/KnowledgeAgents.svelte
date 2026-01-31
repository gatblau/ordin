<script lang="ts">
  // Settings › Knowledge agents: the intro, a workspace picker, one row per connection with Details and
  // Remove, and Connect a knowledge agent… A workspace with no connection says so in one sentence; with no
  // workspace at all, one line says to open or make one and nothing else is drawn. The connect dialog keeps
  // Tab inside it, closes on Escape and puts its verb before Cancel, as the confirmation pattern does
  // (amended January 2026 (user decision)).
  import { onMount } from 'svelte';

  import { formatDate } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import { getApp } from '../../shell/context';
  import { focusableIn, trapIndex } from '../../shell/focus';
  import { openScreen } from '../../shell/open-item';
  import { SCREEN_IDS } from '../../shell/screens';
  import type { BackendState } from '../../store/backend-state.svelte';
  import type { SourcesRecordPolicy } from '../../service-api/types.gen';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { accountLine, KnowledgeAgentsModel } from './knowledge-agents.svelte';

  interface Props {
    backend: BackendState;
    workspaceId?: string | null;
    model?: KnowledgeAgentsModel;
  }
  let { backend, workspaceId = null, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const app = given === undefined ? getApp() : null;
  // svelte-ignore state_referenced_locally
  const model =
    given ??
    new KnowledgeAgentsModel(backend.handle, workspaceId ?? app?.shell.openWorkspace ?? backend.workspaces[0]?.id ?? '', (x) => app?.shell.announce(x), app?.diagnostics);
  const record = (policy: SourcesRecordPolicy | null | undefined): string | null =>
    policy === null || policy === undefined ? null : policy.enabled ? t('knowledge-agents-record-on', { days: policy.retention_days }) : t('knowledge-agents-record-off');
  const addressErrorId = 'knowledge-agents-address-error';

  onMount(() => {
    void model.load();
    return backend.subscribe((env) => {
      if (env.kind === 'knowledge_agents_changed' && env.workspace_id === model.workspaceId) void model.load();
    });
  });

  function signInAt(): void {
    if (app !== null) openScreen(app, app.local.id, null, SCREEN_IDS.settings);
  }

  let dialog = $state<HTMLElement | null>(null);
  /** The connect dialog: Escape closes it, Tab stays inside it (ConfirmDialog's handling). */
  function ondialogkey(event: KeyboardEvent): void {
    if (!model.connectOpen) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      model.closeConnect();
    } else if (event.key === 'Tab' && dialog !== null) {
      const items = focusableIn(dialog);
      const next = trapIndex(items.indexOf(document.activeElement as HTMLElement), items.length, event.shiftKey);
      if (next >= 0) {
        event.preventDefault();
        items[next]?.focus();
      }
    }
  }
</script>

<section class="knowledge-agents settings-section" aria-labelledby="settings-knowledge-agents-heading">
  <h3 id="settings-knowledge-agents-heading">{t('settings-section-knowledge-agents')}</h3>
  <p class="intro">{t('knowledge-agents-intro')}</p>
  {#if model.workspaceId === ''}
    <p>{t('knowledge-agents-no-workspace')}</p>
  {:else}
    <label class="workspace-pick">
      {t('knowledge-agents-workspace')}
      <select value={model.workspaceId} onchange={(e) => void model.chooseWorkspace(e.currentTarget.value)}>
        {#each backend.workspaces as ws (ws.id)}<option value={ws.id}>{ws.name}</option>{/each}
      </select>
    </label>
    <ErrorLine error={model.error} onretry={() => void model.load()} />

    {#if model.connections.length === 0}
      <p>{t('knowledge-agents-none')}</p>
    {:else}
      <ul class="list-rows">
        {#each model.connections as connection (connection.connection_id)}
          <li class="list-row">
            <span class="grow">
              <b>{connection.name}</b>
              <small>
                {#if connection.added_by === 'company'}<span class="tag">{t('knowledge-agents-set-by-company')}</span> · {/if}
                {#if connection.last_answered_at}{t('knowledge-agents-last-answered', { when: formatDate(connection.last_answered_at, { dateStyle: 'medium', timeStyle: 'short' }) })}{:else}{t('knowledge-agents-never-answered')}{/if}
                {#if record(connection.sources_record) !== null} · {record(connection.sources_record)}{/if}
              </small>
            </span>
            <span class="row-actions">
              <button type="button" class="small" onclick={() => void model.openDetails(connection.connection_id)}>{t('knowledge-agents-details')}</button>
              <button type="button" class="small" onclick={() => (model.confirmRemove = connection)}>{t('knowledge-agents-remove')}</button>
            </span>
          </li>
        {/each}
      </ul>
    {/if}
    <p><button type="button" onclick={() => model.openConnect()}>{t('knowledge-agents-connect')}</button></p>
  {/if}

  {#if model.details !== null}
    {@const d = model.details}
    <aside class="side-panel" aria-labelledby="ka-details-heading">
      <h4 id="ka-details-heading">{d.about.agent}</h4>
      <ErrorLine error={model.detailsError} />
      <p>{t('knowledge-agents-places', { count: d.about.repository_count })}</p>
      {#if d.about.as_of}<p>{t('knowledge-agents-as-of', { when: formatDate(d.about.as_of, { dateStyle: 'medium', timeStyle: 'short' }) })}</p>{/if}
      {#if record(d.about.sources_record) !== null}<p>{record(d.about.sources_record)}</p>{/if}
      <p class="address">{d.connection.url}</p>
      {#each d.accounts ?? [] as account, i (i)}
        {@const line = accountLine(account)}
        {#if line !== null}
          <p>
            {t(line.messageId, { system: line.system })}
            {#if line.signIn}<button type="button" onclick={signInAt}>{t('error-sign-in-again')}</button>{/if}
          </p>
        {/if}
      {/each}
      <button type="button" onclick={() => model.closeDetails()}>{t('card-close')}</button>
    </aside>
  {:else if model.detailsError !== null}
    <ErrorLine error={model.detailsError} />
  {/if}
</section>

<svelte:window onkeydown={ondialogkey} />

{#if model.connectOpen}
  <div class="dialog-backdrop">
    <div bind:this={dialog} class="dialog" role="dialog" aria-modal="true" aria-labelledby="ka-connect-heading">
      <h2 id="ka-connect-heading" tabindex="-1" data-region-focus>{t('knowledge-agents-connect-title')}</h2>
      {#if model.added === null}
        <form
          class="field-stack"
          onsubmit={(e) => {
            e.preventDefault();
            void model.connect();
          }}
        >
          <label>
            {t('knowledge-agents-address')}
            <input
              type="url"
              bind:value={model.address}
              autocomplete="off"
              aria-describedby={model.addressNotHttps ? addressErrorId : undefined}
              aria-invalid={model.addressNotHttps ? 'true' : undefined}
            />
          </label>
          <p class="hint">{t('knowledge-agents-address-help')}</p>
          {#if model.addressNotHttps}<p id={addressErrorId} class="invalid">{t('knowledge-agents-address-not-https')}</p>{/if}
          <ErrorLine error={model.connectError} onsignin={signInAt} />
          <div class="dialog-buttons">
            <button type="submit" class="go" disabled={model.busy}>{t('knowledge-agents-connect-verb')}</button>
            <button type="button" data-default="true" onclick={() => model.closeConnect()}>{t('confirm-cancel')}</button>
          </div>
        </form>
      {:else}
        <p><strong>{model.added.about.agent}</strong></p>
        {#if record(model.added.about.sources_record) !== null}<p>{record(model.added.about.sources_record)}</p>{/if}
        <div class="dialog-buttons">
          <button type="button" onclick={() => void model.removeJustAdded()}>{t('knowledge-agents-remove')}</button>
          <button type="button" data-default="true" onclick={() => model.keepIt()}>{t('knowledge-agents-keep-it')}</button>
        </div>
      {/if}
    </div>
  </div>
{/if}

{#if model.confirmRemove !== null}
  {@const target = model.confirmRemove}
  <ConfirmDialog
    title={t('knowledge-agents-remove-confirm', { name: target.name })}
    verb={t('knowledge-agents-remove')}
    initialFocus="cancel"
    onconfirm={() => void model.remove(target)}
    oncancel={() => (model.confirmRemove = null)}
  />
{/if}

<style>
  .knowledge-agents {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .workspace-pick {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-04);
    font-size: 13px;
  }
  .knowledge-agents .address {
    font-family: var(--font-mono);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
</style>
