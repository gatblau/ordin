<script lang="ts">
  // The automatic-publishing line of a linked folder (SM-SF-7): plain text with an information icon
  // (`i-info`, V8) labelled by its sentence; never carried by colour alone; no switch.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-7
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { PublishingController } from './publishing.svelte';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    /** The `ConfluenceLink.id` of the folder. */
    linkId: string;
  }
  let { backend, workspaceId, linkId }: Props = $props();

  const c = new PublishingController(backend.handle, linkId);
  const revision = $derived(backend.workspaceRevision(workspaceId));
  let first = true;

  onMount(() => {
    void c.load();
  });
  $effect(() => {
    void revision;
    if (first) {
      first = false;
      return;
    }
    void c.load();
  });

  const sentence = $derived.by(() => {
    const line = c.line;
    if (line.kind === 'on') return t('confluence-auto-on');
    if (line.kind === 'off') {
      const names = line.more > 0 ? t('confluence-auto-off-more', { names: line.names, count: line.more }) : line.names;
      return t('confluence-auto-off', { names });
    }
    return '';
  });
</script>

{#if c.line.kind !== 'none'}
  <p class="publishing-line">
    <span class="icon-tile" role="img" aria-label={sentence}><Icon name="info" /></span>
    <span>{sentence}</span>
    {#if c.line.kind === 'off'}<span class="how">{t('confluence-auto-how-to-turn-on')}</span>{/if}
  </p>
{/if}
<ErrorLine error={c.error} />

<style>
  .publishing-line {
    margin: var(--space-sm) 0;
  }
  .icon-tile {
    vertical-align: middle;
  }
  .how {
    display: block;
    color: var(--color-text-muted);
  }
</style>
