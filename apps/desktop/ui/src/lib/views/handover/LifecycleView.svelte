<script lang="ts">
  // The lifecycle as a path of stages (SM-NS-9): number, name, role, how many handovers, and "You are here"
  // on the stage the workspace follows; under each stage its agent, what it hands over and its handovers,
  // each opening `handover:<id>`.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-9
  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { stateMessageId } from './handover-controller.svelte';
  import type { LifecycleController } from './lifecycle-controller.svelte';

  interface Props {
    backend: BackendState;
    c: LifecycleController;
    onopen?: (handoverId: string) => void;
  }
  let { backend, c, onopen }: Props = $props();
</script>

<section class="lifecycle" aria-labelledby="lifecycle-heading">
  {#if c.stage === null}
    <h2 id="lifecycle-heading" tabindex="-1" data-region-focus>{t('lifecycle-none')}</h2>
  {:else if c.view !== null}
    <h2 id="lifecycle-heading" tabindex="-1" data-region-focus>{t('lifecycle-title', { name: c.view.lifecycle.name })}</h2>
    <p class="note">{t('lifecycle-readable-note')}</p>
    <ol class="path">
      {#each c.view.stages as line, i (i)}
        <li aria-current={line.you_are_here ? 'step' : undefined}>
          <h3>{i + 1}. {line.stage.name}</h3>
          <p class="small">{line.stage.role} · {t('lifecycle-handover-count', { count: line.handovers.length })}</p>
          {#if line.you_are_here}<p class="here"><strong>{t('lifecycle-you-are-here')}</strong></p>{/if}
          {#if line.stage.agent}<p class="small">{t('lifecycle-stage-agent', { agent: line.stage.agent })}</p>{/if}
          {#if line.stage.hands_over}<p class="small">{t('lifecycle-stage-hands-over', { what: line.stage.hands_over })}</p>{/if}
          <ul class="handovers">
            {#each line.handovers as handover (handover.id)}
              <li>
                <button type="button" class="link" onclick={() => onopen?.(handover.id)}>{handover.title}</button>
                · {handover.other_person} · {t(stateMessageId(handover.state), { name: handover.other_person })}
              </li>
            {/each}
          </ul>
        </li>
      {/each}
    </ol>
  {/if}
  <ErrorLine error={c.error} onretry={() => void c.load()} />
</section>

<style>
  /* The prototype's lifecycle screen (scene `hoLifecycle`, `.holc`, `.hostage`). */
  .lifecycle {
    gap: var(--space-12);
  }
  .lifecycle > h2 {
    font-size: 19px;
  }
  .small {
    font-size: 13px;
  }
  .note {
    font-size: 13px;
    font-weight: 700;
  }
  .path {
    list-style: none;
    margin: 0;
    padding: 0 2px var(--space-04) 0;
    display: grid;
    grid-template-columns: repeat(3, minmax(190px, 1fr));
    gap: var(--space-12);
    overflow-x: auto;
  }
  .path > li {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-list);
    background: var(--color-surface);
    padding: var(--space-10) var(--space-12);
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
    min-inline-size: 0;
  }
  .path > li[aria-current='step'] {
    border-color: var(--color-accent);
    box-shadow: 0 0 0 1px var(--color-accent);
  }
  .path h3 {
    font-size: 15px;
  }
  .here {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--color-accent);
  }
  .handovers {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .handovers > li {
    border-block-start: 1px solid var(--color-line);
    padding-block-start: var(--space-06);
    font-size: 13px;
  }
  .handovers button.link {
    display: block;
    color: var(--color-text);
    text-decoration: none;
    font-weight: 700;
    text-align: start;
  }
</style>
