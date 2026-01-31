<script lang="ts">
  // Open in [IDE] (11 §5.11, UX-A25; amended January 2026 (user decision)): for a repository, or one file of
  // it, one action per IDE found on this computer and ticked in Settings › Code › *Open code in*. One IDE
  // is one button; two or more are items of the row's More menu. Nothing is drawn when none is ticked. It writes
  // nothing to the conversation; a short notice confirms it.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SW-7
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { Ide } from '../../service-api/types.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { appOrNull } from '../fixed/util';
  import { OpenInIdeModel } from './open-in-ide.svelte';

  interface Props {
    backend: BackendState;
    projectId: string;
    /** A file of the repository, or `null` for the repository itself. */
    path?: string | null;
    /** What is opened, named in the notice: the repository's or the file's name. */
    name: string;
    model?: OpenInIdeModel;
  }
  let { backend, projectId, path = null, name, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new OpenInIdeModel(backend.handle);
  const app = appOrNull();
  const offered = $derived(answers(backend, 'projects.ides') && answers(backend, 'projects.open_in_ide'));
  let menuOpen = $state(false);
  const menuId = `open-in-ide-${Math.random().toString(36).slice(2, 10)}`;

  onMount(() => {
    if (offered && given === undefined) void model.load();
  });

  async function open(ide: Ide): Promise<void> {
    menuOpen = false;
    const notice = t('open-in-ide-opened', { name, ide: ide.name });
    if (await model.open(ide, projectId, path, notice)) app?.shell.announce(notice);
  }
</script>

{#if offered && model.ides.length > 0}
  <span class="open-in-ide">
    {#if model.ides.length === 1}
      {@const ide = model.ides[0]}
      {#if ide !== undefined}
        <button type="button" class="small" onclick={() => void open(ide)}>{t('open-in-ide', { ide: ide.name })}</button>
      {/if}
    {:else}
      <button
        type="button"
        class="small"
        aria-haspopup="menu"
        aria-expanded={menuOpen}
        aria-controls={menuId}
        aria-label={t('open-in-ide-menu')}
        onclick={() => (menuOpen = !menuOpen)}
        onkeydown={(e) => {
          if (e.key === 'Escape') menuOpen = false;
        }}><Icon name="more" /></button
      >
      {#if menuOpen}
        <ul id={menuId} class="tree-menu-list" role="menu" aria-label={t('open-in-ide-menu')}>
          {#each model.ides as ide (ide.id)}
            <li role="none">
              <button
                type="button"
                role="menuitem"
                onclick={() => void open(ide)}
                onkeydown={(e) => {
                  if (e.key === 'Escape') menuOpen = false;
                }}>{t('open-in-ide', { ide: ide.name })}</button
              >
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
    <span class="notice" role="status" aria-live="polite">{model.notice}</span>
    <ErrorLine error={model.error} />
  </span>
{/if}

<style>
  .open-in-ide {
    position: relative;
    display: inline-flex;
    flex-wrap: wrap;
    gap: var(--space-06);
    align-items: center;
  }
  .notice {
    font-size: 12px;
    color: var(--color-text-muted);
  }
</style>
