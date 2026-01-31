// The program under test, on the Windows build machine: where its files are, how a journey starts and
// stops it, and how Playwright reaches its window.
//
// The program is the `journeys` build (OrdinDesktop.md DSK-JRN-1): its window opens WebView2's debugging
// port on 127.0.0.1:9222, and Playwright attaches to the real window over it (`connectOverCDP`). The
// runner works over SSH, outside the person's desktop, so the program is started through a scheduled
// task that runs in the signed-in desktop session (`/it`), as a person starting it would.
import { chromium, type Browser, type Page } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';

const env = (name: string, fallback: string) => process.env[name] ?? fallback;

/** Where everything is. Each can be changed through the environment of the run. */
export const where = {
  /** The program, built with `--features journeys`. */
  exe: env('ORDIN_JOURNEYS_EXE', 'C:\\o\\target\\release\\ordin.exe'),
  /** The program's data root (Tauri's local data folder for `org.ordin.desktop`). */
  dataRoot: join(env('LOCALAPPDATA', ''), 'org.ordin.desktop'),
  /** What this run writes: the stand-ins' state and logs, screenshots, the report. */
  run: resolve(env('ORDIN_JOURNEYS_RUN', 'run')),
  /** Where the linked copies of online documents are made (WorkspaceService, `LINKED_COPIES_DIR`). */
  linkedCopies: join(env('USERPROFILE', ''), 'Documents', 'ordin', 'Linked copies'),
  /** The sample documents of this run: a copy of `fixtures/Sample documents`. */
  get sampleDocuments() {
    return join(this.run, 'Sample documents');
  },
  debuggingPort: 'http://127.0.0.1:9222',
};

/** The marker a run leaves in the data root it made, so a later run may delete that data root. */
const RUN_MARKER = 'journeys-run.txt';
const TASK = 'ordin-journeys';

function powershell(script: string): string {
  return execFileSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', script], { encoding: 'utf8' });
}

/** Runs `command` in the signed-in desktop session through a one-off scheduled task, and returns at once. */
export function runInDesktop(name: string, command: string): void {
  const user = env('USERNAME', 'ordin');
  execFileSync('schtasks', ['/create', '/tn', name, '/tr', command, '/sc', 'once', '/st', '23:59', '/it', '/ru', user, '/f'], { stdio: 'ignore' });
  execFileSync('schtasks', ['/run', '/tn', name], { stdio: 'ignore' });
}

export function isRunning(): boolean {
  return powershell('@(Get-Process ordin -ErrorAction SilentlyContinue).Count').trim() !== '0';
}

/** Stops the program at once, as a power cut or Task Manager would, and waits until it has gone. */
export async function stopOrdin(): Promise<void> {
  powershell('Get-Process ordin -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue; exit 0');
  for (let i = 0; i < 60 && (isRunning() || (await portOpen())); i++) await sleep(500);
}

/** Starts the program in the desktop session. It does not wait for the window. */
export function startOrdin(): void {
  if (!existsSync(where.exe)) throw new Error(`no program at ${where.exe}: build it with --features journeys first`);
  runInDesktop(TASK, `"${where.exe}"`);
}

export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function portOpen(): Promise<boolean> {
  try {
    const res = await fetch(`${where.debuggingPort}/json/version`, { signal: AbortSignal.timeout(2000) });
    return res.ok;
  } catch {
    return false;
  }
}

/** Waits for the window's debugging port and attaches Playwright to the program's one page. */
export async function attach(timeoutMs = 120_000): Promise<{ browser: Browser; page: Page }> {
  const until = Date.now() + timeoutMs;
  while (!(await portOpen())) {
    if (Date.now() > until) throw new Error(`the window did not open its debugging port within ${timeoutMs / 1000} s`);
    await sleep(500);
  }
  const browser = await chromium.connectOverCDP(where.debuggingPort, { timeout: 30_000 });
  for (;;) {
    const page = browser.contexts().flatMap((c) => c.pages()).find((p) => !p.url().startsWith('devtools:'));
    if (page) return { browser, page };
    if (Date.now() > until) throw new Error('the window has no page');
    await sleep(250);
  }
}

/**
 * Whether a data root was made by a journey run: it holds the run's marker, or the program set the run's
 * data aside inside it (journey 09's Start with empty data moves the marker into a `set-aside-…` folder).
 */
function madeByJourneys(root: string): boolean {
  if (existsSync(join(root, RUN_MARKER))) return true;
  return readdirSync(root).some((n) => n.startsWith('set-aside-') && existsSync(join(root, n, RUN_MARKER)));
}

/**
 * Moves the data root aside so the program starts as on a first start. A data root an earlier journey run
 * made is deleted; any other (a person's own data on the test machine) is renamed and kept.
 */
export function freshDataRoot(): void {
  const parent = resolve(where.dataRoot, '..');
  const base = basename(where.dataRoot);
  for (const name of readdirSync(parent)) {
    const path = join(parent, name);
    // A `.kept-` root is one an earlier version of this check took for a person's data.
    if ((name.startsWith(`${base}.journeys-`) || name.startsWith(`${base}.kept-`)) && madeByJourneys(path)) rmSync(path, { recursive: true, force: true });
  }
  if (existsSync(where.dataRoot)) {
    const stamp = new Date().toISOString().replace(/[-:]/g, '').replace(/\..*/, '');
    const ours = madeByJourneys(where.dataRoot);
    renameSync(where.dataRoot, `${where.dataRoot}.${ours ? 'journeys' : 'kept'}-${stamp}`);
  }
  mkdirSync(where.dataRoot, { recursive: true });
  writeFileSync(join(where.dataRoot, RUN_MARKER), `made by a journey run at ${new Date().toISOString()}\n`);
}

/**
 * The answer to the next operating-system dialog the program opens (DSK-JRN-3): the program reads and
 * deletes the file in place of showing the folder picker, the certificate picker or the save dialog.
 */
export function answerNextDialog(path: string): void {
  writeFileSync(join(where.dataRoot, 'journeys-dialog-answer.txt'), path);
}
