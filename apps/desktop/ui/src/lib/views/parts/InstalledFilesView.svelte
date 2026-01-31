<script lang="ts">
  // The installed files of an agent (SM-NS-11): one line per file, grouped by folder, with its size and
  // the first 12 characters of its checksum. No line opens anything.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-11
  import { formatNumber } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { groupByFolder, type InstalledFilesController } from './installed-files.svelte';

  interface Props {
    backend: BackendState;
    c: InstalledFilesController;
    /** The agent's name, held by *What [agent] is made of* and passed as the tab title. */
    agent: string;
  }
  let { backend, c, agent }: Props = $props();
  const groups = $derived(groupByFolder(c.files));
</script>

<section class="installed-files" aria-labelledby="installed-files-heading">
  <h2 id="installed-files-heading" tabindex="-1" data-region-focus>{t('installed-files-title', { agent })}</h2>
  <p class="quiet">{t('installed-files-note')}</p>
  <ErrorLine error={c.error} onretry={() => void c.load()} />
  {#each groups as group (group.folder)}
    <h3>{group.folder === '' ? '/' : group.folder}</h3>
    <ul class="list-rows">
      {#each group.files as file (file.path)}
        <li class="list-row">
          <span class="path">{file.path}</span>
          <span class="quiet">· {formatNumber(file.size, { style: 'unit', unit: 'byte', unitDisplay: 'narrow' })} · {file.sha256.slice(0, 12)}</span>
        </li>
      {/each}
    </ul>
  {/each}
</section>

<style>
  /* The prototype's installed files (scene `installedFiles`, `R.mofiles`): folders as small code
     headings, files as rows. */
  .installed-files {
    gap: var(--space-sm);
  }
  .installed-files > h2 {
    font-size: 19px;
  }
  .installed-files > h3 {
    font-family: var(--font-mono);
    font-size: 14px;
    margin-block-start: var(--space-sm);
  }
  .list-rows {
    margin: 0;
  }
  .list-row {
    font-size: 14px;
    flex-wrap: wrap;
  }
  .list-row > .path {
    font-family: var(--font-mono);
    font-weight: 700;
    font-size: 13px;
  }
  .list-row > .quiet {
    font-size: 13px;
  }
  .quiet {
    color: var(--color-text-muted);
  }
  .path {
    overflow-wrap: anywhere;
  }
</style>
