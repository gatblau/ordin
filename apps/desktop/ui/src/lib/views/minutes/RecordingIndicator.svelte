<script lang="ts">
  // The recording indicator in the app header (ScreensModule.md SM-MIN-7): while a recording runs — from the
  // `minutes.record` answer to the `minutes.stop` answer — a red dot, "Recording — 12:04" updated each
  // second, and **Stop**. Announced to screen readers once at start and once at stop, never each second.
  // Drawn only while `recording.view` is set, which only a backend answering `minutes.record` sets (SM-UP-1).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-7
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { appOrNull } from '../fixed/util';
  import { elapsed } from './minutes-model';
  import { recording } from './recording.svelte';

  const app = appOrNull();
  let now = $state(Date.now());
  let shownId: string | null = null;

  $effect(() => {
    const view = recording.view;
    if (view !== null && shownId !== view.id) {
      shownId = view.id;
      app?.shell.announce(t('minutes-recording-started'));
    } else if (view === null && shownId !== null) {
      shownId = null;
      app?.shell.announce(t('minutes-recording-stopped'));
    }
    if (view === null) return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
</script>

{#if recording.view !== null}
  <div class="recording-indicator" data-testid="minutes-recording">
    <span class="dot" aria-hidden="true"></span>
    <span class="words">{t('minutes-recording', { time: elapsed(recording.view.started_at, now) })}</span>
    <button type="button" class="stop" data-testid="minutes-stop" disabled={recording.stopping} onclick={() => void recording.stop()}><Icon name="stop" />{t('minutes-stop')}</button>
    <ErrorLine error={recording.error} onretry={() => void recording.stop()} />
  </div>
{/if}

<style>
  .recording-indicator {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-04) var(--space-md);
    border-block-end: 1px solid var(--color-line);
  }
  .dot {
    inline-size: 10px;
    block-size: 10px;
    border-radius: 50%;
    background: var(--color-danger);
  }
  .words {
    font-variant-numeric: tabular-nums;
  }
</style>
