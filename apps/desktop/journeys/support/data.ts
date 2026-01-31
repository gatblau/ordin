// The saved data a newer or different ordin would leave behind (OrdinDesktop.md DSK-DATA-1 to -3): the
// journey changes the journal's own record of its set-up steps while the program is stopped, then answers
// the message box the program shows at its next start.
import { DatabaseSync } from 'node:sqlite';
import { existsSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { runInDesktop, sleep, where } from './ordin';

function journal(): DatabaseSync {
  return new DatabaseSync(join(where.dataRoot, 'journal.db'));
}

/** One applied step's fingerprint no longer matches the program's: "set up by a different version". */
export function changeAnAppliedStep(): void {
  const db = journal();
  db.exec(`UPDATE schema_migrations SET checksum = '${'0'.repeat(64)}' WHERE rowid = (SELECT MIN(rowid) FROM schema_migrations)`);
  db.close();
}

/** A step this program does not know, as a newer ordin would have applied: "comes from a newer version". */
export function addAStepFromANewerOrdin(): void {
  const db = journal();
  const row = db.prepare('SELECT module, MAX(version) AS version FROM schema_migrations GROUP BY module ORDER BY module LIMIT 1').get() as { module: string; version: number };
  db.prepare('INSERT INTO schema_migrations (module, version, description, checksum, applied_at) VALUES (?, ?, ?, ?, ?)').run(
    row.module,
    row.version + 1,
    'a step of a newer ordin',
    'f'.repeat(64),
    new Date().toISOString(),
  );
  db.close();
}

/** Presses `button` on the program's message box and returns every text the box showed. */
export async function answerMessageBox(button: string): Promise<string> {
  const out = join(where.run, 'message-box.txt');
  rmSync(out, { force: true });
  const script = join(import.meta.dirname, 'message-box.ps1');
  runInDesktop('ordin-journeys-box', `powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "${script}" -Button "${button}" -Out "${out}"`);
  for (let i = 0; i < 300 && !existsSync(out); i++) await sleep(500);
  await sleep(500);
  return existsSync(out) ? readFileSync(out, 'utf8') : 'the message box script wrote nothing';
}
