<script lang="ts">
  // `form`: labelled fields by input (text, document, place picker, choice), a mark on fields Ordin filled,
  // and the form's own verb. The cursor starts in the first empty field. The verb is the task's own; it never
  // approves anything. A document field is one button, **Choose a document…**, that opens the document
  // browser, as a task's form does (SM-TF-3, SM-TF-9; amended January 2026 (user decision)): it replaces the
  // two drop-down lists of the document picker.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-TF-9
  // spec: ordin-desktop/components/ScreensModule#SM-SW-17
  import { onMount, tick } from 'svelte';

  import { answers } from '../../backend/groups';
  import { t } from '../../i18n/fluent';
  import type { ContentRef, ServiceError, SourceLink } from '../../service-api/types.gen';
  import ErrorLine from '../../views/ErrorLine.svelte';
  import DocumentBrowser from '../../views/tasks/DocumentBrowser.svelte';
  import { documentLine, documentRoots } from '../../views/tasks/document-names';
  import { isWaitingForServer } from '../ask';
  import { dismissBlock } from '../dismiss';
  import type { KindProps } from '../types';

  let { backend, workspaceId, blockId, block }: KindProps<'form'> = $props();

  type Value = string | ContentRef | null;
  const start = (): Record<string, Value> => {
    const initial: Record<string, Value> = {};
    for (const field of block.fields) initial[field.name] = Array.isArray(field.value) ? null : (field.value ?? null);
    return initial;
  };
  let values = $state<Record<string, Value>>(start());
  let busy = $state(false);
  let error = $state<ServiceError | null>(null);
  let root = $state<HTMLElement | null>(null);
  let links = $state.raw<SourceLink[]>([]);
  /** The document field whose browser is open, or `null`. */
  let browsing = $state<{ name: string; label: string } | null>(null);

  const places = $derived(backend.trees.get(workspaceId)?.places ?? []);
  const roots = $derived(documentRoots(places, links));
  const waiting = $derived(backend.conversation.get(workspaceId)?.find((i) => i.block_id === blockId)?.waiting ?? true);
  const complete = $derived(block.fields.every((f) => !f.required || (values[f.name] !== null && values[f.name] !== '')));
  const disabled = $derived(busy || !waiting || isWaitingForServer(backend) || !backend.groups.has('ordin'));

  onMount(() => {
    // The cursor starts in the first empty field (the first field when all are filled).
    const target = block.fields.find((f) => values[f.name] === null || values[f.name] === '') ?? block.fields[0];
    if (target !== undefined) focusField(target.name);
    // The linked places the browser offers (SM-TF-9 step 1), as a task's form reads them.
    if (answers(backend, 'workspaces.links')) {
      void backend.handle.call('workspaces.links', { workspace_id: workspaceId }).then((out) => {
        if (out.kind === 'ok') links = out.result;
      });
    }
  });

  function focusField(name: string): void {
    root?.querySelector<HTMLElement>(`[data-field="${CSS.escape(name)}"]`)?.focus();
  }

  /** The value of a document field, when it holds a file. */
  function chosenFile(name: string): { place_id: string; relative_path: string } | null {
    const value = values[name];
    return value !== null && typeof value === 'object' && value.kind === 'place_file' ? value : null;
  }

  /** SM-TF-9 step 4: the chosen file sets the field; focus goes back to it. */
  async function chooseFile(name: string, placeId: string, path: string): Promise<void> {
    browsing = null;
    values[name] = { kind: 'place_file', place_id: placeId, relative_path: path };
    await tick();
    focusField(name);
  }

  async function closeBrowser(name: string): Promise<void> {
    browsing = null;
    await tick();
    focusField(name);
  }

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (disabled || !complete) return;
    busy = true;
    const fields: Record<string, unknown> = {};
    for (const f of block.fields) fields[f.name] = values[f.name] ?? null;
    const out = await backend.handle.call('ordin.answer_block', { block_id: blockId, workspace_id: workspaceId, answer: { kind: 'form', fields } });
    busy = false;
    if (out.kind === 'ok' && out.result.accepted) {
      error = null;
      backend.markAnswered(workspaceId, blockId);
    } else if (out.kind === 'error') {
      error = out.error;
    }
  }

  // SM-DIS-1: Dismiss sets the form aside; nothing is sent and Ordin is told.
  async function dismiss(): Promise<void> {
    if (disabled) return;
    busy = true;
    error = await dismissBlock(backend, workspaceId, blockId);
    busy = false;
  }
