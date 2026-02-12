// SM-AW-6: SharePoint through a folder OneDrive syncs, one test per Gherkin scenario.
import { beforeAll, describe, expect, it } from 'vitest';

import { AddToWorkspaceModel } from '../src/lib/views/add/add-model.svelte';
import AddToWorkspace from '../src/lib/views/add/AddToWorkspace.svelte';
import '../src/lib/views/add/register';
import { err, ok, R1_GROUPS } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const GROUPS = [...R1_GROUPS, 'sharepoint', 'confluence', 'jira'];
const SYNCED = '/Users/asha/Library/CloudStorage/OneDrive-SharedLibraries-Contoso/Finance - Documents';
const render = (state: unknown, model: AddToWorkspaceModel) => text(html(AddToWorkspace, { backend: state, workspaceId: WS, instance: null, model }));
const added = { id: uid('l-od'), kind: 'local_folder', display_name: 'Finance - Documents', state: 'active', workspace_id: WS, added_at: '2026-02-12T09:00:00.000Z', origin_location: SYNCED, place_id: 'p-od' };

describe('SharePoint through a folder OneDrive syncs (SM-AW-6)', () => {
  it('Happy path — the person adds a synced library as a folder on this computer', async () => {
    const { handle, transport, state } = await backendWith({
      groups: GROUPS,
      setup: (tr) => tr.on('app.pick_folder', ok({ folder: SYNCED })).on('workspaces.add_link', ok(added)),
    });
    const model = new AddToWorkspaceModel(handle, WS);
    model.choose('sharepoint');
    expect(render(state, model)).toContain('Use a folder OneDrive syncs');
    model.openOneDrive();
    expect(model.step).toBe('onedrive');
    const out = render(state, model);
    expect(out).toContain('SharePoint through OneDrive');
    expect(out).toContain('Select Sync.');
    expect(out).toContain('Choose the synced folder…');
    await model.chooseFolder();
    expect(transport.sentOf('workspaces.add_link')[0]?.input).toEqual({ workspace_id: WS, location: { kind: 'local_folder', path: SYNCED } });
    expect(model.step).toBe('added');
  });

  it('Edge case — no Microsoft sign-in is possible in this version: the OneDrive step is drawn', async () => {
    const { handle } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.sign_in', err('CRED_PROVIDER_NOT_CONFIGURED', 'Not set up.')) });
    const model = new AddToWorkspaceModel(handle, WS);
    model.choose('sharepoint');
    model.step = 'sign_in';
    await model.signIn('SharePoint');
    expect(model.step).toBe('onedrive');
    expect(model.error).toBeNull();
  });

  it('Edge case — SharePoint search not set up: the OneDrive step, not the Confluence line', async () => {
    const { handle } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('sharepoint.find', err('WS_SOURCE_NOT_CONFIGURED', 'Not set up.')) });
    const model = new AddToWorkspaceModel(handle, WS);
    model.choose('sharepoint');
    await model.search('Finance');
    expect(model.step).toBe('onedrive');
  });

  it('Error path — the organisation blocks ordin\'s app: its sentence, and the OneDrive way below it', async () => {
    const { handle, state } = await backendWith({
      groups: GROUPS,
      setup: (tr) =>
        tr
          .on('identity.sign_in', ok({ kind: 'browser_opened', challenge_id: uid('ms3'), expires_at: 'x' }))
          .on('identity.sign_in_wait', err('CRED_ADMIN_CONSENT_REQUIRED', 'Your organisation has to approve ordin.')),
    });
    const model = new AddToWorkspaceModel(handle, WS);
    model.choose('sharepoint');
    model.step = 'sign_in';
    await model.signIn('SharePoint');
    expect(model.step).toBe('sign_in');
    const out = render(state, model);
    expect(out).toContain('Your organisation has to approve ordin.');
    expect(out.indexOf('Use a folder OneDrive syncs')).toBeGreaterThan(out.indexOf('Your organisation has to approve ordin.'));
  });

  it('Edge case — the person closes the picker: nothing is sent and the step stays drawn', async () => {
    const { handle, transport } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('app.pick_folder', ok({ folder: null })) });
    const model = new AddToWorkspaceModel(handle, WS);
    model.openOneDrive();
    await model.chooseFolder();
    expect(transport.sentOf('workspaces.add_link')).toHaveLength(0);
    expect(model.step).toBe('onedrive');
  });

  it('Confluence keeps its own not-set-up line', async () => {
    const { handle } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('confluence.find', err('WS_SOURCE_NOT_CONFIGURED', 'Not set up.')) });
    const model = new AddToWorkspaceModel(handle, WS);
    model.choose('confluence');
    await model.search('Policy');
    expect(model.step).toBe('not_configured');
  });
});
