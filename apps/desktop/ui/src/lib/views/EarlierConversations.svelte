<script lang="ts">
  // Earlier conversations (ADR-108): each earlier conversation's first request, date and number of
  // messages, one entry per chain. One opens read-only on its own tab (EarlierConversation). Each entry
  // draws `chat` before its words (V8 *Lists*, an earlier conversation).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-WR-16
  import { onMount } from 'svelte';

  import { answers } from '../backend/groups';
  import { formatDate } from '../format/intl';
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import { getApp } from '../shell/context';
  import { openScreen } from '../shell/open-item';
  import { makeKey } from '../shell/screens';
  import type { ScreenProps } from '../shell/screens';
  import ConfirmDialog from './ConfirmDialog.svelte';
  import ContinueDialog from './ContinueDialog.svelte';
  import ErrorLine from './ErrorLine.svelte';
  import { ContinueFlow, deleteConversation, EarlierListModel, EarlierReadModel } from './earlier.svelte';
  import type { ChainEntry } from './earlier.svelte';
  import type { ServiceError } from '../service-api/types.gen';

  interface Props extends ScreenProps {
    model?: EarlierListModel;
  }
  let { backend, workspaceId, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const app = given === undefined ? getApp() : null;
  // svelte-ignore state_referenced_locally
  const model = given ?? new EarlierListModel(backend.handle, workspaceId ?? '');
  const name = (first: string | null | undefined): string => first ?? t('conv-picker-new');

  const entryName = (entry: ChainEntry): string => name(entry.chain[0]?.first_request ?? entry.head.first_request);
  const nameById = (id: string): string => {
    const entry = model.entries.find((e) => e.head.conversation_id === id);
    return entry === undefined ? name(null) : entryName(entry);
  };

  // Continue this conversation straight from the row's menu (SM-CC-2 to SM-CC-5).
  // svelte-ignore state_referenced_locally
  const flow = new ContinueFlow(backend.handle, {
    workspaceId: workspaceId ?? '',
    liveMessages: () => backend.conversation.get(workspaceId ?? '')?.length ?? 0,
    nameOf: nameById,
    announceText: (n) => t('conv-continued-announce', { name: n }),
    onContinued: (_conversationId, text) => {
      if (app === null) return;
      app.shell.openWorkspaceTab(backend.id, workspaceId ?? '');
      void backend.reloadConversation(workspaceId ?? '');
      app.shell.afterContinue();
      app.shell.announce(text);
    },
  });

  // Ask Ordin about it, from the row's menu: a question box under that row.
  let asking = $state<EarlierReadModel | null>(null);

  function askAbout(entry: ChainEntry): void {
    menuFor = null;
    const m = new EarlierReadModel(backend.handle, workspaceId ?? '', entry.head.conversation_id);
    m.askOpen = true;
    asking = m;
  }

  function continueIt(entry: ChainEntry): void {
    menuFor = null;
    void flow.request(entry.head.conversation_id);
  }

  onMount(() => {
    void model.load();
    return backend.subscribe((env) => {
      if (env.kind === 'run_state_changed') flow.onRunStateChanged();
    });
  });

  // SM-WR-16: each row's ⋯ menu (also opened by right-click) holds Ask Ordin about it, Continue this
  // conversation and Delete…, which asks first.
  const canDelete = $derived(answers(backend, 'ordin.delete_conversation'));
  let menuFor = $state<string | null>(null);
  let deleting = $state<ChainEntry | null>(null);
  let deleteError = $state<ServiceError | null>(null);

  function askDelete(entry: ChainEntry): void {
    menuFor = null;
    deleteError = null;
    deleting = entry;
  }

  async function confirmDelete(): Promise<void> {
    const entry = deleting;
    if (entry === null) return;
    const refused = await deleteConversation(backend.handle, entry.head.conversation_id);
    if (refused !== null) {
      deleteError = refused;
      return;
    }
    deleting = null;
    // SM-OS-3: the deleted conversations' read-only tabs close.
    for (const c of [entry.head, ...entry.chain]) app?.screens.closeScreensFor('conversation', c.conversation_id);
    app?.shell.announce(t('conv-deleted', { name: name(entry.chain[0]?.first_request ?? entry.head.first_request) }));
    await model.load();
  }

  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }

  function menuKey(event: KeyboardEvent): void {
    if (event.key !== 'Escape') return;
    event.preventDefault();
    menuFor = null;
  }

  function read(id: string, title: string): void {
    if (app !== null) openScreen(app, backend.id, workspaceId, makeKey('earlier-conversation', id), { title });
  }
