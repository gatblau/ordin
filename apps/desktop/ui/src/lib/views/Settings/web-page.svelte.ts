// A page ordin names, opened in the person's browser by `app.open_web_page` (ServiceApi.md SVC-WEB-1;
// amended January 2026 (user decision)): BotFather for Telegram (SM-SW-5) and Atlassian's token page for
// Jira (SM-SW-11). The command takes the page's name, never an address; it goes to the local backend.
//
// spec: ordin-desktop/components/ServiceApi#SVC-WEB-1
import type { BackendHandle } from '../../backend/backend';
import type { ServiceError, WebPage } from '../../service-api/types.gen';

export class WebPageOpener {
  /** The refusal of the last press, drawn under the button with **Try again**. */
  error = $state<ServiceError | null>(null);
  /** The page last asked for, which **Try again** opens again. */
  last: WebPage | null = null;
  readonly #local: BackendHandle;

  constructor(local: BackendHandle) {
    this.#local = local;
  }

  async open(page: WebPage): Promise<boolean> {
    this.last = page;
    const out = await this.#local.call('app.open_web_page', { page });
    this.error = out.kind === 'error' ? out.error : null;
    return out.kind === 'ok';
  }

  async retry(): Promise<void> {
    if (this.last !== null) await this.open(this.last);
  }
}
