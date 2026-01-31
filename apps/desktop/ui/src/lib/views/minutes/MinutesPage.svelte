<script lang="ts">
  // A minutes page, `minutes:<minutes id>` (ScreensModule.md SM-MIN-8): the body in the Markdown editor under
  // its title and "Made from … on …". The header holds the icon buttons **Save** (after a change; a conflict
  // is shown with **Reopen**) and **Show transcript**, then the minutes menu (SM-MIN-13, SM-MIN-14): Export,
  // Delete recording… while there is one, and Remove…. Text from the meeting is drawn through text
  // interpolation only.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-8
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-14
  import { onMount } from 'svelte';

  import type { ServiceError } from '../../backend/backend';
  import MarkdownEditor from '../../editor/MarkdownEditor.svelte';
  import { formatDate } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { ScreenProps } from '../../shell/screens';
  import type { Minutes, Transcript } from '../../service-api/minutes.gen';
  import ErrorLine from '../ErrorLine.svelte';
  import { appOrNull } from '../fixed/util';
  import { elapsed } from './minutes-model';
  import MinutesMenu from './MinutesMenu.svelte';

  let { backend, instance }: ScreenProps = $props();
  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  const minutesId = instance ?? '';

  let minutes = $state.raw<Minutes | null>(null);
  let body = $state('');
  /** The body as last read or saved; Save is enabled only when the editor's text differs from it. */
  let saved = $state('');
  // The editor may give the text back with its blank lines normalised: only a change in words counts.
  const changed = $derived(minutes !== null && body.trim() !== saved.trim());
  let error = $state<ServiceError | null>(null);
  let actionError = $state<ServiceError | null>(null);
  let conflict = $state(false);
  let transcript = $state.raw<Transcript | null>(null);
  let busy = $state(false);

  async function load(): Promise<void> {
    const out = await backend.handle.call('minutes.get', { minutes_id: minutesId });
    if (out.kind === 'error') {
      error = out.error;
      return;
    }
    error = null;
    conflict = false;
    minutes = out.result;
    body = out.result.body;
    saved = out.result.body;
  }

  onMount(() => void load());

  async function save(): Promise<void> {
    if (minutes === null || !changed || busy) return;
    busy = true;
    const out = await backend.handle.call('minutes.save_edit', { minutes_id: minutes.id, body, base_revision: minutes.revision });
    busy = false;
    if (out.kind === 'error') {
      conflict = out.error.code === 'MIN_EDIT_CONFLICT';
      actionError = out.error;
      return;
    }
    actionError = null;
    minutes = out.result;
    saved = out.result.body;
    app?.shell.announce(t('minutes-saved'));
  }

  async function toggleTranscript(): Promise<void> {
    if (transcript !== null) {
      transcript = null;
      return;
    }
    const out = await backend.handle.call('minutes.transcript', { minutes_id: minutesId });
    if (out.kind === 'error') actionError = out.error;
    else transcript = out.result;
  }

  const at = (ms: number): string => elapsed(new Date(0).toISOString(), ms);
</script>

<section class="minutes-page" aria-labelledby="minutes-page-heading">
  <ErrorLine {error} onretry={() => void load()} />
  {#if minutes !== null}
    <div class="heading tree-line">
      <span class="icon-tile"><Icon name="mic" size="heading" /></span>
      <h2 id="minutes-page-heading" tabindex="-1" data-region-focus>{minutes.title}</h2>
      <span class="actions">
        <button type="button" class="icon-btn" data-testid="minutes-save" aria-label={t('minutes-save')} title={t('minutes-save')} disabled={!changed || busy} onclick={() => void save()}><Icon name="save" size="small" /></button>
        <button type="button" class="icon-btn" data-testid="minutes-show-transcript" aria-label={t('minutes-show-transcript')} title={t('minutes-show-transcript')} aria-pressed={transcript !== null} onclick={() => void toggleTranscript()}><Icon name="eye" size="small" /></button>
        <MinutesMenu {backend} minutesId={minutes.id} title={minutes.title} shown hasRecording={minutes.recording != null} onerror={(e) => (actionError = e)} onrecordingdeleted={(m) => (minutes = m)} />
      </span>
    </div>
    <p class="secondary" data-testid="minutes-kept-on">{t('minutes-kept-on', { source: minutes.source_name, date: formatDate(minutes.kept_at) })}</p>
    <ErrorLine error={actionError} />
    {#if conflict}<button type="button" data-testid="minutes-reopen" onclick={() => void load()}>{t('minutes-reopen')}</button>{/if}
    <MarkdownEditor bind:value={body} label={minutes.title} />
    {#if transcript !== null}
      <ol class="transcript" data-testid="minutes-transcript">
        {#each transcript.utterances as u, i (i)}
          <li><span class="time">{at(u.start_ms)}</span> <span class="speaker">{u.speaker ?? ''}</span> <span class="text">{u.text}</span></li>
        {/each}
      </ol>
    {/if}
  {/if}
</section>


<style>
  .minutes-page {
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-04);
    margin-inline-start: auto;
  }
  /* The knowledge page's icon buttons (Knowledge.svelte): the same size and pressed look. */
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    inline-size: 1.75rem;
    block-size: 1.75rem;
    min-height: 0;
    padding: 0;
  }
  .icon-btn[aria-pressed='true'] {
    background: var(--color-accent-soft);
    color: var(--color-accent-ink);
  }
  .transcript {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .time {
    font-variant-numeric: tabular-nums;
    color: var(--color-text-secondary);
  }
  .speaker {
    font-weight: 600;
  }
</style>