</script>

<section class="earlier" aria-labelledby="earlier-heading">
  <h2 id="earlier-heading" tabindex="-1" data-region-focus>{t('conv-earlier-name')}</h2>
  <ErrorLine error={model.error} onretry={() => void model.load()} />
  <ErrorLine error={flow.error} />
  <ul>
    {#each model.entries as entry (entry.head.conversation_id)}
      <li>
        <div class="tree-line">
          <button type="button" class="row" onclick={() => read(entry.head.conversation_id, name(entry.head.first_request))} oncontextmenu={(e) => { e.preventDefault(); menuFor = entry.head.conversation_id; }}>
            <span class="icon-tile"><Icon name="chat" /></span>
            <span class="first">{entryName(entry)}</span>
            <span class="meta">{t('conv-picker-date', { date: formatDate(entry.head.started_at, { dateStyle: 'medium' }), count: entry.messageCount })}</span>
          </button>
          <span class="tree-actions" class:shown={menuFor === entry.head.conversation_id}>
            <button type="button" class="tree-action" aria-haspopup="menu" aria-expanded={menuFor === entry.head.conversation_id} aria-label={t('more-actions-for', { name: entryName(entry) })} title={t('more-actions-tip')} onclick={() => (menuFor = menuFor === entry.head.conversation_id ? null : entry.head.conversation_id)}><Icon name="more" /></button>
          </span>
          {#if menuFor === entry.head.conversation_id}
            <div role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('tree-node-menu', { name: entryName(entry) })} onkeydown={menuKey} use:focusFirst>
              <button type="button" role="menuitem" onclick={() => askAbout(entry)}><Icon name="chat" />{t('conv-ask-about')}</button>
              <button type="button" role="menuitem" disabled={flow.waitsForCrew || flow.busy} title={flow.waitsForCrew ? t('conv-continue-waits') : undefined} onclick={() => continueIt(entry)}>{t('conv-continue')}</button>
              {#if canDelete}<button type="button" role="menuitem" class="danger" onclick={() => askDelete(entry)}><Icon name="remove" />{t('conv-delete')}</button>{/if}
            </div>
          {/if}
        </div>
        {#if asking !== null && asking.conversationId === entry.head.conversation_id}
          {@const ask = asking}
          <form
            class="ask"
            onsubmit={async (e) => {
              e.preventDefault();
              if (await ask.ask()) asking = null;
            }}
          >
            <label>{t('conv-ask-about')} <input type="text" bind:value={ask.question} autocomplete="off" /></label>
            <button type="submit" data-default="true">{t('conv-send')}</button>
            <button type="button" onclick={() => (asking = null)}>{t('edit-cancel')}</button>
            <ErrorLine error={ask.error} />
          </form>
        {/if}
      </li>
    {/each}
  </ul>
  {#if model.next !== null}<button type="button" onclick={() => void model.more()}>{t('settings-show-more')}</button>{/if}
</section>

{#if deleting !== null}
  <ConfirmDialog
    title={t('conv-delete-title', { name: name(deleting.chain[0]?.first_request ?? deleting.head.first_request) })}
    body={t('conv-delete-body')}
    verb={t('conv-delete-verb')}
    onconfirm={() => void confirmDelete()}
    oncancel={() => (deleting = null)}
  >
    <ErrorLine error={deleteError} />
  </ConfirmDialog>
{/if}

{#if flow.asking !== null}
  <ContinueDialog name={nameById(flow.asking)} oncontinue={() => void flow.confirm()} oncancel={() => flow.cancel()} />
{/if}

<style>
  /* One row per conversation, drawn as the documents list's rows (scene `docs`, `table.files`):
     Small text, a divider under each row and under the list's top edge, the row's own words in body colour. */
  .earlier ul {
    list-style: none;
    margin: 0;
    padding: 0;
    border-block-start: 1px solid var(--color-line);
    font-size: 13px;
  }
  .earlier li .row {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    inline-size: 100%;
    min-height: 0;
    padding: 5px var(--space-06);
    border: 0;
    border-block-end: 1px solid var(--color-line);
    border-radius: 0;
    background: none;
    color: var(--color-text);
    font-size: 13px;
    text-align: start;
  }
  .earlier .first {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .earlier .meta {
    flex: none;
  }
  .earlier .ask {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-04);
    padding: var(--space-04) var(--space-06);
    border-block-end: 1px solid var(--color-line);
  }
</style>
