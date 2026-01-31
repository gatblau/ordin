// The Ready to publish checklist card of a linked folder (SM-WR-13): a `result` block of kind `checklist`
// that carries `confluence_link_id` reads `confluence.ready_to_publish` for it.
//
// spec: ordin-desktop/components/ScreensModule#SM-WR-13
import type { BackendHandle } from '../backend/backend';
import type { ReadyItem, ServiceError } from '../service-api/types.gen';

export class ReadyListModel {
  items = $state.raw<ReadyItem[] | null>(null);
  error = $state.raw<ServiceError | null>(null);
  readonly #handle: Pick<BackendHandle, 'call'>;
  readonly confluenceLinkId: string;

  constructor(handle: Pick<BackendHandle, 'call'>, confluenceLinkId: string) {
    this.#handle = handle;
    this.confluenceLinkId = confluenceLinkId;
  }

  /** `confluence.ready_to_publish { confluence_link_id }`; a refusal is drawn in place of the list. */
  async load(): Promise<void> {
    const out = await this.#handle.call('confluence.ready_to_publish', { confluence_link_id: this.confluenceLinkId });
    if (out.kind === 'error') {
      this.error = out.error;
      this.items = null;
      return;
    }
    this.error = null;
    this.items = out.result;
  }
}
