<!--
  The icon part (ScreensModule.md amendment *Icons*, SM-IC-2; cross-cutting/VisualDesign.md V8). It
  draws one symbol of the window's sprite, hidden from assistive technology and never in the tab order:
  the words beside it carry the meaning. Stroke, size and direction live under `.icon` in tokens.css.
-->
<script lang="ts">
  // spec: ordin-desktop/components/ScreensModule#SM-IC-2
  import type { IconName } from './sprite';

  type IconSize = 'default' | 'small' | 'twist' | 'mark' | 'needs-tab' | 'close' | 'heading';

  let { name, size = 'default' }: { name: IconName; size?: IconSize } = $props();
</script>

<svg class="icon icon-{size}" aria-hidden="true" focusable="false"><use href="#i-{name}" /></svg>
