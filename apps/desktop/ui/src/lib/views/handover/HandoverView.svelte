<script lang="ts">
  // The handover screen drawn from a controller (SM-NS-8): the note as text, what is handed over, the
  // Slack lines, and the buttons the screen's `actions` list. Every string from the handover is a text
  // node; nothing in the note is a link or an instruction to the reader. Icons per V8 (`hoScreen`).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-8
  import AskButton from '../../blocks/AskButton.svelte';
  import { formatDate, formatTime } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type { ReturnOption } from '../../service-api/types.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import Icon from '../../icons/Icon.svelte';
  import { fileKindIcon, fileKindTint } from '../../icons/kinds';
  import type { IconName } from '../../icons/sprite';
  import { openSlackPermalink } from '../fixed/slack-link';
  import { appOrNull } from '../fixed/util';
  import { isSendersSide, stateMessageId, type HandoverController } from './handover-controller.svelte';

  interface Props {
    backend: BackendState;
    workspaceId: string | null;
    c: HandoverController;
    onclose?: () => void;
  }
  let { backend, workspaceId, c, onclose }: Props = $props();
  // The app context is read while the view is set up; Svelte's getContext answers nothing inside a click handler.
  const app = appOrNull();

  let accepting = $state(false);
  let returning = $state(false);
  let reason = $state('');
  let chosen = $state(0);
  let confirmTakeBack = $state(false);

  const screen = $derived(c.screen);
  const has = (a: 'accept_and_start' | 'return_to' | 'ask_ordin' | 'close'): boolean => screen?.actions.includes(a) ?? false;
  const first = $derived(screen?.handover.events[0]);
  const paragraphs = (body: string): string[] => body.split(/\n\s*\n/).filter((p) => p.trim() !== '');
  // V8 *Handover*: a resource handed over draws `sheet` or `pdf` by its name's extension, else `word`.
  const resultIcon = (label: string): IconName => {
    const kind = fileKindIcon(label);
    return kind === 'sheet' || kind === 'pdf' ? kind : 'word';
  };
  const optionLabel = (o: ReturnOption): string => t('handover-return-option', { name: o.person.name, stage: o.stage, round: o.round });

  async function startAccept(): Promise<void> {
    accepting = true;
    await c.loadWorkspaces();
  }
  async function startReturn(): Promise<void> {
    returning = true;
    await c.loadReturnOptions();
  }
  async function sendReturn(): Promise<void> {
    const to = c.options[chosen];
    if (to !== undefined && (await c.returnTo(reason, to))) returning = false;
  }
</script>

