// @vitest-environment happy-dom
// Ordin's folder question (SM-FA-4, OrdinHost.md OH-ACC-2): a waiting `question` block with Allow and
// Not now, mounted so its answer is sent.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import Question from '../src/lib/blocks/kinds/Question.svelte';
import { backendWith, WS } from './fixed-support';
import { ok, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const BLOCK = uid('block-folder');
const question = {
  kind: 'question',
  question: 'Document agent needs to read CVs to answer.',
  choices: ['Allow', 'Not now'],
  allow_other: false,
};

async function draw(
  block: typeof question = question,
): Promise<{ target: HTMLElement; transport: Awaited<ReturnType<typeof backendWith>>['transport'] }> {
  const { state, transport } = await backendWith({ setup: (t) => t.on('ordin.answer_block', ok({ accepted: true })) });
  state.conversation.set(WS, [{ block_id: BLOCK, waiting: true, block } as never]);
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(Question as never, { target, props: { backend: state, workspaceId: WS, blockId: BLOCK, block } as never });
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
  return { target, transport };
}

describe("Ordin's folder question (SM-FA-4)", () => {
  // spec: ordin-desktop/components/ScreensModule#SM-FA-4
  it('Happy path — the question is drawn with Allow and Not now, and Allow is sent', async () => {
    const { target, transport } = await draw();
    expect(target.textContent).toContain('Document agent needs to read CVs to answer.');
    const buttons = [...target.querySelectorAll('button')] as HTMLButtonElement[];
    // SM-DIS-1: a question with its own Not now carries no Skip beside it.
    expect(buttons.map((b) => b.textContent?.trim())).toEqual(['Allow', 'Not now']);
    expect(target.querySelector('input[type="checkbox"]')).toBeNull();
    expect(buttons.every((b) => !b.disabled)).toBe(true);
    buttons[0]?.click();
    await settle(10);
    flushSync();
    expect(transport.sentOf('ordin.answer_block').map((e) => e.input)).toEqual([
      { block_id: BLOCK, workspace_id: WS, answer: { kind: 'choice', choice: 'Allow' } },
    ]);
    expect(([...target.querySelectorAll('button')] as HTMLButtonElement[]).every((b) => b.disabled)).toBe(true);
  });

  it('Edge case — Not now is sent the same way', async () => {
    const { target, transport } = await draw();
    ([...target.querySelectorAll('button')] as HTMLButtonElement[])[1]?.click();
    await settle(10);
    expect(transport.sentOf('ordin.answer_block').map((e) => (e.input as { answer: unknown }).answer)).toEqual([{ kind: 'choice', choice: 'Not now' }]);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-DIS-1
  it('Edge case — the folder card has no Skip: its own Not now sets it aside (SM-DIS-1)', async () => {
    const { target } = await draw();
    const names = ([...target.querySelectorAll('button')] as HTMLButtonElement[]).map((b) => b.textContent?.trim());
    expect(names).not.toContain('Skip');
  });
});

describe('Choice layout and colour (SM-QB-1)', () => {
  const classes = (target: HTMLElement) =>
    ([...target.querySelectorAll('.choices button')] as HTMLButtonElement[]).map((b) => b.className.replace(/\s*svelte-\S+/g, '').trim());

  // spec: ordin-desktop/components/ScreensModule#SM-QB-1
  it('Happy path — Allow and Not now sit on one row, Allow the primary button', async () => {
    const { target } = await draw();
    expect(target.querySelector('.choices')?.classList.contains('row')).toBe(true);
    expect(classes(target)).toEqual(['primary', '']);
  });

  it('Edge case — options of equal weight have no primary button', async () => {
    const { target } = await draw({ ...question, choices: ['Monthly', 'Weekly'] });
    expect(classes(target)).toEqual(['', '']);
  });

  it('Edge case — a long choice stacks the choices; a reject is a danger button', async () => {
    const { target } = await draw({ ...question, choices: ['Keep the draft as it is for now', 'Reject it', 'Not now'] });
    expect(target.querySelector('.choices')?.classList.contains('row')).toBe(false);
    expect(classes(target)).toEqual(['primary', 'danger', '']);
  });
});
