// Settings › Notifications (11 §7): one switch per notification setting, read with `settings.get` of the
// section `notifications` and changed with `settings.change { key, value }`. A key outside the list below is
// not drawn. `DSK_PREFERENCES_WRITE_FAILED` returns the switch to its stored value and draws its sentence
// with **Try again**.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-1
import type { BackendHandle } from '../../backend/backend';
import type { ServiceError, SettingView } from '../../service-api/types.gen';

/** The five keys and the message of each, in the order drawn. */
export const NOTIFICATION_KEYS: readonly { key: string; id: string }[] = [
  { key: 'notifications.approvals', id: 'settings-notify-approvals' },
  { key: 'notifications.finished_work', id: 'settings-notify-finished-work' },
  { key: 'notifications.questions', id: 'settings-notify-questions' },
  { key: 'notifications.draft_reviews', id: 'settings-notify-draft-reviews' },
  { key: 'notifications.handovers', id: 'settings-notify-handovers' },
];

export class NotificationsModel {
  /** The stored views by key. */
  views = $state.raw<ReadonlyMap<string, SettingView>>(new Map());
  /** What each switch shows while a change is in flight. */
  shown = $state.raw<ReadonlyMap<string, boolean>>(new Map());
  error = $state<ServiceError | null>(null);
  #retry: (() => Promise<boolean>) | null = null;
  readonly #handle: BackendHandle;

  constructor(handle: BackendHandle) {
    this.#handle = handle;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('settings.get', { section: 'notifications' });
    if (out.kind === 'ok') {
      this.views = new Map(out.result.settings.map((s) => [s.key, s]));
      this.shown = new Map();
      this.error = null;
    } else this.error = out.error;
  }

  /** The keys drawn, in order: those the backend returned, among the five. */
  get rows(): { key: string; id: string; view: SettingView }[] {
    return NOTIFICATION_KEYS.flatMap((k) => {
      const view = this.views.get(k.key);
      return view === undefined ? [] : [{ ...k, view }];
    });
  }

  isOn(key: string): boolean {
    return this.shown.get(key) ?? this.views.get(key)?.value === true;
  }

  /** Disabled when the company set it or the person may not change it (SM-NS-1). */
  locked(view: SettingView): boolean {
    return view.set_by === 'company' || !view.person_may_change;
  }

  async change(key: string, value: boolean): Promise<boolean> {
    const view = this.views.get(key);
    if (view === undefined || this.locked(view)) return false;
    this.#retry = () => this.change(key, value);
    this.shown = new Map(this.shown).set(key, value);
    const out = await this.#handle.call('settings.change', { key, value });
    const shown = new Map(this.shown);
    shown.delete(key);
    this.shown = shown;
    if (out.kind === 'ok') {
      this.views = new Map(this.views).set(key, out.result);
      this.error = null;
      return true;
    }
    this.error = out.error.code === 'DSK_PREFERENCES_WRITE_FAILED' ? { ...out.error, action: 'try_again' } : out.error;
    return false;
  }

  retry(): void {
    void this.#retry?.();
  }
}
