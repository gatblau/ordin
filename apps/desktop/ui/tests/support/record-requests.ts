// Plan 04, safety net A: after each test file, writes every request its fake backends were sent to
// `$ORDIN_UI_REQUESTS_DIR/<test file>.jsonl`, one `{test, command, input}` per line, for
// `ordin-service-api-check-requests` to check against the backend's own input rules
// (`make ui-contract-check`). Does nothing when the variable is unset.
import { appendFileSync, mkdirSync } from 'node:fs';
import { basename, join } from 'node:path';
import { afterAll, afterEach, expect } from 'vitest';
import { RECORDED } from './fake-backend';

const dir = process.env.ORDIN_UI_REQUESTS_DIR;
let test = '';

afterEach(() => {
  test = basename(expect.getState().testPath ?? 'unknown');
});

afterAll(() => {
  if (!dir || RECORDED.length === 0) return;
  mkdirSync(dir, { recursive: true });
  const name = test || 'unknown';
  const lines = RECORDED.map((r) => JSON.stringify({ test: name, command: r.command, input: r.input ?? null, ...(r.replied !== undefined ? { replied: r.replied } : {}) })).join('\n');
  appendFileSync(join(dir, `${name}.jsonl`), `${lines}\n`);
  RECORDED.length = 0;
});
