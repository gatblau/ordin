<script lang="ts">
  // A task's form (ADR-119): the screen `task-form:<agent instance id>:<command id>` opened from the tree,
  // and a `form` block whose `task` is set, opened by Ordin (SM-TF-8). It reads the task's form from
  // `agents.list`, draws one field per input, says which folders the agent will be able to read, checks the
  // fields, and starts the task with `runs.start_task`. Nothing is written to the conversation.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-TF-1
  // spec: ordin-desktop/components/ScreensModule#SM-TF-2
  // spec: ordin-desktop/components/ScreensModule#SM-TF-3
  // spec: ordin-desktop/components/ScreensModule#SM-TF-4
  // spec: ordin-desktop/components/ScreensModule#SM-TF-5
  // spec: ordin-desktop/components/ScreensModule#SM-TF-6
  // spec: ordin-desktop/components/ScreensModule#SM-TF-7
  // spec: ordin-desktop/components/ScreensModule#SM-TF-8
  // spec: ordin-desktop/components/ScreensModule#SM-TF-9
  import { tick } from 'svelte';

  import { answers } from '../../backend/groups';
  import type { ServiceError } from '../../backend/backend';
  import { dismissBlock } from '../../blocks/dismiss';
  import { formatList, formatLocale } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type {
    ContentRef,
    FolderGrant,
    FormField,
    InstalledAgent,
    TaskForm,
    TaskInput,
    TaskInputProblem,
    TaskInputValue,
    SourceLink,
  } from '../../service-api/types.gen';
  import { openScreen } from '../../shell/open-item';
  import { ScreenAreaState } from '../../shell/screen-area.svelte';
  import { makeKey, SCREEN_IDS } from '../../shell/screens';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { appOrNull } from '../fixed/util';
  import DocumentBrowser from './DocumentBrowser.svelte';
  import { documentLine, documentRoots, placeName as rootName } from './document-names';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    agentId: string;
    commandId: string;
    /** The key of the tab the form is drawn in; closed on Cancel and once the task has started. */
    tabKey: string;
    /** SM-TF-8: the waiting form block Ordin opened, and its fields with Ordin's values. */
    block?: { blockId: string; fields: FormField[] } | null;
  }
  let { backend, workspaceId, agentId, commandId, tabKey, block = null }: Props = $props();

  const app = appOrNull();
  const SHORT_TEXT_MAX = 200;
  const LONG_TEXT_MAX = 4000;
  const DOCUMENTS_MIN = 2;
  const NUMBER = /^-?[0-9]{1,15}(\.[0-9]{1,6})?$/;
  const WHOLE = /^-?[0-9]{1,15}$/;

  type Docs = { place_id: string; relative_path: string }[];

  let agents = $state.raw<InstalledAgent[] | null>(null);
  let grants = $state.raw<FolderGrant[] | null>(null);
  let loadError = $state.raw<ServiceError | null>(null);
  let startError = $state.raw<ServiceError | null>(null);
  let texts = $state<Record<string, string>>({});
  let docs = $state<Record<string, Docs>>({});
  let links = $state.raw<SourceLink[]>([]);
  let linksError = $state.raw<ServiceError | null>(null);
  /** The input whose document browser is open (SM-TF-9). */
  let browsing = $state.raw<TaskInput | null>(null);
  let problems = $state<Record<string, string>>({});
  let busy = $state(false);
  let waitingLine = $state(false);
  let started = false;
  let root = $state<HTMLElement | null>(null);
  let filledFor: string | null = null;

  const agent = $derived(agents?.find((a) => a.instance.id === agentId) ?? null);
  const form = $derived<TaskForm | null>(agent?.task_forms?.find((f) => f.command_id === commandId) ?? null);
  /** A task without inputs has one long-text field named `arguments` (SM-TF-2). */
  const inputs = $derived<TaskInput[]>(
    form === null
      ? []
      : (form.inputs ?? [
          { name: 'arguments', label: form.argument_label ?? t('task-form-details-label'), kind: 'long_text', required: false, choices: [], hint: null, max: null, default: null, whole: false },
        ]),
  );
  const places = $derived(backend.trees.get(workspaceId)?.places ?? []);
  const roots = $derived(documentRoots(places, links));
  const revision = $derived(backend.workspaceRevision(workspaceId));
  const blockWaiting = $derived(block === null ? true : (backend.conversation.get(workspaceId)?.find((i) => i.block_id === block.blockId)?.waiting ?? true));
  const readOnly = $derived(block !== null && !blockWaiting);
  const filledByOrdin = $derived(new Set((block?.fields ?? []).filter((f) => f.filled_by_ordin).map((f) => f.name)));

  async function load(): Promise<void> {
    const out = await backend.handle.call('agents.list', { workspace_id: workspaceId });
    if (out.kind === 'ok') {
      agents = out.result;
      loadError = null;
    } else loadError = out.error;
    if (answers(backend, 'workspaces.links')) {
      const l = await backend.handle.call('workspaces.links', { workspace_id: workspaceId });
      if (l.kind === 'ok') {
        links = l.result;
        linksError = null;
      } else linksError = l.error;
    }
    if (answers(backend, 'workspaces.grants')) {
      const g = await backend.handle.call('workspaces.grants', { workspace_id: workspaceId });
      grants = g.kind === 'ok' ? g.result : null;
    } else grants = null;
  }

  $effect(() => {
    void revision;
    void load();
  });

  $effect(() => {
    if (!backend.trees.has(workspaceId) && answers(backend, 'workspaces.tree')) void backend.loadTree(workspaceId);
  });

  /** Fields start from the block's values (SM-TF-8), else the input's default, once per form read. */
  $effect(() => {
    if (form === null) return;
    const key = form.command_id;
    if (filledFor === key) return;
    filledFor = key;
    const t0: Record<string, string> = {};
    const d0: Record<string, Docs> = {};
    for (const input of inputs) {
      const field = block?.fields.find((f) => f.name === input.name);
      if (input.kind === 'document' || input.kind === 'documents') {
        const list: Docs = [];
        if (field?.input === 'document' && field.value != null && field.value.kind === 'place_file') list.push({ place_id: field.value.place_id, relative_path: field.value.relative_path });
        if (field?.input === 'documents') for (const v of field.value) if (v.kind === 'place_file') list.push({ place_id: v.place_id, relative_path: v.relative_path });
        d0[input.name] = list;
      } else {
        let value: string | null = null;
        if (field?.input === 'text' || field?.input === 'choice') value = field.value ?? null;
        t0[input.name] = value ?? input.default ?? '';
      }
    }
    texts = t0;
    docs = d0;
    void tick().then(() => {
      // The cursor starts in the first field without a value (the first field when all have one).
      const first = inputs.find((i) => (i.kind === 'document' || i.kind === 'documents' ? (d0[i.name]?.length ?? 0) === 0 : (t0[i.name] ?? '') === '')) ?? inputs[0];
      if (first !== undefined) focusField(first.name);
    });
  });

  function fieldId(name: string): string {
    return `tf-${commandId}-${name}`.replace(/[^A-Za-z0-9_-]/g, '-');
  }

  function focusField(name: string): void {
    root?.querySelector<HTMLElement>(`[data-field="${CSS.escape(name)}"]`)?.focus();
  }

  function placeName(placeId: string): string {
    return rootName(roots, placeId);
  }

  function fileName(path: string): string {
    const parts = path.split('/');
    return parts[parts.length - 1] ?? path;
  }

  /** SM-TF-9 step 4: the chosen file sets a document field or joins a documents field; focus goes back. */
  async function chooseFile(input: TaskInput, place: string, path: string): Promise<void> {
    browsing = null;
    const doc = { place_id: place, relative_path: path };
    const list = docs[input.name] ?? [];
    if (!(input.kind === 'documents' && list.some((d) => d.place_id === place && d.relative_path === path))) {
      docs[input.name] = input.kind === 'document' ? [doc] : [...list, doc];
    }
    if (problems[input.name] !== undefined) problems = Object.fromEntries(Object.entries(problems).filter(([k]) => k !== input.name));
    await tick();
    focusField(input.name);
  }

  async function closeBrowser(input: TaskInput): Promise<void> {
    browsing = null;
    await tick();
    focusField(input.name);
  }

  /** SM-TF-3: a whole number's box takes only digits and a leading minus sign. */
  function wholeText(raw: string): string {
    const digits = raw.replace(/[^0-9]/g, '');
    return raw.trimStart().startsWith('-') ? `-${digits}` : digits;
  }

  function typeNumber(input: TaskInput, el: HTMLInputElement): void {
    if (input.whole) {
      const kept = wholeText(el.value);
      if (kept !== el.value) el.value = kept;
    }
    texts[input.name] = el.value;
  }

  /** SM-TF-3: the step buttons and the arrow keys change the value by 1; an empty box steps from 0. */
  function step(input: TaskInput, by: number): void {
    const raw = (texts[input.name] ?? '').trim();
    const value = raw === '' ? 0 : Number(numberText(raw));
    if (!Number.isFinite(value) || (raw !== '' && !NUMBER.test(numberText(raw)))) return;
    const next = input.whole ? String(Math.trunc(value) + by) : String(Number((value + by).toFixed(6)));
    const sep = decimalSeparator();
    texts[input.name] = sep === '.' ? next : next.replace('.', sep);
  }

  function numberKey(input: TaskInput, event: KeyboardEvent): void {
    if (readOnly || (event.key !== 'ArrowUp' && event.key !== 'ArrowDown')) return;
    event.preventDefault();
    step(input, event.key === 'ArrowUp' ? 1 : -1);
  }

  function removeDoc(name: string, index: number): void {
    docs[name] = (docs[name] ?? []).filter((_, i) => i !== index);
  }

  /** The folders of chosen documents the agent holds no grant on (SM-TF-4). */
  const accessFolders = $derived.by(() => {
    const ids: string[] = [];
    for (const list of Object.values(docs)) for (const d of list) if (!ids.includes(d.place_id)) ids.push(d.place_id);
    const held = (id: string): boolean =>
      grants !== null && grants.some((g) => g.place_id === id && g.grantee.kind === 'agent_instance' && g.grantee.agent_instance_id === agentId);
    return ids.filter((id) => !held(id)).map(placeName).filter((n) => n !== '');
  });

  function decimalSeparator(): string {
    const part = new Intl.NumberFormat(formatLocale()).formatToParts(1.1).find((p) => p.type === 'decimal');
    return part?.value ?? '.';
  }

  function numberText(raw: string): string {
    const sep = decimalSeparator();
    const trimmed = raw.trim();
    return sep === '.' ? trimmed : trimmed.split(sep).join('.');
  }

  function isDate(text: string): boolean {
    const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text);
    if (m === null) return false;
    const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])];
    const date = new Date(Date.UTC(y, mo - 1, d));
    return date.getUTCFullYear() === y && date.getUTCMonth() === mo - 1 && date.getUTCDate() === d;
  }

  /** SM-TF-5: each field checked as the core does, except a document's presence. */
  function check(): Record<string, string> {
    const found: Record<string, string> = {};
    for (const input of inputs) {
      const name = input.name;
      if (input.kind === 'document' || input.kind === 'documents') {
        const list = docs[name] ?? [];
        if (list.length === 0) {
          if (input.required) found[name] = t(input.kind === 'document' ? 'task-form-required-document' : 'task-form-required-documents');
        } else if (input.kind === 'documents' && list.length < DOCUMENTS_MIN) found[name] = t('task-form-too-few-documents');
        else if (input.kind === 'documents' && input.max != null && list.length > input.max) found[name] = t('task-form-too-many-documents', { max: input.max });
        continue;
      }
      const text = texts[name] ?? '';
      if (text.trim() === '') {
        if (input.required && (input.default ?? '') === '') found[name] = t('task-form-required');
        continue;
      }
      if (input.kind === 'short_text' && [...text].length > SHORT_TEXT_MAX) found[name] = t('task-form-too-long', { max: SHORT_TEXT_MAX });
      else if (input.kind === 'long_text' && [...text].length > LONG_TEXT_MAX) found[name] = t('task-form-too-long', { max: LONG_TEXT_MAX });
      else if (input.kind === 'number' && !NUMBER.test(numberText(text))) found[name] = t(input.whole ? 'task-form-not-a-whole-number' : 'task-form-not-a-number');
      else if (input.kind === 'number' && input.whole && !WHOLE.test(numberText(text))) found[name] = t('task-form-not-a-whole-number');
      else if (input.kind === 'date' && !isDate(text.trim())) found[name] = t('task-form-not-a-date');
    }
    return found;
  }

  function values(): TaskInputValue[] {
    const out: TaskInputValue[] = [];
    for (const input of inputs) {
      const name = input.name;
      const ref = (d: { place_id: string; relative_path: string }): ContentRef => ({ kind: 'place_file', place_id: d.place_id, relative_path: d.relative_path });
      if (input.kind === 'document') {
        const d = docs[name]?.[0];
        if (d !== undefined) out.push({ name, kind: 'document', document: ref(d) });
      } else if (input.kind === 'documents') {
        const list = docs[name] ?? [];
        if (list.length > 0) out.push({ name, kind: 'documents', documents: list.map(ref) });
      } else {
        const text = texts[name] ?? '';
        if (text.trim() === '') continue;
        if (input.kind === 'choice') out.push({ name, kind: 'choice', choice: text });
        else if (input.kind === 'number') out.push({ name, kind: 'text', text: numberText(text) });
        else if (input.kind === 'date') out.push({ name, kind: 'text', text: text.trim() });
        else out.push({ name, kind: 'text', text });
      }
    }
    return out;
  }

  function problemText(p: TaskInputProblem, input: TaskInput | undefined): string {
    switch (p.problem) {
      case 'required_empty':
        return t(input?.kind === 'document' ? 'task-form-required-document' : input?.kind === 'documents' ? 'task-form-required-documents' : 'task-form-required');
      case 'too_long':
        return t('task-form-too-long', { max: input?.kind === 'short_text' ? SHORT_TEXT_MAX : LONG_TEXT_MAX });
      case 'not_a_number':
        return t(input?.whole ? 'task-form-not-a-whole-number' : 'task-form-not-a-number');
      case 'not_a_date':
        return t('task-form-not-a-date');
      case 'too_few_documents':
        return t('task-form-too-few-documents');
      case 'too_many_documents':
        return t('task-form-too-many-documents', { max: input?.max ?? 0 });
      case 'not_a_choice':
        return t('task-form-not-a-choice');
      case 'document_gone':
        return t('task-form-document-gone', { name: p.document ?? '' });
      default:
        return t('task-form-changed');
    }
  }

  async function showProblems(found: Record<string, string>): Promise<void> {
    problems = found;
    await tick();
    const first = inputs.find((i) => found[i.name] !== undefined);
    if (first !== undefined) focusField(first.name);
  }

  function closeTab(): void {
    app?.screens.close(ScreenAreaState.scope(backend.id, workspaceId), tabKey);
  }

  async function start(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (busy || form === null || readOnly) return;
    waitingLine = false;
    startError = null;
    const found = check();
    if (Object.keys(found).length > 0) {
      await showProblems(found);
      return;
    }
    problems = {};
    busy = true;
    const out = await backend.handle.call('runs.start_task', {
      workspace_id: workspaceId,
      agent_instance_id: agentId,
      command_id: commandId,
      values: values(),
      ...(block !== null ? { block_id: block.blockId } : {}),
    });
    busy = false;
    if (out.kind === 'error') {
      startError = out.error;
      return;
    }
    const result = out.result;
    if (result.kind === 'started') {
      started = true;
      if (block !== null) backend.markAnswered(workspaceId, block.blockId);
      app?.shell.announce(t('task-form-started', { agent: agent?.instance.display_name ?? '', task: form.title }));
      if (app !== null) openScreen(app, backend.id, workspaceId, makeKey(SCREEN_IDS.run, result.run_id), { inFront: true });
      closeTab();
    } else if (result.kind === 'waiting') {
      waitingLine = true;
    } else {
      const found: Record<string, string> = {};
      let changed = false;
      for (const p of result.problems) {
        if (p.problem === 'wrong_kind' || p.problem === 'unknown_input') {
          changed = true;
          continue;
        }
        if (found[p.name] === undefined) found[p.name] = problemText(p, inputs.find((i) => i.name === p.name));
      }
      if (changed && inputs[0] !== undefined) {
        found[inputs[0].name] = t('task-form-changed');
        void load();
      }
      await showProblems(found);
    }
  }

  function cancel(): void {
    if (!started) closeTab();
  }

  // SM-DIS-1: Dismiss on the form Ordin opened sets it aside: nothing starts, Ordin is told, and the
  // form's tab closes. Cancel only closes the tab; the form still waits.
  async function dismiss(): Promise<void> {
    if (block === null || busy || started) return;
    busy = true;
    startError = await dismissBlock(backend, workspaceId, block.blockId);
    busy = false;
    if (startError === null) closeTab();
  }

  function keydown(event: KeyboardEvent): void {
    if (event.key !== 'Escape') return;
    event.preventDefault();
    cancel();
  }

  const describedBy = (input: TaskInput): string | undefined => {
    const ids = [input.hint ? `${fieldId(input.name)}-hint` : null, problems[input.name] ? `${fieldId(input.name)}-problem` : null].filter((x) => x !== null);
    return ids.length > 0 ? ids.join(' ') : undefined;
  };
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="task-form" bind:this={root} onkeydown={keydown} aria-labelledby={`${fieldId('')}-title`}>
  {#if agents === null}
    <ErrorLine error={loadError} onretry={() => void load()} />
  {:else if form === null}
    <p>{t('task-form-gone')}</p>
    <button type="button" onclick={closeTab}>{t('task-form-close')}</button>
  {:else if !form.startable}
    <h2 id={`${fieldId('')}-title`} tabindex="-1">{form.title}</h2>
    <p>{t('task-form-unreadable')}</p>
    <button type="button" onclick={closeTab}>{t('task-form-close')}</button>
  {:else}
    <form onsubmit={(e) => void start(e)} novalidate>
      <h2 id={`${fieldId('')}-title`}>{form.title}</h2>
      {#if form.description !== ''}<p class="description">{form.description}</p>{/if}
      {#each inputs as input (input.name)}
        {@const id = fieldId(input.name)}
        {@const invalid = problems[input.name] !== undefined}
        <div class="field">
          {#if input.kind === 'choice' && input.choices.length <= 4}
            <fieldset role="radiogroup" data-field={input.name} tabindex="-1" aria-invalid={invalid ? 'true' : undefined} aria-describedby={describedBy(input)}>
              <legend class="label">{input.label}{' '}{@render mark(input)}</legend>
              {#if filledByOrdin.has(input.name)}<span class="filled-mark"><span aria-hidden="true">✎</span> {t('block-filled-by-ordin')}</span>{/if}
              {#if input.hint}<span class="hint" id={`${id}-hint`}>{input.hint}</span>{/if}
              {#if !input.required && (input.default ?? '') === ''}
                <label><input type="radio" name={id} value="" checked={(texts[input.name] ?? '') === ''} disabled={readOnly} onchange={() => (texts[input.name] = '')} />{t('task-form-no-choice')}</label>
              {/if}
              {#each input.choices as option (option)}
                <label><input type="radio" name={id} value={option} checked={texts[input.name] === option} disabled={readOnly} onchange={() => (texts[input.name] = option)} />{option}</label>
              {/each}
            </fieldset>
          {:else}
            {#if input.kind === 'document' || input.kind === 'documents'}
              <span class="label" id={`${id}-label`}>{input.label}{' '}{@render mark(input)}</span>
            {:else}
              <label class="label" for={id}>{input.label}{' '}{@render mark(input)}</label>
            {/if}
            {#if filledByOrdin.has(input.name)}<span class="filled-mark"><span aria-hidden="true">✎</span> {t('block-filled-by-ordin')}</span>{/if}
            {#if input.hint}<span class="hint" id={`${id}-hint`}>{input.hint}</span>{/if}
            {#if input.kind === 'short_text'}
              <input {id} data-field={input.name} type="text" maxlength={SHORT_TEXT_MAX} value={texts[input.name] ?? ''} readonly={readOnly} aria-invalid={invalid ? 'true' : undefined} aria-describedby={describedBy(input)} oninput={(e) => (texts[input.name] = e.currentTarget.value)} />
            {:else if input.kind === 'long_text'}
              <textarea {id} data-field={input.name} rows="6" maxlength={LONG_TEXT_MAX} value={texts[input.name] ?? ''} readonly={readOnly} aria-invalid={invalid ? 'true' : undefined} aria-describedby={describedBy(input)} oninput={(e) => (texts[input.name] = e.currentTarget.value)}></textarea>
            {:else if input.kind === 'number'}
              <span class="number">
                <input {id} data-field={input.name} type="text" size="6" inputmode={input.whole ? 'numeric' : 'decimal'} value={texts[input.name] ?? ''} readonly={readOnly} aria-invalid={invalid ? 'true' : undefined} aria-describedby={describedBy(input)} oninput={(e) => typeNumber(input, e.currentTarget)} onkeydown={(e) => numberKey(input, e)} />
                {#if !readOnly}
                  <span class="steps">
                    <button type="button" class="step" tabindex="-1" aria-label={t('task-form-step-up')} title={t('task-form-step-up')} aria-controls={id} onclick={() => step(input, 1)}>▲</button>
                    <button type="button" class="step" tabindex="-1" aria-label={t('task-form-step-down')} title={t('task-form-step-down')} aria-controls={id} onclick={() => step(input, -1)}>▼</button>
                  </span>
                {/if}
              </span>
            {:else if input.kind === 'date'}
              <input {id} data-field={input.name} type="date" value={texts[input.name] ?? ''} readonly={readOnly} aria-invalid={invalid ? 'true' : undefined} aria-describedby={describedBy(input)} oninput={(e) => (texts[input.name] = e.currentTarget.value)} />
            {:else if input.kind === 'choice'}
              <select {id} data-field={input.name} disabled={readOnly} aria-invalid={invalid ? 'true' : undefined} aria-describedby={describedBy(input)} onchange={(e) => (texts[input.name] = e.currentTarget.value)}>
                {#if !input.required && (input.default ?? '') === ''}<option value="" selected={(texts[input.name] ?? '') === ''}>{t('task-form-no-choice')}</option>{/if}
                {#each input.choices as option (option)}<option value={option} selected={texts[input.name] === option}>{option}</option>{/each}
              </select>
            {:else}
              {@const chosen = docs[input.name] ?? []}
              {#each chosen as doc, i (`${doc.place_id}:${doc.relative_path}`)}
                <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
                <span class="chosen" data-field={input.kind === 'document' ? input.name : undefined} tabindex={input.kind === 'document' ? -1 : undefined} aria-invalid={input.kind === 'document' && invalid ? 'true' : undefined} aria-describedby={input.kind === 'document' ? describedBy(input) : undefined}>
                  <span class="chosen-name">{documentLine(roots, doc.place_id, doc.relative_path)}</span>
                  {#if !readOnly}
                    {#if input.kind === 'document'}
                      <button type="button" onclick={() => (browsing = input)}>{t('task-form-change-document')}</button>
                    {:else}
                      <button type="button" onclick={() => removeDoc(input.name, i)}>{t('task-form-remove-document', { name: fileName(doc.relative_path) })}</button>
                    {/if}
                  {/if}
                </span>
              {/each}
              {#if !readOnly && (input.kind === 'documents' || chosen.length === 0)}
                {@const full = input.kind === 'documents' && input.max != null && chosen.length >= input.max}
                <span class="document-picker">
                  <button type="button" {id} data-field={input.name} aria-haspopup="dialog" aria-labelledby={`${id}-label ${id}`} aria-invalid={invalid ? 'true' : undefined} aria-describedby={describedBy(input)} disabled={full} onclick={() => (browsing = input)}>{chosen.length === 0 ? t('task-form-choose-document') : t('task-form-add-document')}</button>
                </span>
              {/if}
              {#if input.kind === 'documents'}
                <span class="count">{t('task-form-documents-count', { count: chosen.length, max: input.max ?? 0 })}</span>
              {/if}
            {/if}
          {/if}
          {#if invalid}<span class="problem" id={`${id}-problem`}>{problems[input.name]}</span>{/if}
        </div>
      {/each}
      <ErrorLine error={linksError} onretry={() => void load()} />
      {#if accessFolders.length > 0 && !readOnly}
        <p class="access">{t('task-form-access', { agent: agent?.instance.display_name ?? '', folders: formatList(accessFolders) })}</p>
      {/if}
      {#if readOnly}
        <p>{t('task-form-sent')}</p>
      {:else}
        <div class="buttons">
          <button type="submit" class="primary" disabled={busy}>{busy ? t('task-form-starting') : form.title}</button>
          <button type="button" onclick={cancel}>{t('task-form-cancel')}</button>
          {#if block !== null}<button type="button" disabled={busy} onclick={() => void dismiss()}>{t('block-dismiss')}</button>{/if}
        </div>
        <div aria-live="polite">{#if waitingLine}<p>{t('run-screen-waiting')}</p>{/if}</div>
        <ErrorLine error={startError} />
      {/if}
    </form>
    {#if browsing !== null}
      {@const open = browsing}
      <DocumentBrowser {backend} {roots} label={open.label} onchoose={(place, path) => void chooseFile(open, place, path)} oncancel={() => void closeBrowser(open)} />
    {/if}
  {/if}
</section>

{#snippet mark(input: TaskInput)}
  <span class="mark">{input.required ? t('task-form-required-mark') : t('task-form-optional')}</span>
{/snippet}

<style>
  .task-form form {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    max-width: 44rem;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
  }
  fieldset {
    border: none;
    margin: 0;
    padding: 0;
  }
  /* SM-TF-3: the label in the normal reading size above its field, the hint below it in smaller type. */
  .label {
    font-size: 14px;
    font-weight: 600;
    padding: 0;
  }
  .mark {
    font-weight: 400;
    color: var(--color-text-muted);
  }
  .hint,
  .filled-mark,
  .count,
  .description {
    color: var(--color-text-muted);
    font-size: 12px;
  }
  .number {
    display: inline-flex;
    align-items: stretch;
    gap: 2px;
    align-self: flex-start;
  }
  .number input {
    width: calc(6ch + 16px);
  }
  .steps {
    display: inline-flex;
    flex-direction: column;
  }
  .step {
    font-size: 9px;
    line-height: 1;
    padding: 1px 6px;
  }
  .chosen {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }
  .problem {
    color: var(--color-danger);
    font-size: 12px;
  }
  .document-picker,
  .buttons {
    display: flex;
    gap: var(--space-sm);
  }
</style>
