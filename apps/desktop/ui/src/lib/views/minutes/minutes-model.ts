// The pure rules of the minutes screens (ScreensModule.md SM-MIN-2, SM-MIN-5 to SM-MIN-7): the stage line of
// a job, its progress, which jobs are still being worked on, whether Record can be pressed, and the time a
// recording has run. No component state here, so the tests check each rule on its own.
//
// spec: ordin-desktop/components/ScreensModule#SM-MIN-2
import type { MinutesJob, MinutesJobState, ObsState } from '../../service-api/minutes.gen';

/** A message id and its arguments. */
export interface Line {
  id: string;
  args?: Record<string, string | number>;
}

/** The plain stage line of a job (SM-MIN-2). A failed job carries its code's sentence, which is drawn instead (`null`). */
// spec: ordin-desktop/components/ScreensModule#SM-MIN-2
export function stageLine(state: MinutesJobState): Line | null {
  switch (state.state) {
    case 'downloading_model':
      return { id: 'minutes-stage-downloading-model', args: { received: state.received_mb, total: state.total_mb } };
    case 'transcribing':
      return { id: 'minutes-stage-transcribing', args: { done: Math.floor(state.done_ms / 60_000), total: Math.ceil(state.total_ms / 60_000) } };
    case 'failed':
      return null;
    default:
      // Message ids are kebab-case, so `waiting_for_model` is `minutes-stage-waiting-for-model`.
      return { id: `minutes-stage-${state.state.replace(/_/g, '-')}` };
  }
}

/** The progress bar's value and maximum, when the state has numbers. */
export function progressOf(state: MinutesJobState): { value: number; max: number } | null {
  if (state.state === 'downloading_model' && state.total_mb > 0) return { value: state.received_mb, max: state.total_mb };
  if (state.state === 'transcribing' && state.total_ms > 0) return { value: state.done_ms, max: state.total_ms };
  return null;
}

/** A job the **In progress** list shows: every one not `done`, `transcript_ready` or `cancelled` (SM-MIN-11:
 * text ready for Ordin counts as finished). */
// spec: ordin-desktop/components/ScreensModule#SM-MIN-11
export function inProgress(job: MinutesJob): boolean {
  return job.state.state !== 'done' && job.state.state !== 'transcript_ready' && job.state.state !== 'cancelled';
}

/** A job that turns a recording into text for Ordin, not minutes (SM-MIN-11). */
// spec: ordin-desktop/components/ScreensModule#SM-MIN-11
export function forOrdin(job: MinutesJob): boolean {
  return job.purpose === 'transcript';
}

/** A job still moving, which keeps the screen reading every 2 seconds. */
export function moving(job: MinutesJob): boolean {
  return inProgress(job) && job.state.state !== 'failed';
}

/** **Record** is enabled only when OBS is ready (SM-MIN-5, SM-MIN-6). */
// spec: ordin-desktop/components/ScreensModule#SM-MIN-5
export function canRecord(recorder: ObsState | null | undefined): boolean {
  return recorder?.state === 'ready';
}

/** Minutes and seconds since `startedAt`, "12:04" (SM-MIN-7). */
// spec: ordin-desktop/components/ScreensModule#SM-MIN-7
export function elapsed(startedAt: string, now: number): string {
  const seconds = Math.max(0, Math.floor((now - Date.parse(startedAt)) / 1000));
  const minutes = Math.floor(seconds / 60);
  return `${String(minutes).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`;
}

/** The poll intervals of SM-MIN-2 (jobs) and SM-MIN-7 (the indicator). */
export const JOBS_POLL_MS = 2_000;
export const RECORDING_POLL_MS = 5_000;
