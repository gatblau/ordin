<script lang="ts">
  // Add to this workspace › Jira issue… (ADR-106 step 2). Desktop backend only.
  //
  // spec: ordin-desktop/components/ScreensModule#ADR-106
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { SourceLink } from '../../service-api/types.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { JiraAddController, jiraOffered } from './jira.svelte';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    onadded?: (link: SourceLink | undefined) => void;
    onclose: () => void;
    onopensettings?: () => void;
  }
  let { backend, workspaceId, onadded, onclose, onopensettings }: Props = $props();

  const c = new JiraAddController(backend.handle, workspaceId);
  const offered = jiraOffered(backend.handle);
</script>

{#if offered}
  <form
    class="jira-add"
    role="group"
    aria-labelledby="jira-add-title"
    onsubmit={(e) => {
      e.preventDefault();
      void c.add().then((ok) => {
        if (ok) onadded?.(c.link ?? undefined);
      });
    }}
  >
    <h3 id="jira-add-title" tabindex="-1">{t('jira-add-to-workspace')}</h3>
    <label>{t('jira-add-paste')}<input type="text" bind:value={c.pasted} /></label>
    <label>
      {t('jira-add-search')}
      <input type="search" value={c.text} oninput={(e) => c.type(e.currentTarget.value)} />
    </label>
    {#if c.searched && c.results.length === 0}<p>{t('jira-add-none')}</p>{/if}
    <ul class="jira-results" role="listbox" aria-label={t('jira-add-search')}>
      {#each c.results as issue (issue.id)}
        <li role="option" aria-selected={c.picked === issue.id}>
          <button type="button" onclick={() => (c.picked = issue.id)}><span class="icon-tile"><Icon name="task" /></span>{issue.issue.key} {issue.summary}</button>
        </li>
      {/each}
    </ul>
    {#if c.notConnected}
      <p>{t('jira-add-not-connected')}</p>
      <button type="button" onclick={() => onopensettings?.()}>{t('jira-open-settings')}</button>
    {/if}
    <ErrorLine error={c.error} />
    <button type="submit">{t('jira-add')}</button>
    <button type="button" onclick={onclose}>{t('confirm-cancel')}</button>
  </form>
{/if}
