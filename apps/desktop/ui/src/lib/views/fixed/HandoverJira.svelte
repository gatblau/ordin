<script lang="ts">
  // The handover draft's Jira place (ADR-106 step 4): **Which issue?** as a single choice; below it the
  // line the Handover Service sent, drawn as text. The screens add no Jira sentence of their own.
  //
  // spec: ordin-desktop/components/ScreensModule#ADR-106
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { DraftJiraView } from '../../service-api/types.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { HandoverJiraController } from './jira.svelte';

  interface Props {
    backend: BackendState;
    draftId: string;
    jira: DraftJiraView;
    /** Opens the Jira issue dialog of step 2 for the draft's workspace. */
    onuseexisting?: () => void;
  }
  let { backend, draftId, jira, onuseexisting }: Props = $props();

  const c = new HandoverJiraController(backend.handle, draftId, jira);
  const view = $derived(c.jira ?? jira);
  const selected = $derived(view.selection.kind === 'linked' ? view.selection.key : '__new__');
</script>

<fieldset class="handover-jira">
  <legend>{t('handover-jira-issue-label')}</legend>
  {#each view.options as option (option.key)}
    <label>
      <input type="radio" name="jira-issue" value={option.key} checked={selected === option.key} onchange={() => void c.choose({ kind: 'linked', key: option.key })} />
      <span class="icon-tile"><Icon name="task" /></span>
      {option.key} {option.summary}
    </label>
  {/each}
  <label>
    <input type="radio" name="jira-issue" value="__new__" checked={selected === '__new__'} onchange={() => void c.choose({ kind: 'create_new' })} />
    {t('handover-jira-issue-new', { project: view.project })}
  </label>
  {#if view.options.length === 0}
    <button type="button" onclick={() => onuseexisting?.()}>{t('handover-jira-use-existing')}</button>
  {/if}
  {#if c.line !== ''}<p class="jira-line">{c.line}</p>{/if}
  <ErrorLine error={c.error} />
</fieldset>
