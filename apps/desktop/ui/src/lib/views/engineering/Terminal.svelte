<script lang="ts">
  // The engineer's terminal screen (xterm.js, loaded on first use). On a server's workspace there are
  // no code views yet (`server-no-code-views`).
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { ScreenProps } from '../../shell/screens';
  import ErrorLine from '../ErrorLine.svelte';
  import { appOrNull } from '../fixed/util';
  import { readFromDocument, terminalTheme, watchTheme } from './code-theme';
  import { TerminalController } from './terminal-controller.svelte';
  import { loadXterm } from './xterm';

  let { backend, instance }: ScreenProps = $props();
  // The app context is read while the screen is set up; Svelte's getContext answers nothing in onMount, so
  // the terminal never started (plan 04 FU-005).
  const app = appOrNull();

  let host = $state<HTMLElement | null>(null);
  let controller = $state<TerminalController | null>(null);
  const noCode = $derived(backend.kind === 'server');
  let stop: (() => void) | null = null;

  onMount(() => {
    if (noCode) return;
    void start();
    return () => {
      stop?.();
      void controller?.close();
    };
  });

  async function start(): Promise<void> {
    if (app === null || host === null) return;
    const { Terminal } = await loadXterm();
    const term = new Terminal({
      cursorBlink: true,
      convertEol: false,
      // A link in terminal output comes from a command or an agent: never open it (secscan F2).
      linkHandler: { activate: () => {} },
    });
    term.open(host);
    const paint = (): void => {
      term.options.theme = terminalTheme(readFromDocument());
    };
    paint();
    const stopPaint = watchTheme(paint);
    const c = new TerminalController(backend.handle, app.streams, (bytes) => term.write(bytes));
    controller = c;
    const data = term.onData((text) => void c.input(text));
    const size = term.onResize(({ cols, rows }) => void c.resize(cols, rows));
    stop = () => {
      stopPaint();
      data.dispose();
      size.dispose();
      term.dispose();
    };
    await c.open(instance ?? '', term.cols, term.rows);
  }
</script>

<section class="terminal" aria-labelledby="terminal-heading">
  <h2 id="terminal-heading" tabindex="-1" data-region-focus>{t('engineering-terminal-title')}</h2>
  {#if noCode}
    <p>{t('server-no-code-views')}</p>
  {:else}
    <ErrorLine error={controller?.error ?? null} />
    <div class="terminal-host" bind:this={host}></div>
  {/if}
</section>

<style>
  .terminal-host {
    min-block-size: 240px;
  }
</style>
