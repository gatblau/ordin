// @vitest-environment happy-dom
// Setting aside what waits for the person (ScreensModule.md SM-DIS-1, OrdinHost.md OH-DIS-1): **Skip** on a
// question, **Dismiss** on a form and **Not now** on a crew plan send `ordin.answer_block` with
// `{ kind: 'dismissed' }`, and the item stops waiting. A refusal is the error line and the item still waits.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { CrewPlanController } from '../src/lib/blocks/crew-plan.svelte';
import Form from '../src/lib/blocks/kinds/Form.svelte';
import Question from '../src/lib/blocks/kinds/Question.svelte';
import { backendWith, WS } from './fixed-support';
import { err, ok, R1_GROUPS, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const BLOCK = uid('block-waiting');
const DISMISSED = { block_id: BLOCK, workspace_id: WS, answer: { kind: 'dismissed' } };
const question = { kind: 'question', question: 'Which supplier?', choices: ['Kestrel', 'Acme'], allow_other: false };
const form = {
  kind: 'form',
  title: 'Q3 rates',
  verb: 'Compare',
  target: { kind: 'agent_instance', agent_instance_id: uid('agent') },
  fields: [{ name: 'quarter', label: 'Quarter', required: true, input: 'text', value: null, placeholder: null, filled_by_ordin: false }],
};

async function draw(component: unknown, block: unknown, reply = ok({ accepted: true }) as never) {
  const made = await backendWith({ setup: (t) => t.on('ordin.answer_block', reply) });
  made.state.conversation.set(WS, [{ block_id: BLOCK, waiting: true, block } as never]);
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(component as never, { target, props: { backend: made.state, workspaceId: WS, blockId: BLOCK, block } as never });
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
  return { ...made, target };
}

const named = (target: HTMLElement, label: string): HTMLButtonElement | undefined =>
  ([...target.querySelectorAll('button')] as HTMLButtonElement[]).find((b) => b.textContent?.trim() === label);

async function press(button: HTMLButtonElement | undefined): Promise<void> {
  expect(button).toBeDefined();
  button?.click();
  await settle(10);
  flushSync();
}

describe('SM-DIS-1 — setting aside what waits', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-DIS-1
  it('Happy path — Skip on a question sends a dismissal and the question stops waiting', async () => {
    const { target, transport, state } = await draw(Question, question);
    const labels = ([...target.querySelectorAll('button')] as HTMLButtonElement[]).map((b) => b.textContent?.trim());
    expect(labels).toEqual(['Kestrel', 'Acme', 'Skip']);
    await press(named(target, 'Skip'));
    expect(transport.sentOf('ordin.answer_block').map((e) => e.input)).toEqual([DISMISSED]);
    expect(state.conversation.get(WS)?.[0]?.waiting).toBe(false);
    expect(named(target, 'Skip')).toBeUndefined();
    expect(named(target, 'Kestrel')?.disabled).toBe(true);
  });

  it('Happy path — Dismiss on a form sends a dismissal, not the form, and the form stops waiting', async () => {
    const { target, transport, state } = await draw(Form, form);
    await press(named(target, 'Dismiss'));
    expect(transport.sentOf('ordin.answer_block').map((e) => e.input)).toEqual([DISMISSED]);
    expect(state.conversation.get(WS)?.[0]?.waiting).toBe(false);
    expect(named(target, 'Dismiss')).toBeUndefined();
    expect(named(target, 'Compare')?.disabled).toBe(true);
  });

  it('Error — a refused dismissal is the error line and the item still waits', async () => {
    const { target, state } = await draw(Question, question, err('OH_BLOCK_ANSWERED', 'This was already answered.') as never);
    await press(named(target, 'Skip'));
    expect(target.textContent).toContain('This was already answered.');
    expect(state.conversation.get(WS)?.[0]?.waiting).toBe(true);
  });

  it('Edge — an item that no longer waits has no Skip or Dismiss', async () => {
    const made = await backendWith();
    made.state.conversation.set(WS, [{ block_id: BLOCK, waiting: false, block: question } as never]);
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(Question as never, { target, props: { backend: made.state, workspaceId: WS, blockId: BLOCK, block: question } as never });
    await settle();
    flushSync();
    expect(named(target, 'Skip')).toBeUndefined();
  });

  it('Happy path — Not now on a crew plan sends a dismissal, starts no crew and the plan stops waiting', async () => {
    const charter = '0192a1c5-1111-7222-8333-444455556666';
    const made = await backendWith({
      groups: [...R1_GROUPS, 'crews'],
      setup: (t) => {
        t.on('crews.charter', ok({ charter: { id: charter, state: 'valid', members: [], guardrails: { approval_points: [] } } }));
        t.on('agents.list', ok([]));
        t.on('ordin.answer_block', ok({ accepted: true }));
      },
    });
    made.state.conversation.set(WS, [{ block_id: BLOCK, workspace_id: WS, block: { kind: 'crew_plan' } as never, seq: 1, at: '', waiting: true }]);
    const plan = new CrewPlanController(made.state, WS, BLOCK, charter);
    await plan.load();
    expect(plan.offersStart).toBe(true);
    await plan.notNow();
    expect(made.transport.sentOf('ordin.answer_block').map((e) => e.input)).toEqual([DISMISSED]);
    expect(made.transport.sentOf('crews.start')).toHaveLength(0);
    expect(plan.waiting).toBe(false);
    expect(plan.offersStart).toBe(false);
  });
});
