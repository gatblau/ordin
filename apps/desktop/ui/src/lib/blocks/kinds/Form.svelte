<script lang="ts">
  // `form`: labelled fields by input (text, document picker, place picker, choice), a mark on fields Ordin
  // filled, and the form's own verb. The cursor starts in the first empty field. The verb is the task's own;
  // it never approves anything.
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { ContentRef, FileEntry, ServiceError } from '../../service-api/types.gen';
  import ErrorLine from '../../views/ErrorLine.svelte';
  import { isWaitingForServer } from '../ask';
  import type { KindProps } from '../types';

  let { backend, workspaceId, blockId, block }: KindProps<'form'> = $props();

  type Value = string | ContentRef | null;
  const start = (): Record<string, Value> => {
    const initial: Record<string, Value> = {};
    for (const field of block.fields) initial[field.name] = field.value ?? null;
    return initial;
  };
  let values = $state<Record<string, Value>>(start());
  let busy = $state(false);
  let error = $state<ServiceError | null>(null);
  let root = $state<HTMLElement | null>(null);
  let documentPlace = $state<Record<string, string>>({});
  let files = $state<Record<string, FileEntry[]>>({});

  const places = $derived(backend.trees.get(workspaceId)?.places ?? []);
  const waiting = $derived(backend.conversation.get(workspaceId)?.find((i) => i.block_id === blockId)?.waiting ?? true);
  const complete = $derived(block.fields.every((f) => !f.required || (values[f.name] !== null && values[f.name] !== '')));
  const disabled = $derived(busy || !waiting || isWaitingForServer(backend) || !backend.groups.has('ordin'));

  onMount(() => {
    // The cursor starts in the first empty field (the first field when all are filled).
    const target = block.fields.find((f) => values[f.name] === null || values[f.name] === '') ?? block.fields[0];
    if (target !== undefined) root?.querySelector<HTMLElement>(`[data-field="${CSS.escape(target.name)}"]`)?.focus();
  });

  async function pickPlace(name: string, placeId: string): Promise<void> {
    documentPlace[name] = placeId;
    files[name] = [];
    values[name] = null;
    if (placeId === '' || !backend.groups.has('files')) return;
    const out = await backend.handle.call('files.list_folder', { place_id: placeId, folder: '', sort: 'name_asc' });
    if (out.kind === 'ok') files[name] = out.result.items.filter((e) => !e.is_folder);
  }

  function pickFile(name: string, path: string): void {
    const place = documentPlace[name];
    values[name] = place === undefined || path === '' ? null : { kind: 'place_file', place_id: place, relative_path: path };
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
        <span class="document-picker">
          <select data-field={field.name} aria-label={field.label} {disabled} onchange={(e) => void pickPlace(field.name, e.currentTarget.value)}>
            <option value=""></option>
            {#each places as place (place.id)}<option value={place.id}>{place.friendly_name}</option>{/each}
          </select>
          <select id={`f-${blockId}-${field.name}`} required={field.required} {disabled} onchange={(e) => pickFile(field.name, e.currentTarget.value)}>
            <option value=""></option>
            {#each files[field.name] ?? [] as entry (entry.path)}<option value={entry.path}>{entry.name}</option>{/each}
          </select>
        </span>
      {/if}
    </div>
  {/each}
  <button type="submit" class="primary" disabled={disabled || !complete}>{block.verb}</button>
  <ErrorLine {error} />
</form>

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
  .document-picker {
    display: flex;
    gap: var(--space-sm);
  }
</style>
