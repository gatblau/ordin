// The handover screen's state (SM-NS-8): the screen `handover.open` returns, the person's workspaces for
// *Accept and start*, the return options, and the four commands the buttons send. A refused command keeps
// the screen and shows the owner's sentence; `HAND_ALREADY_ACCEPTED` also re-reads the screen.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-8
import type { HandoverScreen, ReturnOption, ServiceError, Workspace } from '../../service-api/types.gen';
import type { Handle } from '../fixed/util';

/** The sender's side (SA-UI-CU-06): neither `accept_and_start` nor `return_to` is offered, and the handover waits. */
// spec: ordin-desktop/components/ScreensModule#SA-UI-CU-06
export function isSendersSide(screen: HandoverScreen): boolean {
  return !screen.actions.includes('accept_and_start') && !screen.actions.includes('return_to') && (screen.handover.state === 'sent' || screen.handover.state === 'seen');
}

/** The Fluent id of a handover state: `sent_back` is `handover-state-sent-back`. */
export function stateMessageId(state: string): string {
  return `handover-state-${state.replace(/_/g, '-')}`;
}

export class HandoverController {
  screen = $state.raw<HandoverScreen | null>(null);
  error = $state.raw<ServiceError | null>(null);
  workspaces = $state.raw<Workspace[]>([]);
  options = $state.raw<ReturnOption[]>([]);
  busy = $state(false);
  readonly #handle: Handle;
  readonly #id: string;
  readonly #openWorkspace: (workspaceId: string) => void;

  constructor(handle: Handle, id: string, openWorkspace: (workspaceId: string) => void = () => undefined) {
    this.#handle = handle;
    this.#id = id;
    this.#openWorkspace = openWorkspace;
  }

  /** Reads the handover; a refusal leaves the earlier screen (if any) and shows its sentence. */
  async load(): Promise<void> {
    const out = await this.#handle.call('handover.open', { id: this.#id });
    if (out.kind === 'ok') {
      this.screen = out.result;
      this.error = null;
    } else {
      this.error = out.error;
    }
  }

  /** The person's workspaces the handover can be accepted into. */
  async loadWorkspaces(): Promise<void> {
    const out = await this.#handle.call('workspaces.list', { filter: 'active' });
    if (out.kind === 'ok') this.workspaces = out.result;
    else this.error = out.error;
  }

  /** `into` is a new workspace named from the title, or one the person has. The returned workspace opens. */
  async accept(into: { kind: 'new_workspace'; name: string } | { kind: 'existing'; workspace_id: string }): Promise<boolean> {
    if (this.busy) return false;
    this.busy = true;
    const out = await this.#handle.call('handover.accept_and_start', { id: this.#id, into });
    this.busy = false;
    if (out.kind === 'error') {
      await this.#refused(out.error);
      return false;
    }
    this.error = null;
    this.#openWorkspace(out.result);
    return true;
  }

  async loadReturnOptions(): Promise<void> {
    const out = await this.#handle.call('handover.return_options', { id: this.#id });
    if (out.kind === 'ok') this.options = out.result;
    else this.error = out.error;
  }

  /** Returns the handover; the reason is required, so a blank one sends nothing. */
  async returnTo(reason: string, to: ReturnOption): Promise<boolean> {
    if (this.busy || reason.trim() === '') return false;
    this.busy = true;
    const out = await this.#handle.call('handover.return_to', { id: this.#id, reason: reason.trim(), to });
    this.busy = false;
    if (out.kind === 'error') {
      await this.#refused(out.error);
      return false;
    }
    this.error = null;
    await this.load();
    return true;
  }

  async takeBack(): Promise<boolean> {
    return this.#sender('handover.take_back');
  }

  async remind(): Promise<boolean> {
    return this.#sender('handover.remind');
  }

  async #sender(command: 'handover.take_back' | 'handover.remind'): Promise<boolean> {
    if (this.busy) return false;
    this.busy = true;
    const out = await this.#handle.call(command, { id: this.#id });
    this.busy = false;
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.error = null;
    await this.load();
    return true;
  }

  async #refused(error: ServiceError): Promise<void> {
    if (error.code === 'HAND_ALREADY_ACCEPTED') {
      await this.load();
    }
    this.error = error;
  }
}
