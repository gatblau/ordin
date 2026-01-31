// Shared by tests/engineering-*.test.ts and tests/fixed-*.test.ts: a loaded backend over the fake transport.
import { createHandle } from '../src/lib/backend/backend';
import { BackendState } from '../src/lib/store/backend-state.svelte';
import { Diagnostics } from '../src/lib/store/diagnostics.svelte';
import { FakeTransport, hello, LOCAL, ok, R1_GROUPS, SERVER } from './support/fake-backend';

export const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
export const PROJECT = '0192f300-0000-7000-8000-000000000001';

export async function backendWith(
  options: { groups?: string[]; kind?: 'local' | 'server'; setup?: (t: FakeTransport) => void } = {},
) {
  const transport = new FakeTransport();
  const kind = options.kind ?? 'local';
  const id = kind === 'local' ? LOCAL : SERVER;
  transport.on('service.hello', ok(hello(id, options.groups ?? R1_GROUPS)));
  options.setup?.(transport);
  const diagnostics = new Diagnostics();
  const handle = createHandle({ id, kind, transport, diagnostics });
  const state = new BackendState({ handle, diagnostics });
  await state.load();
  transport.sent.length = 0;
  return { transport, handle, state, diagnostics };
}

export const text = (markup: string): string => markup.replace(/<[^>]*>/g, ' ').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ');
export const buttons = (markup: string): string[] => [...markup.matchAll(/<button[^>]*>/g)].map((m) => m[0]);

export const project = (over: Record<string, unknown> = {}) =>
  ({
    check_commands: [],
    current_branch: 'main',
    display_name: 'Billing',
    id: PROJECT,
    is_git_repository: true,
    language_servers_in_use: [],
    opened_at: '2026-01-10T10:00:00.000Z',
    place_id: '0192f200-0000-7000-8000-000000000001',
    state: 'opened',
    ...over,
  }) as never;
