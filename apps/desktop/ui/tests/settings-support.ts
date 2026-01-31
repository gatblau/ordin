// Shared by tests/settings-*.test.ts and tests/home-*.test.ts: a loaded backend state over a fake transport.
import { createHandle } from '../src/lib/backend/backend';
import { BackendState } from '../src/lib/store/backend-state.svelte';
import { Diagnostics } from '../src/lib/store/diagnostics.svelte';
import { FakeTransport, hello, LOCAL, ok, R1_GROUPS } from './support/fake-backend';

export const text = (markup: string): string => markup.replace(/<[^>]*>/g, ' ').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ');

/** A loaded local backend whose `service.hello` answers `groups`. */
export async function loaded(groups: string[] = R1_GROUPS, setup: (t: FakeTransport) => void = () => undefined) {
  const transport = new FakeTransport();
  transport.on('service.hello', ok(hello(LOCAL, groups)));
  setup(transport);
  const diagnostics = new Diagnostics();
  const handle = createHandle({ id: LOCAL, kind: 'local', transport, diagnostics });
  const state = new BackendState({ handle, diagnostics });
  await state.load();
  transport.sent.length = 0;
  return { transport, handle, state, diagnostics };
}
