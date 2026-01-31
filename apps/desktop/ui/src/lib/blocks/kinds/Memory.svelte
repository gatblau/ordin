<script lang="ts">
  // `memory`: the facts with their sources. A fact an agent cited reads as the agent's claim (SM-SF-6). The
  // facts are read with `memory.get` in chunks of 100; an id it does not return (forgotten meanwhile) is not
  // drawn. Correcting, forgetting and remembering everywhere happen on the memory screen, opened with **Open**.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-6
  import { t } from '../../i18n/fluent';
  import type { AgentInstance, FactView, ServiceError } from '../../service-api/types.gen';
  import { getApp } from '../../shell/context';
  import { openScreen } from '../../shell/open-item';
  import { SCREEN_IDS } from '../../shell/screens';
  import Icon from '../../icons/Icon.svelte';
  import ErrorLine from '../../views/ErrorLine.svelte';
  import AskButton from '../AskButton.svelte';
  import FactList from '../FactList.svelte';
  import type { KindProps } from '../types';

  let { backend, workspaceId, block }: KindProps<'memory'> = $props();
  const app = getApp();

  let facts = $state<FactView[]>([]);
  let agents = $state<Pick<AgentInstance, 'id' | 'display_name'>[]>([]);
  let loaded = $state(false);
  let error = $state<ServiceError | null>(null);

  $effect(() => {
    const ids = block.facts;
    const workspace = block.workspace_id;
    loaded = false;
    if (!backend.groups.has('memory')) {
      loaded = true;
      return;
    }
    let current = true;
    void (async () => {
      const found: FactView[] = [];
      for (let at = 0; at < ids.length; at += 100) {
        const out = await backend.handle.call('memory.get', { ids: ids.slice(at, at + 100), workspace_id: workspace });
        if (!current) return;
        if (out.kind !== 'ok') {
          error = out.error;
          break;
        }
        found.push(...out.result.facts);
      }
      // In the order the block gives; an id not returned is simply not drawn.
      facts = ids.flatMap((id) => found.filter((f) => f.id === id));
      if (backend.groups.has('agents') && facts.some((f) => f.came_from.kind === 'agent_citing')) {
        const list = await backend.handle.call('agents.list', { workspace_id: workspace });
        if (current && list.kind === 'ok') agents = list.result.map((a) => a.instance);
      }
      if (current) loaded = true;
    })();
    return () => {
      current = false;
    };
  });
</script>

<div class="block-memory">
  {#if loaded && facts.length === 0 && error === null}<p>{t('block-cannot-show')}</p>{/if}
  <FactList {facts} {agents} />
  <div class="actions">
    <button type="button" onclick={() => openScreen(app, backend.id, workspaceId, SCREEN_IDS.memory)}>{t('card-open')}</button>
    <button type="button" onclick={() => openScreen(app, backend.id, workspaceId, SCREEN_IDS.knowledge)}><Icon name="know" />{t('block-open-knowledge')}</button>
    <AskButton {backend} {workspaceId} labelId="block-ask-about-this" text={t('block-ask-text-this', { title: t('tree-memory') })} />
  </div>
  <ErrorLine {error} />
</div>

<style>
  .block-memory {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
  }
</style>
