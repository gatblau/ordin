// First start's scenes for the side-by-side review (cross-cutting/VisualDesign.md, Internal Logic step 7;
// ScreensModule.md SM-FS-1 to SM-FS-4): `setup` (signed out, the sign-in button), `setupReady` (already
// signed in, part-way through) and `noseat` (no ready harness: no Claude access, Ask IT). Sample data is
// illustrative (12 §2).
import type { ProgressStep, StepState } from '../src/lib/service-api/types.gen';
import type { FakeTransport, ok as okFn } from '../tests/support/fake-backend';
import type { Seed } from './scenes';

type Ok = typeof okFn;

const step = (check: ProgressStep['check'], state: StepState): ProgressStep => ({ check, state, message_id: `setup-step-${check.replaceAll('_', '-')}` });
const progress = (steps: ProgressStep[]) => ({ done: steps.filter((s) => s.state === 'done').length, total: steps.length, running: true, steps });
const claude = (state: string) => ({
  harness: 'claude_code', id: '0192f3a0-0000-7000-8000-000000000001', last_checked_at: '2026-01-28T09:00:00.000Z', location: '/usr/local/bin/claude',
  signed_in: state === 'ready', state, tested: true, version: '2.1.0',
});
/** The guided run still running: it never answers while the scene is captured. */
const running = (t: FakeTransport): void => void t.on('setup.run_guided', () => new Promise(() => undefined));

const setup: Seed = (t: FakeTransport, _local: string, ok: Ok) => {
  const steps = [step('company_settings', 'done'), step('harness', 'done'), step('harness_sign_in', 'needs_you'), step('agent_repositories', 'waiting')];
  t.on('setup.status', ok({ needs_guided: true, progress: progress(steps), attachments: [claude('needs_sign_in')] }));
  running(t);
};

const setupReady: Seed = (t: FakeTransport, _local: string, ok: Ok) => {
  const steps = [step('company_settings', 'done'), step('harness', 'done'), step('harness_sign_in', 'done'), step('agent_repositories', 'running')];
  t.on('setup.status', ok({ needs_guided: true, progress: progress(steps), attachments: [claude('ready')] }));
  running(t);
};

const noseat: Seed = (t: FakeTransport, _local: string, ok: Ok) => {
  const steps = [step('company_settings', 'done'), step('harness', 'done'), step('harness_sign_in', 'failed'), step('agent_repositories', 'waiting')];
  t.on('setup.status', ok({ needs_guided: true, progress: progress(steps), attachments: [claude('checked')] }));
  t.on('setup.run_guided', ok({
    started_at: '2026-01-28T09:00:00.000Z', ended_at: '2026-01-28T09:00:04.000Z', ready: false,
    results: [{ check: 'harness_sign_in', checked_at: '2026-01-28T09:00:04.000Z', outcome: 'failed', code: 'SETUP_NO_HARNESS_ACCESS', message_id: 'setup-no-harness-access', message_args: {}, action: 'ask_it_for_claude_access' }],
  }));
};

export const SCENES: Record<string, Seed> = { setup, setupReady, noseat };
