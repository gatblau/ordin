// Before the first journey: a fresh data root, a fresh run folder with its own copy of the sample
// documents, the Claude Code wrapper, the stand-ins. The program itself is started by the first journey
// that needs it (support/test.ts), so "getting ready" sees the very first start.
import { cpSync, mkdirSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { freshDataRoot, stopOrdin, where } from './ordin';
import { installClaudeWrapper, startStandIns, stopStandIns, writeStandInsFile } from './stand-ins';

export default async function globalSetup(): Promise<void> {
  if (process.platform !== 'win32') throw new Error('the journeys drive the program on Windows only (OrdinDesktop.md DSK-JRN-1); see release/checklist.md');
  await stopOrdin();
  stopStandIns();
  rmSync(where.run, { recursive: true, force: true });
  mkdirSync(where.run, { recursive: true });
  cpSync(join(import.meta.dirname, '..', 'fixtures', 'Sample documents'), where.sampleDocuments, { recursive: true });
  freshDataRoot();
  writeStandInsFile();
  installClaudeWrapper();
  await startStandIns();
}
