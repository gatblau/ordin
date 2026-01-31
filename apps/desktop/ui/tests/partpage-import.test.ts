// The import screen (SM-NS-10).
import { beforeAll, describe, expect, it } from 'vitest';

import { draftScreenKey, ImportController } from '../src/lib/views/parts/import.svelte';
import ImportView from '../src/lib/views/parts/ImportView.svelte';
import { OFFERED_PART_KINDS } from '../src/lib/views/parts/part-kinds';
import { err, ok } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const repo = (id: string, name: string, canPublish: boolean) => ({ agent_count: 1, repository: { id, display_name: name, person_can_publish: canPublish }, set_by_company: false });
const setup = (tr: import('./support/fake-backend').FakeTransport) =>
  tr
    .on('catalogue.list_repositories', ok([repo(uid('r1'), 'Company agents', true), repo(uid('r2'), 'Read only', false)]))
    .on('app.pick_folder', ok({ folder: '/Users/ana/work/clause-checker' }))
    .on('authoring.import', ok({ id: 'd1', kind: 'agent' }));

describe('Import screen (SM-NS-10)', () => {
  it('Happy path — importing an agent from a folder (SM-NS-10): authoring.import is sent without as_part and the agent draft opens', async () => {
    const { handle, state, transport } = await backendWith({ groups: ['service', 'authoring', 'catalogue', 'app'], setup });
    const c = new ImportController(handle, handle, WS);
    await c.load();
    expect(c.choices.map((r) => r.repository.id)).toEqual([uid('r1')]);
    c.repositoryId = uid('r1');
    expect(c.ready).toBe(false);
    await c.chooseFolder();
    expect(c.folderName).toBe('clause-checker');
    const ref = await c.run();
    expect(transport.sentOf('authoring.import')[0]?.input).toEqual({ agent_repository_id: uid('r1'), source: '/Users/ana/work/clause-checker', workspace_id: WS });
    expect(ref && draftScreenKey(ref)).toBe('agent-draft:d1');
    const body = text(html(ImportView, { backend: state, c }));
    expect(body).toContain('Import an agent or a part');
    expect(body).toContain('What are you bringing in?');
    expect(body).toContain('Company agents');
    expect(body).not.toContain('Read only</option>');
    expect(body).toContain('clause-checker');
  });

  it('one part is sent as as_part with its kind and opens the part draft; "not offered" kinds are not choosable', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => setup(tr).on('authoring.import', ok({ id: 'p1', kind: 'part' })) });
    const c = new ImportController(handle, handle, WS);
    await c.load();
    c.what = 'part';
    c.repositoryId = uid('r1');
    await c.chooseFolder();
    expect(c.ready).toBe(false);
    c.partKind = 'Skill';
    const ref = await c.run();
    expect(transport.sentOf('authoring.import')[0]?.input).toMatchObject({ as_part: 'Skill' });
    expect(ref && draftScreenKey(ref)).toBe('part-draft:p1');
    expect(OFFERED_PART_KINDS).not.toContain('AgentIdentity');
    expect(OFFERED_PART_KINDS).toContain('Rule');
  });

  it('Error path — a refused import draws the owner\'s sentence and keeps the choices', async () => {
    const { handle, state } = await backendWith({ setup: (tr) => setup(tr).on('authoring.import', err('AUTH_IMPORT_UNREADABLE', 'ordin could not read that folder.')) });
    const c = new ImportController(handle, handle, WS);
    await c.load();
    c.repositoryId = uid('r1');
    await c.chooseFolder();
    expect(await c.run()).toBeNull();
    expect(c.folder).toBe('/Users/ana/work/clause-checker');
    expect(c.repositoryId).toBe(uid('r1'));
    expect(text(html(ImportView, { backend: state, c }))).toContain('ordin could not read that folder.');
  });

  it('a cancelled folder picker leaves the folder as it was', async () => {
    const { handle } = await backendWith({ setup: (tr) => setup(tr).on('app.pick_folder', ok({ folder: null })) });
    const c = new ImportController(handle, handle, WS);
    await c.chooseFolder();
    expect(c.folder).toBeNull();
  });
});
