<script lang="ts">
  // A fixed screen that is a titled list of rows with an optional action per row. Text only. Each row
  // draws the list's icon in a tile before its words when one is given (V8 *Where icons are drawn*).
  import type { Snippet } from 'svelte';

  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { IconName } from '../../icons/sprite';
  import type { ServiceError } from '../../service-api/types.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';

  export interface Row {
    id: string;
    title: string;
    detail?: string | undefined;
  }
  interface Props {
    heading: string;
    rows: Row[];
    error?: ServiceError | null;
    backend: BackendState;
    onopen?: (row: Row) => void;
    onretry?: () => void;
    /** The icon each row draws before its words, and its tile colour (V8 *Icon tile colour*). */
    icon?: IconName | undefined;
    tint?: string | undefined;
    /** The message id of the line drawn when there is no row (default `fixed-nothing-here`, SM-EM-1). */
    emptyId?: string;
    children?: Snippet;
  }
  let { heading, rows, error = null, backend, onopen, onretry, icon, tint, emptyId = 'fixed-nothing-here', children }: Props = $props();
  const headingId = `fixed-list-${Math.random().toString(36).slice(2, 8)}`;
</script>

<section class="fixed-list" aria-labelledby={headingId}>
  <h2 id={headingId} tabindex="-1" data-region-focus>{heading}</h2>
  <ErrorLine {error} {onretry} />
  <ul>
    {#each rows as row (row.id)}
      <li>
        {#if icon}<span class="icon-tile" data-tint={tint}><Icon name={icon} /></span>{/if}
        {#if onopen}
          <button type="button" class="link" onclick={() => onopen(row)}>{row.title}</button>
        {:else}
          {row.title}
        {/if}
        {#if row.detail}<span class="secondary"> · {row.detail}</span>{/if}
      </li>
    {/each}
  </ul>
  {#if rows.length === 0 && error === null}<p class="secondary">{t(emptyId)}</p>{/if}
  {@render children?.()}
</section>

<style>
  ul {
    padding: 0;
  }
  li {
    list-style: none;
    padding-block: var(--space-04);
  }
  .secondary {
    color: var(--color-text-muted);
  }
  .icon-tile {
    vertical-align: middle;
    margin-inline-end: var(--space-04);
  }
  button.link {
    border: none;
    background: none;
    color: var(--color-accent);
    text-decoration: underline;
    padding: 0;
  }
</style>
