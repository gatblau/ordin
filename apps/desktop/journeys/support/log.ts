// The program's log, read as a journey goes: every line at WARN or ERROR a journey did not expect fails it.
//
// The log is JSON lines in `<data root>/logs/`, one object per line with `level`, `target` and
// `fields.message` (the event's name, such as `journal.migrate.refused`). A journey may expect some
// messages: those every journey may meet are in `expected-log.txt` with a reason each, and a journey adds
// its own with `expectLog`.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { where } from './ordin';

export type LogLine = { level: string; target?: string; fields?: Record<string, unknown> & { message?: string } };

/** `expected-log.txt`: `<message> <reason>` per line; `#` starts a comment. Each line needs a reason. */
export function expectedEverywhere(): string[] {
  const text = readFileSync(resolve(import.meta.dirname, '..', 'expected-log.txt'), 'utf8');
  return text
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => l !== '' && !l.startsWith('#'))
    .map((l) => {
      const [message, ...reason] = l.split(/\s+/);
      if (reason.length === 0) throw new Error(`expected-log.txt: "${l}" needs a reason`);
      return message;
    });
}

/** Reads the log files from where it last stopped; a restart's new file is read from its start. */
export class LogWatch {
  private seen = new Map<string, number>();

  /** Skips everything written so far. */
  skip(): void {
    for (const [file, lines] of this.files()) this.seen.set(file, lines.length);
  }

  /** The lines written since the last call. */
  read(): LogLine[] {
    const out: LogLine[] = [];
    for (const [file, lines] of this.files()) {
      let at = this.seen.get(file) ?? 0;
      for (; at < lines.length; at++) {
        try {
          out.push(JSON.parse(lines[at]) as LogLine);
        } catch {
          // A line still being written; it is read whole next time.
          break;
        }
      }
      this.seen.set(file, at);
    }
    return out;
  }

  private files(): [string, string[]][] {
    const dir = join(where.dataRoot, 'logs');
    if (!existsSync(dir)) return [];
    return readdirSync(dir)
      .sort()
      .map((n) => join(dir, n))
      .map((p) => [p, readFileSync(p, 'utf8').split('\n').filter((l) => l.trim() !== '')] as [string, string[]]);
  }
}

export const isProblem = (l: LogLine) => l.level === 'WARN' || l.level === 'ERROR';
export const describe = (l: LogLine) => `${l.level} ${l.target ?? ''} ${JSON.stringify(l.fields ?? {})}`.slice(0, 400);
