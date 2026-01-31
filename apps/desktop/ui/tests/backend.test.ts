import { describe, expect, it } from 'vitest';

import { Backends, createHandle, newRequestId } from '../src/lib/backend/backend';
import { Diagnostics } from '../src/lib/store/diagnostics.svelte';
import { err, FakeTransport, LOCAL, ok, SERVER } from './support/fake-backend';
import { uid } from './support/ids';

function setup(options: { timeoutMs?: number; onNotInThisVersion?: () => void } = {}) {
  const transport = new FakeTransport();
  const diagnostics = new Diagnostics();
  const handle = createHandle({ id: LOCAL, kind: 'local', transport, diagnostics, ...options });
  return { transport, diagnostics, handle };
}

describe('newRequestId', () => {
  const V7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

  it('is a lowercase hyphenated version-7 UUID, the only form the core accepts', () => {
    for (let i = 0; i < 200; i += 1) expect(newRequestId()).toMatch(V7);
  });

  it('carries the time in its first 48 bits', () => {
    const id = newRequestId(0x0123456789ab);
    expect(id.replace(/-/g, '').slice(0, 12)).toBe('0123456789ab');
  });

  it('is what a handle sends when no generator is given', async () => {
    const { transport, handle } = setup();
    await handle.call('service.hello', {} as never);
    expect(transport.sent[0]?.request_id).toMatch(V7);
  });
});

describe('BackendHandle', () => {
  it('sends the envelope to IPC_NAMES[command] with a new request id each time', async () => {
    const { transport, handle } = setup();
    transport.on('home.needs_you', ok([]));
    await handle.call('home.needs_you', {});
    await handle.call('home.needs_you', {});
    const [a, b] = transport.sent;
    expect(a?.command).toBe('home.needs_you');
    expect(a?.backend).toBe(LOCAL);
    expect(a?.api_version).toEqual({ major: 1, minor: 0 });
    expect(a?.request_id).not.toBe(b?.request_id);
  });

  it('Error path — the bridge cannot be reached: UI_IPC_FAILED is recorded and the outcome is one error', async () => {
    const { transport, diagnostics, handle } = setup();
    transport.bridgeDown = true;
    const out = await handle.call('home.needs_you', {});
    expect(out).toEqual({ kind: 'error', error: { code: 'UI_IPC_FAILED', message: '' } });
    expect(diagnostics.codes()).toEqual(['UI_IPC_FAILED']);
  });

  it('a command with no reply after the timeout is UI_COMMAND_TIMEOUT', async () => {
    const { transport, diagnostics, handle } = setup({ timeoutMs: 20 });
    transport.hang.add('home.needs_you');
    const out = await handle.call('home.needs_you', {});
    expect(out.kind === 'error' && out.error.code).toBe('UI_COMMAND_TIMEOUT');
    expect(diagnostics.codes()).toEqual(['UI_COMMAND_TIMEOUT']);
  });

  it('a conversation command that waits for the model is not timed out', async () => {
    const { transport, handle } = setup({ timeoutMs: 5 });
    transport.on('ordin.send_message', async () => {
      await new Promise((r) => setTimeout(r, 30));
      return ok({ accepted: true, position_in_queue: 0 });
    });
    const out = await handle.call('ordin.send_message', { workspace_id: uid('w'), text: 'hi' });
    expect(out.kind).toBe('ok');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-ER-2
  it.each(['setup.run_guided', 'setup.sign_in_harness', 'identity.sign_in_wait', 'app.pick_folder', 'files.save_export', 'identity.import_client_certificate'] as const)(
    'SM-FS-6 — %s waits for the person and is not timed out',
    async (command) => {
      const { transport, diagnostics, handle } = setup({ timeoutMs: 5 });
      transport.on(command, async () => {
        await new Promise((r) => setTimeout(r, 30));
        return ok({});
      });
      const out = await handle.call(command, {} as never);
      expect(out.kind).toBe('ok');
      expect(diagnostics.codes()).toEqual([]);
    },
  );

  it('a reply of kind error comes back as the core sent it', async () => {
    const { transport, diagnostics, handle } = setup();
    transport.on('approval.get', err('APPR_CARD_ALREADY_DECIDED', "This was already decided on Asha's laptop."));
    const out = await handle.call('approval.get', { approval_id: uid('x') });
    expect(out).toEqual({ kind: 'error', error: { code: 'APPR_CARD_ALREADY_DECIDED', message: "This was already decided on Asha's laptop." } });
    expect(diagnostics.codes()).toEqual([]);
  });

  it('SVC_NOT_IN_THIS_VERSION asks the owner to reload the backend (SM-UP-1)', async () => {
    let reloads = 0;
    const { transport, handle } = setup({ onNotInThisVersion: () => reloads++ });
    transport.on('mail.list_messages', err('SVC_NOT_IN_THIS_VERSION', 'This needs a newer version of ordin.'));
    await handle.call('mail.list_messages', {} as never);
    expect(reloads).toBe(1);
  });
});

describe('Backends', () => {
  it('finds the local handle and the paired servers by id', () => {
    const transport = new FakeTransport();
    const local = createHandle({ id: LOCAL, kind: 'local', transport });
    const server = createHandle({ id: SERVER, kind: 'server', transport });
    const backends = new Backends(local);
    backends.setServers([server]);
    expect(backends.get(LOCAL)).toBe(local);
    expect(backends.get(SERVER)).toBe(server);
    expect(backends.get('nope')).toBeUndefined();
    expect(backends.servers()).toEqual([server]);
    expect(backends.all()).toEqual([local, server]);
  });
});
