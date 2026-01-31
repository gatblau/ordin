// Settings › Display: `display.appearance` (`system`, `light`, `dark`) through `settings.get` and
// `settings.change` of the section `display`, answered by the desktop program.
//
// spec: ordin-desktop/components/ScreensModule#SM-UP-1
import type { BackendHandle } from '../../backend/backend';
import type { ServiceError, SettingView } from '../../service-api/types.gen';

export const APPEARANCE_KEY = 'display.appearance';
export const APPEARANCES = ['system', 'light', 'dark'] as const;
export type Appearance = (typeof APPEARANCES)[number];

export class DisplayModel {
  appearance = $state<Appearance>('system');
  mayChange = $state(true);
  error = $state<ServiceError | null>(null);
  readonly #handle: BackendHandle;

  constructor(handle: BackendHandle) {
    this.#handle = handle;
  }

  #apply(view: SettingView): void {
    if (view.key !== APPEARANCE_KEY) return;
    if ((APPEARANCES as readonly string[]).includes(view.value as string)) this.appearance = view.value as Appearance;
    this.mayChange = view.person_may_change;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('settings.get', { section: 'display' });
    if (out.kind === 'ok') {
      for (const s of out.result.settings) this.#apply(s);
      this.error = null;
    } else this.error = out.error;
  }

  async choose(value: Appearance): Promise<void> {
    const out = await this.#handle.call('settings.change', { key: APPEARANCE_KEY, value });
    if (out.kind === 'ok') {
      this.#apply(out.result);
      this.error = null;
    } else this.error = out.error;
  }
}

/**
 * `settings-appearance` is one message: "Appearance: Same as this computer / Light / Dark". The label is the
 * text before the colon and the three choices follow, separated by " / ", in the order system, light, dark.
 */
export function splitAppearance(message: string): { label: string; options: [string, string, string] } {
  const at = message.indexOf(':');
  const label = at < 0 ? message : message.slice(0, at);
  const rest = (at < 0 ? '' : message.slice(at + 1)).split(' / ').map((s) => s.trim());
  return { label: label.trim(), options: [rest[0] ?? '', rest[1] ?? '', rest[2] ?? ''] };
}
