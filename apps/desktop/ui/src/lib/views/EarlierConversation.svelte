<script lang="ts">
  // One earlier conversation, read-only on its own tab, drawn with the whole chain it continues (SM-CC-6),
  // with Ask Ordin about it and Continue this conversation (SM-CC-2). The live conversation offers Continue
  // nowhere. Delete… removes it after asking (SM-WR-17). Text is drawn as text. **Ask Ordin about it** draws `chat` (V8 *Actions*); Continue has no icon.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-WR-17
  import { onMount } from 'svelte';

  import { answers } from '../backend/groups';
  import { formatDate } from '../format/intl';
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import { getApp } from '../shell/context';
  import { ScreenAreaState } from '../shell/screen-area.svelte';
  import { makeKey } from '../shell/screens';
  import type { ScreenProps } from '../shell/screens';
  import ConfirmDialog from './ConfirmDialog.svelte';
  import ContinueDialog from './ContinueDialog.svelte';
  import ErrorLine from './ErrorLine.svelte';
  import { ContinueFlow, deleteConversation, EarlierReadModel } from './earlier.svelte';
  import type { ServiceError } from '../service-api/types.gen';

  interface Props extends ScreenProps {
    model?: EarlierReadModel;
    flow?: ContinueFlow;
  }
  let { backend, workspaceId, instance, model: given, flow: givenFlow }: Props = $props();
  // svelte-ignore state_referenced_locally
  const app = given === undefined ? getApp() : null;
  // svelte-ignore state_referenced_locally
  const id = instance ?? '';
  // svelte-ignore state_referenced_locally
  const model = given ?? new EarlierReadModel(backend.handle, workspaceId ?? '', id);
  const head = $derived(model.parts[model.parts.length - 1]?.conversation);
  const nameOf = (): string => head?.first_request ?? t('conv-picker-new');
  // svelte-ignore state_referenced_locally
  const flow =
    givenFlow ??
    new ContinueFlow(backend.handle, {
      workspaceId: workspaceId ?? '',
      liveMessages: () => backend.conversation.get(workspaceId ?? '')?.length ?? 0,
      nameOf,
      announceText: (name) => t('conv-continued-announce', { name }),
      onContinued: (_conversationId, text) => {
        if (app === null) return;
        // The read-only tab closes; the workspace's conversation shows the new conversation.
        app.screens.close(ScreenAreaState.scope(backend.id, workspaceId), makeKey('earlier-conversation', id));
        app.shell.openWorkspaceTab(backend.id, workspaceId ?? '');
        void backend.reloadConversation(workspaceId ?? '');
        app.shell.afterContinue();
        app.shell.announce(text);
      },
    });
  let heading = $state<HTMLElement | null>(null);

  // SM-WR-17: Delete… asks first; on success the tab closes.
  const canDelete = $derived(answers(backend, 'ordin.delete_conversation'));
  let confirmingDelete = $state(false);
  let deleteError = $state<ServiceError | null>(null);

  async function confirmDelete(): Promise<void> {
    const refused = await deleteConversation(backend.handle, id);
    if (refused !== null) {
      deleteError = refused;
      return;
    }
    confirmingDelete = false;
    if (app === null) return;
    app.shell.announce(t('conv-deleted', { name: nameOf() }));
    app.screens.closeScreensFor('conversation', id);
  }

  onMount(() => {
    heading?.focus();
    void model.load();
    return backend.subscribe((env) => {
      if (env.kind === 'run_state_changed') flow.onRunStateChanged();
    });
  });
</script>

<section class="earlier-conversation" aria-labelledby="earlier-read-heading">
  <h2 id="earlier-read-heading" bind:this={heading} tabindex="-1" data-region-focus>{nameOf()}</h2>
  <div class="actions">
    <button type="button" onclick={() => (model.askOpen = !model.askOpen)}><Icon name="chat" />{t('conv-ask-about')}</button>
    {#if flow.waitsForCrew}
      <button type="button" disabled>{t('conv-continue')}</button>
      <span>{t('conv-continue-waits')}</span>
    {:else}
      <button type="button" disabled={flow.busy} onclick={() => void flow.request(id)}>{t('conv-continue')}</button>
    {/if}
    {#if canDelete}
      <button type="button" onclick={() => { deleteError = null; confirmingDelete = true; }}><Icon name="remove" />{t('conv-delete')}</button>
    {/if}
  </div>
  {#if model.askOpen}
    <form
      onsubmit={(e) => {
        e.preventDefault();
        void model.ask();
      }}
    >
      <label>{t('conv-ask-about')} <input type="text" bind:value={model.question} autocomplete="off" /></label>
      <button type="submit" data-default="true">{t('conv-send')}</button>
    </form>
  {/if}
  <ErrorLine error={flow.error ?? model.error} />

  {#each model.parts as part, i (part.conversation.conversation_id)}
    {#if i > 0}<p class="chain-from">{t('conv-continued-from', { time: formatDate(part.conversation.started_at, { dateStyle: 'medium', timeStyle: 'short' }) })}</p>{/if}
    <ol class="messages">
      {#each part.messages as m (m.seq)}
        <li data-author={m.author}>
          {#if m.block_id && model.moved.has(m.block_id)}
            <span class="moved">{t('conv-item-moved')}</span>
          {:else}
            <span class="text">{m.text}</span>
          {/if}
          <time datetime={m.at}>{formatDate(m.at, { timeStyle: 'short' })}</time>
        </li>
      {/each}
    </ol>
  {/each}
  {#if model.hasMore}<button type="button" onclick={() => void model.showMore()}>{t('settings-show-more')}</button>{/if}
</section>

{#if confirmingDelete}
  <ConfirmDialog title={t('conv-delete-title', { name: nameOf() })} body={t('conv-delete-body')} verb={t('conv-delete-verb')} onconfirm={() => void confirmDelete()} oncancel={() => (confirmingDelete = false)}>
    <ErrorLine error={deleteError} />
  </ConfirmDialog>
{/if}

{#if flow.asking !== null}
  <ContinueDialog name={nameOf()} onkeep={() => void flow.keepPointsFirst()} oncontinue={() => void flow.confirm()} oncancel={() => flow.cancel()} />
{/if}
