// Show the installed files (SM-NS-11).
import { beforeAll, describe, expect, it } from 'vitest';

import { groupByFolder, InstalledFilesController } from '../src/lib/views/parts/installed-files.svelte';
import InstalledFilesView from '../src/lib/views/parts/InstalledFilesView.svelte';
import { err, ok } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const sha = (c: string) => c.repeat(64);
const files = [
  { path: 'rules/a.md', sha256: sha('a'), size: 1200 },
  { path: 'SKILL.md', sha256: sha('b'), size: 300 },
  { path: 'rules/b.md', sha256: sha('c'), size: 50 },
];

describe('Show the installed files (SM-NS-11)', () => {
  it('Happy path — the installed files are read only (SM-NS-11): 3 lines are drawn and none opens anything', async () => {
    const { handle, state, transport } = await backendWith({ setup: (tr) => tr.on('agents.installed_files', ok(files)) });
    const c = new InstalledFilesController(handle, uid('agent-1'));
    await c.load();
    expect(transport.sentOf('agents.installed_files')[0]?.input).toEqual({ agent_instance_id: uid('agent-1') });
    const markup = html(InstalledFilesView, { backend: state, c, agent: 'Clause checker' });
    expect(text(markup)).toContain('The installed files of Clause checker');
    expect(text(markup)).toContain('Read only.');
    expect(markup.match(/<li/g)).toHaveLength(3);
    expect(markup).not.toMatch(/<button/);
    expect(text(markup)).toContain('aaaaaaaaaaaa');
    expect(text(markup)).not.toContain('a'.repeat(13));
  });

  it('groups by the path up to its last slash, in the reply\'s order', () => {
    expect(groupByFolder(files).map((g) => [g.folder, g.files.map((f) => f.path)])).toEqual([
      ['rules', ['rules/a.md', 'rules/b.md']],
      ['', ['SKILL.md']],
    ]);
  });

  it('Error path — INST_NOT_INSTALLED draws its sentence, with no list', async () => {
    const { handle, state } = await backendWith({ setup: (tr) => tr.on('agents.installed_files', err('INST_NOT_INSTALLED', 'That agent is no longer installed.')) });
    const c = new InstalledFilesController(handle, uid('agent-1'));
    await c.load();
    const markup = html(InstalledFilesView, { backend: state, c, agent: 'Clause checker' });
    expect(text(markup)).toContain('That agent is no longer installed.');
    expect(markup).not.toMatch(/<li/);
  });
});
