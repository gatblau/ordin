// Check it (11 §5.30, UX-A63, SA-CLOSE-01): the Checker's report for an installed agent or a library
// part, in place. The panel keeps the last `check_run_id` for the open page and reopens it with
// `checker.report`; a failure finding is drawn before warnings.
//
// spec: ordin-desktop/components/ScreensModule#SA-CLOSE-01
import type { CheckReport, Finding, PartKind, ServiceError } from '../../service-api/types.gen';
import type { Handle } from './util';

export type CheckSubjectRef =
  | { kind: 'installed'; agentInstanceId: string }
  | { kind: 'library'; agentRepositoryId?: string | null; partKind: PartKind; identifier: string };

/** The last check run of each page, so reopening the page shows the earlier result again. */
const lastRuns = new Map<string, string>();
export function forgetCheckRuns(): void {
  lastRuns.clear();
}
export function pageKey(subject: CheckSubjectRef): string {
  return subject.kind === 'installed' ? `installed:${subject.agentInstanceId}` : `library:${subject.agentRepositoryId ?? ''}:${subject.partKind}:${subject.identifier}`;
}

/** Failures before warnings, each group in the order the Checker gave. */
export function orderedFindings(findings: readonly Finding[]): Finding[] {
  return [...findings.filter((f) => f.severity === 'failure'), ...findings.filter((f) => f.severity !== 'failure')];
}

/**
 * The part page a finding opens (SM-NS-15): `part-page:<agent instance id>:<part id>` for a finding of an
 * installed agent that names a part other than the one the page is already about. A finding of a library
 * part names that part, so it opens nothing.
 *
 * spec: ordin-desktop/components/ScreensModule#SM-NS-15
 */
export function partPageKeyFor(subject: CheckSubjectRef, finding: Finding, currentPart: string | null = null): string | null {
  const part = finding.part_identifier;
  if (subject.kind !== 'installed' || part == null || part === '' || part === currentPart) return null;
  return `part-page:${subject.agentInstanceId}:${part}`;
}

export class CheckItController {
  report = $state.raw<CheckReport | null>(null);
  running = $state(false);
  error = $state.raw<ServiceError | null>(null);
  readonly #handle: Handle;
  readonly subject: CheckSubjectRef;

  constructor(handle: Handle, subject: CheckSubjectRef) {
    this.#handle = handle;
    this.subject = subject;
  }

  /** Reopens the last result of this page, when there is one. */
  async reopen(): Promise<void> {
    const id = lastRuns.get(pageKey(this.subject));
    if (id === undefined) return;
    const out = await this.#handle.call('checker.report', { check_run_id: id });
    if (out.kind === 'ok') this.report = out.result;
  }

  async check(): Promise<void> {
    if (this.running) return;
    this.running = true;
    this.error = null;
    const s = this.subject;
    const out =
      s.kind === 'installed'
        ? await this.#handle.call('checker.check_installed', { agent_instance_id: s.agentInstanceId })
        : await this.#handle.call('checker.check_library_item', {
            ...(s.agentRepositoryId == null ? {} : { agent_repository_id: s.agentRepositoryId }),
            kind: s.partKind,
            identifier: s.identifier,
          });
    this.running = false;
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    this.report = out.result;
    lastRuns.set(pageKey(s), out.result.check_run_id);
  }
}