<section class="handover" aria-labelledby="handover-heading">
  {#if screen !== null}
    {@const h = screen.handover}
    <h2 id="handover-heading" tabindex="-1" data-region-focus>{t('handover-screen-title', { title: h.title })}</h2>
    <p class="from">{t('handover-screen-from', { name: h.sender, time: first === undefined ? '' : `${formatDate(first.at)}, ${formatTime(first.at)}` })}</p>
    {#if h.stage_name}<p><Icon name="run" size="small" /> {t('handover-screen-stage', { stage: h.stage_name })}</p>{/if}
    {#if screen.arrived}<p>{t('handover-screen-arrived', { done: screen.arrived[0], total: screen.arrived[1] })}</p>{/if}
    <div class="note">
    {#each h.note.sections as section, i (i)}
      <section>
        <h3>{section.heading}</h3>
        {#each paragraphs(section.body) as paragraph, j (j)}<p class="text">{paragraph}</p>{/each}
        {#if section.drafted_by_ordin}<p class="quiet">{t('handover-drafted-by-ordin')}</p>{/if}
      </section>
    {/each}
    {#each screen.marked_lines as line, i (i)}
      <p class="text">{line}</p>
      <p class="marked">{t('handover-line-instruction')}</p>
    {/each}
    </div>
    <h3>{t('handover-results')}</h3>
    <ul class="results">
      {#each screen.results as result, i (i)}
        <li><span class="icon-tile" data-tint={fileKindTint(resultIcon(result.label))}><Icon name={resultIcon(result.label)} /></span>{result.label}{#if result.cannot_open} <span class="quiet">{result.cannot_open}</span>{/if}</li>
      {/each}
    </ul>
    {#if h.checks_passed.length > 0}
      <h3>{t('handover-checked-by', { name: h.sender })}</h3>
      <ul class="checks">{#each h.checks_passed as check, i (i)}<li>{check}</li>{/each}</ul>
    {/if}
    {#each screen.slack as line, i (i)}
      <p>
        {line.destination_line}
        {#if line.state === 'posted' && line.permalink}
          <button type="button" onclick={() => void openSlackPermalink(backend.handle, line.permalink ?? '', app?.diagnostics)}><Icon name="open" />{t('slack-open-in-slack')}</button>
        {/if}
      </p>
    {/each}

    {#if isSendersSide(screen)}
      <p>{t(stateMessageId(h.state), { name: h.receiver.name })}</p>
      <button type="button" disabled={c.busy} onclick={() => (confirmTakeBack = true)}>{t('handover-take-back')}</button>
      <button type="button" disabled={c.busy} onclick={() => void c.remind()}>{t('handover-remind', { name: h.receiver.name })}</button>
    {/if}

    <div class="actions">
      {#if has('accept_and_start')}<button type="button" class="primary" disabled={c.busy} onclick={() => void startAccept()}>{t('handover-accept')}</button>{/if}
      {#if has('return_to')}<button type="button" disabled={c.busy} onclick={() => void startReturn()}><Icon name="send" />{t('handover-return', { name: h.sender })}</button>{/if}
      {#if has('ask_ordin') && workspaceId !== null}
        <AskButton {backend} {workspaceId} labelId="block-ask-about-this" text={t('block-ask-text-this', { title: h.title })} />
      {/if}
      {#if has('close')}<button type="button" onclick={() => onclose?.()}>{t('card-close')}</button>{/if}
    </div>

    {#if accepting}
      <fieldset class="accept">
        <legend>{t('handover-accept-question')}</legend>
        <button type="button" disabled={c.busy} onclick={() => void c.accept({ kind: 'new_workspace', name: h.title }).then((done) => done && (accepting = false))}>{t('handover-accept-new')}</button>
        {#each c.workspaces as workspace (workspace.id)}
          <button type="button" disabled={c.busy} onclick={() => void c.accept({ kind: 'existing', workspace_id: workspace.id }).then((done) => done && (accepting = false))}>{workspace.name}</button>
        {/each}
      </fieldset>
    {/if}

    {#if returning}
      <form
        class="return"
        onsubmit={(e) => {
          e.preventDefault();
          void sendReturn();
        }}
      >
        <label>
          {t('handover-return-why')}
          <textarea required rows="3" bind:value={reason}></textarea>
        </label>
        {#each c.options as option, i (i)}
          <label>
            <input type="radio" name="return-to" checked={chosen === i} onchange={() => (chosen = i)} />
            {optionLabel(option)}
          </label>
        {/each}
        <button type="submit" disabled={c.busy || reason.trim() === '' || c.options.length === 0}><Icon name="send" />{t('handover-return', { name: c.options[chosen]?.person.name ?? h.sender })}</button>
      </form>
    {/if}
  {/if}
  <ErrorLine error={c.error} onretry={() => void c.load()} />
</section>

{#if confirmTakeBack && screen !== null}
  <ConfirmDialog
    title={t('handover-take-back-title', { name: screen.handover.receiver.name })}
    verb={t('handover-take-back-verb')}
    onconfirm={() => {
      confirmTakeBack = false;
      void c.takeBack();
    }}
    oncancel={() => (confirmTakeBack = false)}
  />
{/if}

<style>
  /* The prototype's handover screen (scene `hoScreen`, `.ptmain`, `.cgrp`, `.hores`). */
  .handover {
    gap: var(--space-sm);
  }
  .handover > h2 {
    font-size: 19px;
    margin-block-end: var(--space-06);
  }
  .handover > h3 {
    font-size: 14px;
    margin-block-start: var(--space-sm);
  }
  .from {
    font-weight: 700;
  }
  /* The stage line follows the sender's line; it carries no class of its own. */
  .from + :global(p) {
    font-size: 13px;
    color: var(--color-text-muted);
    display: flex;
    gap: var(--space-04);
    align-items: center;
  }
  .note {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-lg);
    padding: var(--space-md) 18px;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .note > section {
    background: none;
    border: 0;
    border-radius: 0;
    padding: 0;
    gap: var(--space-06);
  }
  .note > section > h3 {
    font-size: 17px;
  }
  .marked {
    align-self: flex-start;
    border-radius: var(--radius-pill);
    padding: 1px var(--space-sm);
    font-size: 12px;
    font-weight: 600;
    background: var(--color-waiting-soft);
    color: var(--color-waiting-text);
  }
  .results,
  .checks {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .results > :global(li) {
    display: flex;
    gap: var(--space-sm);
    align-items: center;
    flex-wrap: wrap;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: var(--space-06) var(--space-10);
    background: var(--color-surface);
    font-weight: 700;
  }
  .results .quiet {
    font-weight: 400;
    font-size: 13px;
  }
  .checks > li {
    font-size: 16px;
  }
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .quiet {
    color: var(--color-text-muted);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-06);
    margin-block-start: var(--space-sm);
  }
</style>
