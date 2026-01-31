<script lang="ts">
  // The one **Publish to…** item of a document's menu (SM-PUB-1): pressed, it opens to one item per
  // destination the workspace is connected to. With no destination nothing is drawn.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-PUB-1
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import { PUBLISH_SYSTEMS, type PublishSystem } from './destinations';

  interface Props {
    destinations: readonly PublishSystem[];
    onchoose: (system: PublishSystem) => void;
  }
  let { destinations, onchoose }: Props = $props();
  let open = $state(false);
</script>

{#if destinations.length > 0}
  <button type="button" role="menuitem" aria-haspopup="menu" aria-expanded={open} onclick={() => (open = !open)}><Icon name="publish" />{t('publish-to')}</button>
  {#if open}
    <div role="menu" class="publish-to-list" aria-label={t('publish-to')}>
      {#each destinations as system (system)}
        <button type="button" role="menuitem" onclick={() => onchoose(system)}>{t('publish-to-destination', { destination: PUBLISH_SYSTEMS[system].name })}</button>
      {/each}
    </div>
  {/if}
{/if}

<style>
  .publish-to-list {
    display: flex;
    flex-direction: column;
    padding-inline-start: var(--space-md);
  }
</style>
