// What journey 13 needs beyond the other journeys (phase-5-playbook.md PB-MIN-2): whether this build has
// the `minutes` switch, the transcript and recording fixtures, the speech-model folder, and the fake OBS of
// Step 81 (`cargo run -p ordin-obs --features fake --example fake-obs`) serving OBS's own port.
import { spawn, type ChildProcess } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { where } from './ordin';

const env = (name: string, fallback: string) => process.env[name] ?? fallback;

/** True when the program under test was built with `minutes` (scripts/journeys-windows.sh sets it). */
export const minutesBuild = process.env.ORDIN_JOURNEYS_MINUTES === '1';

const REPO = resolve(import.meta.dirname, '..', '..', '..', '..');
const FIXTURES = join(REPO, 'crates', 'ordin-dictation', 'tests', 'fixtures');

/** A fixture of the Transcription Service (`meeting.vtt`, `speech-60s.m4a`, `two-track.mkv`). */
export const fixture = (name: string): string => join(FIXTURES, name);

/** Where the program keeps the speech model: `<data root>/speech-models` (DSK-MIN-1, row 3.16a). */
export const speechModels = (): string => join(where.dataRoot, 'speech-models');

/** The files in the speech-model folder, a download's `.part` included. */
export function speechModelFiles(): string[] {
  return existsSync(speechModels()) ? readdirSync(speechModels()) : [];
}

/**
 * Puts the larger model in place, as a person who had downloaded it would have it. The file is a copy of
 * the one in `speech-models.json`, kept on the test machine (ORDIN_JOURNEYS_SPEECH_MODEL).
 */
export function placeLargerModel(): void {
  const source = env('ORDIN_JOURNEYS_SPEECH_MODEL', 'C:\\ordin-test-models\\ggml-medium.bin');
  if (!existsSync(source)) throw new Error(`no speech model at ${source}: copy ggml-medium.bin there, or set ORDIN_JOURNEYS_SPEECH_MODEL`);
  mkdirSync(speechModels(), { recursive: true });
  copyFileSync(source, join(speechModels(), 'ggml-medium.bin'));
}

const FAKE_OBS_PORT = 4455;

/** The fake OBS on 127.0.0.1:4455; StopRecord answers with a copy of `two-track.mkv`. */
export function startFakeObs(): { stop: () => void } {
  const exe = env('ORDIN_JOURNEYS_FAKE_OBS', join(REPO, 'target', 'release', 'examples', 'fake-obs.exe'));
  if (!existsSync(exe)) throw new Error(`no fake OBS at ${exe}: build it with cargo build --release -p ordin-obs --features fake --example fake-obs`);
  const folder = join(where.run, 'obs-recordings');
  mkdirSync(folder, { recursive: true });
  const output = join(folder, 'meeting.mkv');
  copyFileSync(fixture('two-track.mkv'), output);
  const child: ChildProcess = spawn(exe, [String(FAKE_OBS_PORT), output], { stdio: 'ignore', windowsHide: true });
  return { stop: () => void child.kill() };
}
