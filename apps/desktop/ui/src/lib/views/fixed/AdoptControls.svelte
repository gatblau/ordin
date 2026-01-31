<script lang="ts">
  // The Adopt actions of one fact or page (ADR-110, steps 5 and 6 of the Team knowledge amendment):
  // **Adopt into [own folder]…** on an item with `folder` set when the workspace has an own folder with
  // read-write access; **Adopt again** and **Keep as it is** on an item whose own copy changed at its
  // source; with `removed_at_source` only **Keep as it is**. Adopting is only ever this dialog.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-13
  import { t } from '../../i18n/fluent';
  import type { AdoptionMark, AdoptItem } from '../../service-api/types.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import AdoptDialog from './AdoptDialog.svelte';
  import type { TeamController } from './team.svelte';

  interface Props {
    backend: BackendState;
    team: TeamController;
    item: AdoptItem;
    /** The read folder the item belongs to (`FactView.folder` / `PageView.folder`). */
    folder?: string | null | undefined;
    /** Set on an own item adopted from a read folder that has since changed or gone. */
    mark?: AdoptionMark | null | undefined;
    /** Where an adopted item came from, for `team-adopt-from` when adopting again. */
    adoptedFrom?: string | null | undefined;
    sharedBy?: string | null | undefined;
    statement?: string;
    title?: string;
    body?: string;
    /** Called after an adoption or a kept item, so the list can be read again. */
    ondone?: () => void;
  }
  let { backend, team, item, folder, mark, adoptedFrom, sharedBy, statement = '', title = '', body = '', ondone }: Props = $props();

  let dialog = $state<{ again: boolean } | null>(null);
  const canAdoptHere = $derived(team.canAdopt && !!folder && !mark);
  const source = $derived(folder ?? adoptedFrom ?? '');

  async function keep(): Promise<void> {
    if (await team.keepAsIs(item)) ondone?.();
  }
</script>

{#if canAdoptHere}
  <button type="button" onclick={() => (dialog = { again: false })}>{t('team-adopt', { folder: team.ownFolderName })}</button>
{/if}
{#if mark}
  {#if mark === 'changed_at_source' && team.canAdopt}
    <button type="button" onclick={() => (dialog = { again: true })}>{t('team-adopt-again')}</button>
  {/if}
  <button type="button" onclick={() => void keep()}>{t('team-keep-as-is')}</button>
{/if}
{#if dialog !== null}
  <AdoptDialog
    controller={team}
    backend={backend.handle}
    {item}
    ownFolder={team.ownFolderName}
    fromFolder={source}
    sharedBy={sharedBy ?? ''}
    {statement}
    {title}
    {body}
    again={dialog.again}
    onclose={() => (dialog = null)}
    {...ondone ? { ondone } : {}}
  />
{/if}
