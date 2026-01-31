// What [agent] is made of (SM-NS-11): the installed files as a tree, and a reader for one.
import { beforeAll, describe, expect, it } from 'vitest';

import { buildTree, folderPaths, InstalledFilesController, isMarkdown } from '../src/lib/views/parts/installed-files.svelte';
import InstalledFilesView from '../src/lib/views/parts/InstalledFilesView.svelte';
import { err, ok } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const sha = (c: string) => c.repeat(64);
const files = [
  { path: 'ordin.lock', sha256: sha('d'), size: 80 },
  { path: 'rules/b.md', sha256: sha('a'), size: 1200 },
  { path: 'SKILL.md', sha256: sha('b'), size: 300 },
  { path: 'rules/a.md', sha256: sha('c'), size: 50 },
  { path: 'skills/write/notes/one.txt', sha256: sha('e'), size: 9 },
];

const shape = (nodes: ReturnType<typeof buildTree>): unknown =>
  nodes.map((n) => (n.kind === 'folder' ? { [n.name]: shape(n.children) } : n.name));

describe('What [agent] is made of (SM-NS-11)', () => {
  it('builds folders first, then files, each by name, nesting every folder of a path', () => {
    expect(shape(buildTree(files))).toEqual([
      { rules: ['a.md', 'b.md'] },
      { skills: [{ write: [{ notes: ['one.txt'] }] }] },
      'ordin.lock',
      'SKILL.md',
    ]);
  });

  it('keeps each file\'s whole path and entry, and skips empty path segments', () => {
    const tree = buildTree([{ path: '/a//b.md', sha256: sha('a'), size: 1 }, { path: '', sha256: sha('b'), size: 1 }]);
    expect(tree).toHaveLength(1);
    const a = tree[0];
    if (a?.kind !== 'folder') throw new Error('expected the folder a');
    expect(a.path).toBe('a');
    const b = a.children[0];
    expect(b?.kind === 'file' && b.path).toBe('/a//b.md');
    expect(b?.kind === 'file' && b.entry.size).toBe(1);
  });

  it('lists every folder path, nested ones included', () => {
    expect(folderPaths(buildTree(files))).toEqual(['rules', 'skills', 'skills/write', 'skills/write/notes']);
    expect(buildTree([])).toEqual([]);
  });

  it('draws Markdown files formatted and other files as plain text', () => {
    expect(isMarkdown('skills/a.md')).toBe(true);
    expect(isMarkdown('README.MARKDOWN')).toBe(true);
    expect(isMarkdown('ordin.lock')).toBe(false);
    expect(isMarkdown('rules/policy.yaml')).toBe(false);
  });

  it('Happy path — the tree is drawn and nothing is read until a file is chosen', async () => {
    const { handle, state, transport } = await backendWith({ setup: (tr) => tr.on('agents.installed_files', ok(files)) });
    const c = new InstalledFilesController(handle, uid('agent-1'));
    await c.load();
    expect(transport.sentOf('agents.installed_files')[0]?.input).toEqual({ agent_instance_id: uid('agent-1') });
    const markup = html(InstalledFilesView, { backend: state, c, agent: 'Clause checker' });
    expect(text(markup)).toContain('What Clause checker is made of');
    expect(text(markup)).toContain('Read only.');
    expect(text(markup)).toContain('SKILL.md');
    expect(text(markup)).toContain('one.txt');
    expect(text(markup)).toContain('Choose a file to read it.');
    expect(markup).toContain('role="tree"');
    expect(transport.sentOf('agents.installed_file')).toHaveLength(0);
  });

  it('a chosen Markdown file is drawn formatted, with its size and checksum under Details', async () => {
    const { handle, state, transport } = await backendWith({
      setup: (tr) => {
        tr.on('agents.installed_files', ok(files));
        tr.on('agents.installed_file', ok({ path: 'rules/a.md', text: '# Be kind\n\nAlways **kind**.', cut: false }));
      },
    });
    const c = new InstalledFilesController(handle, uid('agent-1'));
    await c.load();
    await c.open('rules/a.md');
    expect(transport.sentOf('agents.installed_file')[0]?.input).toEqual({ agent_instance_id: uid('agent-1'), path: 'rules/a.md' });
    const markup = html(InstalledFilesView, { backend: state, c, agent: 'Clause checker' });
    expect(markup).toContain('<strong>kind</strong>');
    expect(markup).not.toContain('**kind**');
    expect(text(markup)).toContain('cccccccccccc');
    expect(text(markup)).not.toContain('c'.repeat(13));
  });

  it('a chosen file that is not Markdown is drawn as it is, in monospace', async () => {
    const { handle, state } = await backendWith({
      setup: (tr) => {
        tr.on('agents.installed_files', ok(files));
        tr.on('agents.installed_file', ok({ path: 'ordin.lock', text: 'version: **1**', cut: true }));
      },
    });
    const c = new InstalledFilesController(handle, uid('agent-1'));
    await c.load();
    await c.open('ordin.lock');
    const markup = html(InstalledFilesView, { backend: state, c, agent: 'Clause checker' });
    expect(markup).toMatch(/<pre class="mono[^"]*">version: \*\*1\*\*<\/pre>/);
    expect(text(markup)).toContain('Only the start of this file is shown.');
  });

  it('Error path — a file that cannot be read draws its sentence in the reader', async () => {
    const { handle, state } = await backendWith({
      setup: (tr) => {
        tr.on('agents.installed_files', ok(files));
        tr.on('agents.installed_file', err('PKI_PART_NOT_FOUND', 'That part is not in this agent.'));
      },
    });
    const c = new InstalledFilesController(handle, uid('agent-1'));
    await c.load();
    await c.open('SKILL.md');
    expect(c.file).toBeNull();
    const markup = html(InstalledFilesView, { backend: state, c, agent: 'Clause checker' });
    expect(text(markup)).toContain('That part is not in this agent.');
  });

  it('Error path — INST_NOT_INSTALLED draws its sentence, with no tree', async () => {
    const { handle, state } = await backendWith({ setup: (tr) => tr.on('agents.installed_files', err('INST_NOT_INSTALLED', 'That agent is no longer installed.')) });
    const c = new InstalledFilesController(handle, uid('agent-1'));
    await c.load();
    const markup = html(InstalledFilesView, { backend: state, c, agent: 'Clause checker' });
    expect(text(markup)).toContain('That agent is no longer installed.');
    expect(markup).not.toContain('role="tree"');
  });
});
