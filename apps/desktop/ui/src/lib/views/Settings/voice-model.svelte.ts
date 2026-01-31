// Settings › How Ordin talks to you (11 §5.19, UX-A34): tone, formality, length, the name to use and notes
// for Ordin. Each change sends `voice.update` with the one field changed; the control keeps the person's
// choice while the command is in flight and returns to the stored value on a refusal.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-1
import type { BackendHandle } from '../../backend/backend';
import type { ServiceError, VoiceLength, VoiceRegister, VoiceSettings, VoiceTone, VoiceUpdateInput } from '../../service-api/types.gen';

export const TONES: readonly VoiceTone[] = ['friendly', 'neutral', 'warm', 'direct'];
export const REGISTERS: readonly VoiceRegister[] = ['everyday', 'formal'];
export const LENGTHS: readonly VoiceLength[] = ['brief', 'shortest'];
/** The program's own default of each choice (11 §5.19, the prototype's "(default)" marks). */
export const DEFAULTS = { tone: 'friendly', register: 'everyday', length: 'brief' } as const;
/** Notes for Ordin hold at most 300 characters. */
export const NOTES_MAX = 300;

export class VoiceModel {
  settings = $state.raw<VoiceSettings | null>(null);
  /** The text boxes, saved on leaving them. */
  nameToUse = $state('');
  notes = $state('');
  error = $state<ServiceError | null>(null);
  /** The box last saved on leaving it, for its "Saved" line. */
  saved = $state<'name' | 'notes' | null>(null);
  readonly #handle: BackendHandle;

  constructor(handle: BackendHandle) {
    this.#handle = handle;
  }

  #sync(settings: VoiceSettings): void {
    this.settings = settings;
    this.nameToUse = settings.name_to_use.value ?? '';
    this.notes = settings.notes.value ?? '';
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('voice.get', {});
    if (out.kind === 'ok') {
      this.#sync(out.result);
      this.error = null;
    } else this.error = out.error;
  }

  /** Sends one field; the shown value is the person's choice until the reply, and the stored one after a refusal. */
  async update(change: VoiceUpdateInput): Promise<boolean> {
    const stored = this.settings;
    if (stored === null) return false;
    this.settings = {
      ...stored,
      ...(change.tone != null ? { tone: { ...stored.tone, value: change.tone } } : {}),
      ...(change.register != null ? { register: { ...stored.register, value: change.register } } : {}),
      ...(change.length != null ? { length: { ...stored.length, value: change.length } } : {}),
    };
    const out = await this.#handle.call('voice.update', change);
    if (out.kind === 'ok') {
      this.#sync(out.result);
      this.error = null;
      return true;
    }
    this.#sync(stored);
    this.error = out.error;
    return false;
  }

  /** **Call me**, saved on leaving the box; an empty box sends `""`. */
  async saveName(): Promise<boolean> {
    if (this.nameToUse === (this.settings?.name_to_use.value ?? '')) return true;
    this.saved = null;
    const ok = await this.update({ name_to_use: this.nameToUse });
    if (ok) this.saved = 'name';
    return ok;
  }

  async saveNotes(): Promise<boolean> {
    const text = [...this.notes].slice(0, NOTES_MAX).join('');
    if (text === (this.settings?.notes.value ?? '')) return true;
    this.saved = null;
    const ok = await this.update({ notes: text });
    if (ok) this.saved = 'notes';
    return ok;
  }
}
