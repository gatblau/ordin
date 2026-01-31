<script lang="ts">
  // One link between two things, read as words (ADR-111 step 4): "Borealis supplies packaging · since 8 Jan".
  // A live link has Correct and Forget; an ended link has no actions. A link set by a page names the page
  // by the title its view read; a page that is gone reads `memory-page-gone`, with no Open the page (SM-CU-11).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-CU-11
  import { formatDate } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type { LinkView } from '../../service-api/types.gen';
  import type { AgentNames } from './facts';
  import type { PageTitleMap } from './page-titles.svelte';

  interface Props {
    link: LinkView;
    names: AgentNames;
    oncorrect?: (link: LinkView) => void;
    onforget?: (link: LinkView) => void;
    onopenthing?: (id: string) => void;
    onopenpage?: (pageId: string) => void;
    /** Page titles by id, read by the view (`LinkBy::page` carries only the page id); `null` for a page that is gone. */
    pageTitles?: PageTitleMap;
  }
  let { link, names, oncorrect, onforget, onopenthing, onopenpage, pageTitles = {} }: Props = $props();

  const ended = $derived(link.until !== null && link.until !== undefined);
  const by = $derived.by(() => {
    const b = link.by;
    switch (b.kind) {
      case 'agent': {
        const agent = names.nameOf(b.agent_instance_id);
        return agent === null ? t('memory-remembered-by-ordin') : t('memory-remembered-by', { agent });
      }
      case 'ordin':
        return t('memory-remembered-by-ordin');
      case 'person':
        return t('memory-link-by-person');
      case 'page': {
        const title = pageTitles[b.page_id];
        if (title === null) return t('memory-page-gone');
        return title === undefined ? '' : t('memory-link-by-page', { page: title });
      }
      default:
        return '';
    }
  });
</script>

<li class="link-line">
  <p>
    {#if ended}
      {t('memory-link-line-ended', { from: link.from.name, kind: link.kind, to: link.to.name, since: formatDate(link.since), until: formatDate(link.until as string) })}
    {:else}
      {t('memory-link-line', { from: link.from.name, kind: link.kind, to: link.to.name, date: formatDate(link.since) })}
    {/if}
  </p>
  <p class="secondary things">
    <button type="button" class="link" onclick={() => onopenthing?.(link.from.id)}>{link.from.name}</button>
    <button type="button" class="link" onclick={() => onopenthing?.(link.to.id)}>{link.to.name}</button>
  </p>
  {#if by !== ''}<p class="secondary">{by}</p>{/if}
  {#if link.by.kind === 'page' && pageTitles[link.by.page_id] !== null && onopenpage}
    <button type="button" onclick={() => onopenpage((link.by as { page_id: string }).page_id)}>{t('memory-open-page')}</button>
  {/if}
  {#if !ended}
    {#if oncorrect}<button type="button" onclick={() => oncorrect(link)}>{t('memory-link-correct')}</button>{/if}
    {#if onforget}<button type="button" class="danger" onclick={() => onforget(link)}>{t('memory-link-forget')}</button>{/if}
  {/if}
</li>

<style>
  .link-line {
    list-style: none;
    padding: var(--space-04) 0;
  }
  p {
    margin: 0;
  }
  .secondary {
    color: var(--color-text-muted);
  }
  button.link {
    border: none;
    background: none;
    color: var(--color-accent);
    text-decoration: underline;
    padding: 0;
  }
</style>
