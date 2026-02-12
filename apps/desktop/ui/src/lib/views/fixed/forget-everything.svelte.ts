// Forget everything, for one workspace or every workspace (SM-FE-1 to SM-FE-3): the count, the typed
// confirmation and the forgetting.
//
// spec: ordin-desktop/components/ScreensModule#SM-FE-2
// spec: ordin-desktop/components/ScreensModule#SM-FE-3
import type { BackendHandle } from '../../backend/backend';
import type { EverythingCount, ForgetTarget, ServiceError } from '../../service-api/types.gen';

export class ForgetEverything {
  readonly #handle: BackendHandle;
  readonly target: ForgetTarget;
  /** What the person types to confirm: the workspace's name, or the every-workspace phrase. */
  readonly phrase: string;
  count = $state<EverythingCount | null>(null);
  typed = $state('');
  running = $state(false);
  error = $state<ServiceError | null>(null);

  constructor(handle: BackendHandle, target: ForgetTarget, phrase: string) {
    this.#handle = handle;
    this.target = target;
    this.phrase = phrase;
  }

  /** SM-FE-2.2: nothing remembered and nothing to check. */
  get nothing(): boolean {
    return this.count !== null && this.count.facts === 0 && this.count.to_check === 0;
  }

  /** SM-FE-2.4: the trimmed text equals the phrase, ignoring case. */
  get confirmed(): boolean {
    return this.typed.trim().toLocaleLowerCase() === this.phrase.trim().toLocaleLowerCase();
  }

  /** `memory.count_everything`; the error, when it fails, is the opener's to draw. */
  async open(): Promise<ServiceError | null> {
    const out = await this.#handle.call('memory.count_everything', { of: this.target });
    if (out.kind === 'error') return out.error;
    this.count = out.result;
    return null;
  }

  /** `memory.forget_everything`: the facts removed, or `null` with `error` set and the dialog kept open. */
  async forget(): Promise<number | null> {
    if (!this.confirmed || this.running) return null;
    this.running = true;
    this.error = null;
    const out = await this.#handle.call('memory.forget_everything', { of: this.target });
    this.running = false;
    if (out.kind === 'error') {
      this.error = out.error;
      return null;
    }
    return out.result.facts_removed;
  }
}
