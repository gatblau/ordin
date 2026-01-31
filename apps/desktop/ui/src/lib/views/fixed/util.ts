// Small helpers the fixed and engineering views share: an outcome unwrapped into a plain result, the
// announcement a polite live region draws, and the app context when there is one (tests render a
// component with none).
import type { BackendHandle, CommandOutcome, ServiceError } from '../../backend/backend';
import { isNotWired, type NotWired } from '../../backend/wiring';
import { getApp, type AppContext } from '../../shell/context';

export type Settled<T> = { ok: true; value: T } | { ok: false; error: ServiceError } | { ok: false; notWired: true };

/** Turns `CommandOutcome<T> | NotWired` into one shape a view can branch on. */
export function settle<T>(outcome: CommandOutcome<T> | NotWired): Settled<T> {
  if (isNotWired(outcome)) return { ok: false, notWired: true };
  return outcome.kind === 'ok' ? { ok: true, value: outcome.result } : { ok: false, error: outcome.error };
}

export function failed<T>(s: Settled<T>): s is Exclude<Settled<T>, { ok: true }> {
  return !s.ok;
}

export function errorOf<T>(s: Settled<T>): ServiceError | null {
  return !s.ok && 'error' in s ? s.error : null;
}

/** The app context, or `null` when a component is drawn with none (a test, a preview). */
export function appOrNull(): AppContext | null {
  try {
    return getApp();
  } catch {
    return null;
  }
}

export { Announcer } from './announcer.svelte';

export type Handle = Pick<BackendHandle, 'call' | 'id' | 'kind'>;

/** A file name from a path: the last segment. */
export function lastSegment(path: string): string {
  const parts = path.split(/[\\/]/).filter((p) => p !== '');
  return parts.length === 0 ? path : (parts[parts.length - 1] as string);
}
