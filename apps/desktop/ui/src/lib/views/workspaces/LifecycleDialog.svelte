<script lang="ts">
  // The confirmation of Remove workspace…, Delete now… and Empty Bin… (SM-WS-6, SM-WS-8), drawn where the
  // action was chosen. The removing verb is never the default button; a refused verb keeps the dialog open
  // with its error line.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-WS-6
  // spec: ordin-desktop/components/ScreensModule#SM-WS-8
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { askingWords, type LifecyclePlace, type WorkspaceLifecycle } from './lifecycle.svelte';

  interface Props {
    model: WorkspaceLifecycle;
    place: LifecyclePlace;
  }
  let { model, place }: Props = $props();

  const asking = $derived(model.asking !== null && model.asking.from === place ? model.asking : null);
  const words = $derived(asking === null ? null : askingWords(asking));
</script>

{#if asking !== null && words !== null}
  <ConfirmDialog title={words.title} body={words.body} verb={words.verb} onconfirm={() => void model.confirm()} oncancel={() => model.cancel()}>
    {#if words.extra !== null}<p class="dialog-body">{words.extra}</p>{/if}
    <ErrorLine error={model.askError} onretry={() => void model.confirm()} />
  </ConfirmDialog>
{/if}