</script>

<form bind:this={root} class="block-form" onsubmit={(e) => void submit(e)} aria-labelledby={`form-${blockId}`}>
  <h3 id={`form-${blockId}`}>{block.title}</h3>
  {#each block.fields as field (field.name)}
    <div class="field">
      <label for={`f-${blockId}-${field.name}`}>{field.label}{#if field.required}<span aria-hidden="true"> *</span>{/if}</label>
      {#if field.filled_by_ordin}<span class="filled-mark"><span aria-hidden="true">✎</span> {t('block-filled-by-ordin')}</span>{/if}
      {#if field.input === 'text'}
        <input
          data-field={field.name}
          id={`f-${blockId}-${field.name}`}
          type="text"
          required={field.required}
          placeholder={field.placeholder ?? undefined}
          value={typeof values[field.name] === 'string' ? (values[field.name] as string) : ''}
          oninput={(e) => (values[field.name] = e.currentTarget.value)}
          {disabled}
        />
      {:else if field.input === 'choice'}
        <select data-field={field.name} id={`f-${blockId}-${field.name}`} required={field.required} {disabled} onchange={(e) => (values[field.name] = e.currentTarget.value === '' ? null : e.currentTarget.value)}>
          <option value="" selected={values[field.name] === null}></option>
          {#each field.options as option (option)}<option value={option} selected={values[field.name] === option}>{option}</option>{/each}
        </select>
      {:else if field.input === 'place'}
        <select data-field={field.name} id={`f-${blockId}-${field.name}`} required={field.required} {disabled} onchange={(e) => (values[field.name] = e.currentTarget.value === '' ? null : e.currentTarget.value)}>
          <option value="" selected={values[field.name] === null}></option>
          {#each places as place (place.id)}<option value={place.id} selected={values[field.name] === place.id}>{place.friendly_name}</option>{/each}
        </select>
      {:else}
        {@const chosen = chosenFile(field.name)}
        <span class="document-picker">
          {#if chosen !== null}
            <span class="chosen-name">{documentLine(roots, chosen.place_id, chosen.relative_path)}</span>
          {/if}
          <button
            type="button"
            id={`f-${blockId}-${field.name}`}
            data-field={field.name}
            aria-haspopup="dialog"
            {disabled}
            onclick={() => (browsing = { name: field.name, label: field.label })}
          >{chosen === null ? t('task-form-choose-document') : t('task-form-change-document')}</button>
        </span>
      {/if}
    </div>
  {/each}
  <div class="buttons">
    <button type="submit" class="primary" disabled={disabled || !complete}>{block.verb}</button>
    {#if waiting}<button type="button" {disabled} onclick={() => void dismiss()}>{t('block-dismiss')}</button>{/if}
  </div>
  <ErrorLine {error} />
</form>

{#if browsing !== null}
  {@const open = browsing}
  <DocumentBrowser {backend} {roots} label={open.label} onchoose={(place, path) => void chooseFile(open.name, place, path)} oncancel={() => void closeBrowser(open.name)} />
{/if}

<style>
  /* Label beside its field (the prototype's `.form`, scene `form`). */
  .block-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    align-items: flex-start;
  }
  .block-form > h3 {
    margin: 0;
    font-size: 15px;
  }
  .field {
    display: grid;
    grid-template-columns: minmax(8rem, max-content) minmax(0, 1fr);
    gap: var(--space-04) var(--space-12);
    align-items: start;
    align-self: stretch;
  }
  .field > label {
    padding-block-start: 6px;
  }
  .field > :is(input, select, .document-picker) {
    grid-column: 2;
    grid-row: 1;
  }
  .filled-mark {
    grid-column: 2;
    color: var(--color-text-muted);
    font-size: 12px;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
  }
  .document-picker {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
    align-items: center;
  }
</style>
