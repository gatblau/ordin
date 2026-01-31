<script lang="ts">
  // `key_figures`: 1 to 8 figures, each a label, a value with its unit or currency, and the change against a
  // named earlier value when `against` is present. A change is a word and a mark as well as a colour.
  import { formatMoney, formatNumber, formatPercent } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import AskButton from '../AskButton.svelte';
  import SourceLine from '../SourceLine.svelte';
  import type { KindProps } from '../types';
  import type { KeyFigure } from '../../service-api/types.gen';

  let { backend, workspaceId, block }: KindProps<'key_figures'> = $props();

  const value = (figure: KeyFigure): string => {
    const unit = figure.unit ?? null;
    if (unit === null || unit === '') return formatNumber(figure.value);
    if (/^[A-Z]{3}$/.test(unit)) {
      try {
        return formatMoney(figure.value, unit);
      } catch {
        return `${formatNumber(figure.value)} ${unit}`;
      }
    }
    return `${formatNumber(figure.value)} ${unit}`;
  };

  const change = (figure: KeyFigure): { mark: string; text: string } | null => {
    const c = figure.change;
    if (c === undefined || c === null) return null;
    if (c.earlier_value === 0 || figure.value === c.earlier_value) {
      return figure.value === c.earlier_value
        ? { mark: '=', text: t('block-figure-same', { against: c.against }) }
        : { mark: figure.value > c.earlier_value ? '▲' : '▼', text: t('block-figure-was', { against: c.against, value: formatNumber(c.earlier_value) }) };
    }
    const fraction = (figure.value - c.earlier_value) / Math.abs(c.earlier_value);
    return fraction > 0
      ? { mark: '▲', text: t('block-figure-up', { percent: formatPercent(fraction, 1), against: c.against }) }
      : { mark: '▼', text: t('block-figure-down', { percent: formatPercent(Math.abs(fraction), 1), against: c.against }) };
  };
</script>

<div class="block-key-figures">
  <ul class="figures">
    {#each block.figures as figure (figure.label)}
      {@const delta = change(figure)}
      <li>
        <span class="label">{figure.label}</span>
        <span class="value">{value(figure)}</span>
        {#if delta !== null}<span class="change"><span aria-hidden="true">{delta.mark}</span> {delta.text}</span>{/if}
      </li>
    {/each}
  </ul>
  <SourceLine source={block.source} />
  <AskButton {backend} {workspaceId} labelId="block-ask-about-this" text={t('block-ask-text-figures')} />
</div>

<style>
  .figures {
    list-style: none;
    padding: 0;
    display: grid;
    margin: 0;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: var(--space-sm);
  }
  li {
    display: flex;
    flex-direction: column;
    gap: var(--space-02);
    padding: var(--space-sm) 10px;
    background: var(--color-background);
    border-radius: var(--radius-md);
  }
  .label {
    font-size: 12px;
    color: var(--color-text-muted);
  }
  .value {
    font-size: 22px;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
  }
  .change {
    font-size: 12px;
    color: var(--color-text-secondary);
  }
  .block-key-figures {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    align-items: flex-start;
  }
  .figures {
    align-self: stretch;
  }
</style>
