// Settings › Telegram (11 §5.25, UX-A51): what `telegram.status` offers, **Set up Telegram** with the key
// BotFather sent, and **Remove Telegram**.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-1
import type { BackendHandle } from '../../backend/backend';
import type { NotOfferedReason, ServiceError, TelegramStatus } from '../../service-api/types.gen';

/** The sentence for a `reason_not_offered` (SM-NS-1.3). */
export const NOT_OFFERED_MESSAGE: Readonly<Record<NotOfferedReason, string>> = {
  not_allowed_by_company: 'settings-telegram-not-allowed',
  tools_turned_off: 'settings-telegram-tools-off',
  app_not_on_computer: 'settings-telegram-no-app',
  no_bot: 'settings-telegram-no-bot',
};

export class TelegramModel {
  status = $state.raw<TelegramStatus | null>(null);
  /** The key field: a password field, cleared after sending. */
  key = $state('');
  error = $state<ServiceError | null>(null);
  readonly #handle: BackendHandle;

  constructor(handle: BackendHandle) {
    this.#handle = handle;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('telegram.status', {});
    if (out.kind === 'ok') {
      this.status = out.result;
      this.error = null;
    } else this.error = out.error;
  }

  /** **Connect**: `telegram.connect_bot { key }`; the reply's `BotInfo` draws the bot line. */
  async connect(): Promise<boolean> {
    const key = this.key;
    this.key = '';
    if (key.trim() === '' || this.status === null) return false;
    const out = await this.#handle.call('telegram.connect_bot', { key });
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.error = null;
    this.status = { ...this.status, connected: true, bot: out.result };
    return true;
  }

  /** **Remove Telegram**: at once. */
  async remove(): Promise<void> {
    const out = await this.#handle.call('telegram.remove', {});
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    await this.load();
  }
}
