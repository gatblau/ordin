// The stand-ins a journey run uses in place of what is not on the test machine, or costs money:
//   - the model Claude Code talks to (stand-ins/model.mjs, 127.0.0.1:9339), scripted per journey;
//   - SharePoint, Confluence and Jira (stand-ins/services.mjs, 127.0.0.1:9440);
//   - Claude Code itself is the real one, started through a wrapper first on the test user's PATH that
//     gives it a key that is not real, the model stand-in's address and its own settings folder.
// Nothing a journey does leaves the computer.
import { execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { where } from './ordin';

const HERE = import.meta.dirname;
const PIDS = () => join(where.run, 'stand-ins.pids');
const MODEL_PORT = 9339;
const SERVICES_PORT = 9440;

/** A rule of the model stand-in: when the person's latest message (or an agent's brief) holds `when`. */
export type Rule = {
  who?: 'ordin' | 'agent';
  when: string;
  steps: ({ tool: string; input?: unknown } | { text: string; delay?: number })[];
};

/** What the model stand-in answers from now on. It reads the file on every request. */
export function script(rules: Rule[], defaults: { ordin?: string; agent?: string } = {}): void {
  writeFileSync(
    join(where.run, 'script.json'),
    JSON.stringify({ rules, default_ordin: defaults.ordin, default_agent: defaults.agent ?? 'Done.' }, null, 1),
  );
}

/** The model stand-in's log lines so far (one per request, and what Ordin's turn line said). */
export function modelLog(): string[] {
  const file = join(where.run, 'model.log');
  return existsSync(file) ? readFileSync(file, 'utf8').split('\n').filter(Boolean) : [];
}

/** Makes the next `count` page updates or SharePoint uploads fail, as a service that is down. */
export async function failNextSends(count: number): Promise<void> {
  await fetch(`http://127.0.0.1:${SERVICES_PORT}/__fail_puts?n=${count}`, { method: 'POST' });
}

/** A Confluence page as the stand-in holds it now. */
export function confluencePage(title: string): { v: number; body: string } | undefined {
  const file = join(where.run, 'services-pages.json');
  if (!existsSync(file)) return undefined;
  return (JSON.parse(readFileSync(file, 'utf8')).pages as { title: string; v: number; body: string }[]).find((p) => p.title === title);
}

function stopListeners(): void {
  // A stand-in left over from a run that was cut short still holds its port.
  execFileSync(
    'powershell',
    [
      '-NoProfile',
      '-Command',
      `Get-NetTCPConnection -State Listen -LocalPort ${MODEL_PORT},${SERVICES_PORT} -ErrorAction SilentlyContinue | ForEach-Object { Stop-Process -Id $_.OwningProcess -Force -ErrorAction SilentlyContinue }; exit 0`,
    ],
    { stdio: 'ignore' },
  );
}

async function waitFor(url: string, init?: RequestInit): Promise<void> {
  for (let i = 0; i < 40; i++) {
    try {
      await fetch(url, { ...init, signal: AbortSignal.timeout(1000) });
      return;
    } catch {
      await new Promise((r) => setTimeout(r, 250));
    }
  }
  throw new Error(`stand-in at ${url} did not start`);
}

/** Starts both stand-ins in the run folder. */
export async function startStandIns(): Promise<void> {
  stopListeners();
  const env = {
    ...process.env,
    STAND_IN_DIR: where.run,
    STAND_IN_FIND: [where.linkedCopies, where.sampleDocuments].join(';'),
  };
  const pids: number[] = [];
  for (const file of ['model.mjs', 'services.mjs']) {
    const child = spawn(process.execPath, [join(HERE, 'stand-ins', file)], { env, detached: true, stdio: 'ignore', windowsHide: true });
    child.unref();
    pids.push(child.pid ?? 0);
  }
  writeFileSync(PIDS(), pids.join('\n'));
  await waitFor(`http://127.0.0.1:${MODEL_PORT}/v1/messages`, { method: 'POST', body: '{"model":"x","stream":false}' });
  await waitFor(`http://127.0.0.1:${SERVICES_PORT}/rest/api/3/myself`);
}

export function stopStandIns(): void {
  if (!existsSync(PIDS())) return;
  for (const pid of readFileSync(PIDS(), 'utf8').split('\n').map(Number).filter(Boolean)) {
    try {
      process.kill(pid);
    } catch {
      // already gone
    }
  }
}

/**
 * Where the program finds the services' stand-ins (DSK-JRN-4): written into the data root before the
 * program starts. The SharePoint entry also signs a Microsoft account in against the token stand-in.
 */
export function writeStandInsFile(): void {
  const at = `http://127.0.0.1:${SERVICES_PORT}`;
  writeFileSync(
    join(where.dataRoot, 'journeys-stand-ins.json'),
    JSON.stringify({
      confluence: { gateway: at, site: 'https://acme.atlassian.net', token: 'confluence-journeys-token' },
      sharepoint: { graph: `${at}/graph/v1.0`, token_endpoint: `${at}/microsoft/oauth2/v2.0/token`, client_id: 'ordin-journeys' },
      jira: { site_base_url: at, gateway_base_url: at },
    }),
  );
}

/**
 * Puts the Claude Code wrapper first on the test user's PATH. The program finds `claude` on the PATH it
 * starts with, so the wrapper must be in place before the program starts.
 */
export function installClaudeWrapper(): void {
  const real = process.env.ORDIN_JOURNEYS_CLAUDE ?? join(process.env.APPDATA ?? '', 'npm', 'claude.cmd');
  if (!existsSync(real)) throw new Error(`no Claude Code at ${real}: install it with npm, or set ORDIN_JOURNEYS_CLAUDE`);
  const bin = resolve(process.env.USERPROFILE ?? '', 'ordin-journeys-bin');
  mkdirSync(bin, { recursive: true });
  const home = join(where.run, 'claude-home');
  mkdirSync(home, { recursive: true });
  writeFileSync(
    join(bin, 'claude.cmd'),
    [
      '@echo off',
      'rem The journeys\' Claude Code: a key that is not real, the model stand-in, its own settings folder.',
      'set ANTHROPIC_API_KEY=sk-ant-not-a-real-key',
      `set ANTHROPIC_BASE_URL=http://127.0.0.1:${MODEL_PORT}`,
      `set CLAUDE_CONFIG_DIR=${home}`,
      'set DISABLE_AUTOUPDATER=1',
      'set CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1',
      'set DISABLE_TELEMETRY=1',
      `"${real}" %*`,
      '',
    ].join('\r\n'),
  );
  execFileSync(
    'powershell',
    [
      '-NoProfile',
      '-Command',
      `$bin='${bin}'; $p=[Environment]::GetEnvironmentVariable('PATH','User'); $rest=@(($p -split ';') | Where-Object { $_ -and $_ -ne $bin }); [Environment]::SetEnvironmentVariable('PATH', (@($bin) + $rest) -join ';', 'User')`,
    ],
    { stdio: 'ignore' },
  );
}
