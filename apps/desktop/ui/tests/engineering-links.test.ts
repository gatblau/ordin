import { describe, expect, test } from 'vitest';
import editor from '../src/lib/views/engineering/CodeEditor.svelte?raw';
import terminal from '../src/lib/views/engineering/Terminal.svelte?raw';

describe('links in untrusted text are never opened', () => {
  test('the code editor turns link detection off', () => {
    expect(editor).toMatch(/links:\s*false/);
  });
  test('the terminal refuses to activate a link in its output', () => {
    expect(terminal).toMatch(/linkHandler:\s*\{\s*activate:\s*\(\)\s*=>\s*\{\}\s*\}/);
  });
});
