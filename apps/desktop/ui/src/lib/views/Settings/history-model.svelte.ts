// Settings › History: the number of days conversations and history are kept. Validated in place; the last
// valid value stays until a valid one is saved.
//
// spec: ordin-desktop/components/ScreensModule#SM-UP-1
import type { BackendHandle } from '../../backend/backend';
import type { RetentionSetting, ServiceError } from '../../service-api/types.gen';

export class HistoryModel {
  days = $state<number | null>(null);
  /** What the box shows; kept as typed while it is invalid. */
  text = $state('');
  invalid = $state(false);
  error = $state<ServiceError | null>(null);
  setting = $state.raw<RetentionSetting | null>(null);
  busy = $state(false);
  readonly #handle: BackendHandle;

  constructor(handle: BackendHandle) {
    this.#handle = handle;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('history.get', {});
    if (out.kind === 'ok') {
      this.setting = out.result;
      this.days = out.result.days;
      this.text = String(out.result.days);
      this.error = null;
    } else this.error = out.error;
  }

  /** A whole number of days, 1 or more. */
  static parse(text: string): number | null {
    const trimmed = text.trim();
    if (!/^\d{1,9}$/.test(trimmed)) return null;
    const n = Number(trimmed);
    return n >= 1 ? n : null;
  }

  /** Sends the days when the text is valid; otherwise marks the box invalid and sends nothing. */
  async submit(text: string): Promise<boolean> {
    this.text = text;
    const n = HistoryModel.parse(text);
    if (n === null) {
      this.invalid = true;
      return false;
    }
    this.invalid = false;
    this.busy = true;
    const out = await this.#handle.call('history.set_days', { days: n });
    this.busy = false;
    if (out.kind === 'ok') {
      this.setting = out.result;
      this.days = out.result.days;
      this.text = String(out.result.days);
      this.error = null;
      return true;
    }
    this.error = out.error;
    this.text = this.days === null ? '' : String(this.days);
    return false;
  }
}
