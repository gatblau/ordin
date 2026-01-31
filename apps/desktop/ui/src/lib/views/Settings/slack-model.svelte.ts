// Settings › Slack (ADR-102): the fixed sentence first, the sign-in line, **Connect your own Slack app…**
// when the profile names no Slack app, **Posted without asking** with **Turn off** per rule, and the
// more-permissions line.
//
// spec: ordin-desktop/components/ScreensModule#SM-UP-1
import type { BackendHandle } from '../../backend/backend';
import type { ServiceError, SlackRuleView, SlackStatus } from '../../service-api/types.gen';
import { SignInsModel } from './signins-model.svelte';

export class SlackModel {
  status = $state.raw<SlackStatus | null>(null);
  rules = $state.raw<SlackRuleView[]>([]);
  error = $state<ServiceError | null>(null);
  ownAppOpen = $state(false);
  clientId = $state('');
  readonly #handle: BackendHandle;
  readonly #signIns: SignInsModel;

  constructor(handle: BackendHandle, signIns: SignInsModel = new SignInsModel(handle)) {
    this.#handle = handle;
    this.#signIns = signIns;
  }

  async load(): Promise<void> {
    const status = await this.#handle.call('slack.status', {});
    if (status.kind === 'error') {
      this.error = status.error;
      return;
    }
    this.status = status.result;
    this.error = null;
    if (status.result.rules_allowed) {
      const rules = await this.#handle.call('slack.rules', {});
      if (rules.kind === 'ok') this.rules = rules.result;
      else this.error = rules.error;
    } else this.rules = [];
  }

  /** The rules that can be turned off. */
  get activeRules(): SlackRuleView[] {
    return this.rules.filter((r) => r.rule.state === 'on');
  }

  /** **Sign in to Slack…** and the **Allow…** of the more-permissions line: `identity.sign_in { kind: slack }`. */
  async signIn(): Promise<void> {
    const out = await this.#signIns.signIn('slack');
    if (out === 'error') this.error = this.#signIns.error;
    await this.load();
  }

  /** **Sign out**: `identity.sign_out` of the Slack credential `identity.list` returns. */
  async signOut(): Promise<void> {
    await this.#signIns.load();
    const credential = this.#signIns.items.find((c) => c.kind === 'slack' && c.state === 'signed_in');
    if (credential !== undefined) await this.#signIns.signOut(credential.id);
    await this.load();
  }

  async turnOff(ruleId: string): Promise<void> {
    const out = await this.#handle.call('slack.turn_off_rule', { rule_id: ruleId });
    if (out.kind === 'error') this.error = out.error;
    await this.load();
  }

  async connectOwnApp(): Promise<boolean> {
    const clientId = this.clientId.trim();
    if (clientId === '') return false;
    const out = await this.#handle.call('slack.connect_own_app', { client_id: clientId });
    if (out.kind === 'ok') {
      this.status = out.result;
      this.ownAppOpen = false;
      this.clientId = '';
      await this.load();
      return true;
    }
    this.error = out.error;
    return false;
  }
}
