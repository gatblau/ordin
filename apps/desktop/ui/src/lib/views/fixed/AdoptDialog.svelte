<script lang="ts">
  // Adopt into [own folder]… (ADR-110, step 6 of the Team knowledge amendment; SM-NS-13): the text of a
  // fact (its statement) or of a page (title and body) in editable fields, `team-adopt-help`, **Cancel**
  // and **Adopt**. `edit` carries only the fields the person changed (none: `null`). A refusal stays in the
  // dialog as one sentence, the text kept so the person can change it and try again. With `again` the same
  // dialog is **Adopt again**, opened on the source's current text.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-13
  import type { BackendHandle } from '../../backend/backend';
  import { t } from '../../i18n/fluent';
  import type { AdoptItem, ServiceError } from '../../service-api/types.gen';
  import ErrorLine from '../ErrorLine.svelte';
  import { adoptEdit, type TeamController } from './team.svelte';

  interface Props {
    controller: TeamController;
    backend: BackendHandle;
    item: AdoptItem;
    /** The own folder's name (`team-adopt-title`). */
    ownFolder: string;
    /** The folder the item came from, and who shared it (`team-adopt-from`). */
    fromFolder: string;
    sharedBy: string;
    /** The text as it is now: a fact's `statement`, or a page's `title` and `body`. */
    statement?: string;
    title?: string;
    body?: string;
    again?: boolean;
    onclose: () => void;
    ondone?: () => void;
  }
  let { controller, backend, item, ownFolder, fromFolder, sharedBy, statement = '', title = '', body = '', again = false, onclose, ondone }: Props = $props();

  // svelte-ignore state_referenced_locally
  let statementText = $state(statement);
  // svelte-ignore state_referenced_locally
  let titleText = $state(title);
  // svelte-ignore state_referenced_locally
  let bodyText = $state(body);
  let error = $state<ServiceError | null>(null);
  let busy = $state(false);

  async function submit(): Promise<void> {
    if (busy) return;
    busy = true;
    const out = await controller.adopt(item, adoptEdit(item.kind, { statement, title, body }, { statement: statementText, title: titleText, body: bodyText }), again);
    busy = false;
    if (out.ok) {
      onclose();
      ondone?.();
    } else error = out.error;
  }
</script>

<form
  class="adopt-dialog"
  role="group"
  aria-labelledby="team-adopt-title"
  onsubmit={(e) => {
    e.preventDefault();
    void submit();
  }}
  onkeydown={(e) => {
    if (e.key === 'Escape') onclose();
  }}
>
  <h3 id="team-adopt-title" tabindex="-1">{again ? t('team-adopt-again') : t('team-adopt-title', { folder: ownFolder })}</h3>
  <p>{t('team-adopt-from', { folder: fromFolder, name: sharedBy })}</p>
  {#if item.kind === 'fact'}
    <textarea rows="4" aria-labelledby="team-adopt-title" bind:value={statementText}></textarea>
  {:else}
    <input type="text" aria-labelledby="team-adopt-title" bind:value={titleText} />
    <textarea rows="8" aria-labelledby="team-adopt-title" bind:value={bodyText}></textarea>
  {/if}
  <p>{t('team-adopt-help')}</p>
  <ErrorLine {error} />
  <button type="button" onclick={onclose}>{t('confirm-cancel')}</button>
  <button type="submit" data-default="true" disabled={busy}>{t('team-adopt-verb')}</button>
</form>
