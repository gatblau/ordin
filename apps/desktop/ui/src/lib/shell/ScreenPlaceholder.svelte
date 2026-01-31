<script lang="ts">
  // Stands in for a fixed screen whose module has not registered it yet, so the shell compiles and runs.
  import { t } from '../i18n/fluent';

  interface Props {
    titleId: string;
  }
  let { titleId }: Props = $props();
</script>

<section class="screen-placeholder" aria-labelledby="placeholder-heading">
  <h2 id="placeholder-heading" tabindex="-1" data-region-focus>{t(titleId)}</h2>
</section>
