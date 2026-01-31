// @vitest-environment happy-dom
// Settings › Agents and models (ScreensModule.md SM-AGM-1, amended January 2026; user decision; Approved
// by the user January 2026): what runs the person's agents, the model for each level and the effort for
// each kind of work, read with app.agent_settings and changed with app.change_agent_setting.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { stripIsolates } from '../src/lib/i18n/fluent';
import AgentsSettings from '../src/lib/views/Settings/AgentsSettings.svelte';
import { AgentsSettingsModel, isModelName, shownHarnesses } from '../src/lib/views/Settings/agents-settings-model.svelte';
import { visibleSections } from '../src/lib/views/Settings/sections';
import type { AgentSettingsView } from '../src/lib/service-api/types.gen';
import { backendWith } from './fixed-support';
import { AGENT_SETTINGS_DEFAULT, err, ok, R1_GROUPS, settle } from './support/fake-backend';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const DEFAULT = AGENT_SETTINGS_DEFAULT as AgentSettingsView;
const clone = (): AgentSettingsView => JSON.parse(JSON.stringify(DEFAULT)) as AgentSettingsView;

let drawn: { target: HTMLElement; part: ReturnType<typeof mount> } | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn.part);
  drawn?.target.remove();
  drawn = null;
  document.body.innerHTML = '';
});

async function draw(view: AgentSettingsView = DEFAULT, change?: (input: unknown) => ReturnType<typeof ok>) {
  const b = await backendWith({
    setup: (tr) => {
      tr.on('app.agent_settings', ok(view));
      tr.on('app.change_agent_setting', change ?? (() => ok(view)));
    },
  });
  const target = document.createElement('div');
  document.body.appendChild(target);
  const part = mount(AgentsSettings, { target, props: { backend: b.state } });
  drawn = { target, part };
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
  return { target, transport: b.transport };
}

async function after(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}

const words = (el: Element) => stripIsolates(el.textContent ?? '').replace(/\s+/g, ' ');

describe('Settings › Agents and models — the section', () => {
  it('is listed after Using your plan for the local backend that answers both commands', () => {
    const ids = (groups: string[], kind: 'local' | 'server' = 'local') => visibleSections({ groups: new Set(groups), kind }).map((s) => s.id);
    const local = ids(R1_GROUPS);
    expect(local.indexOf('agents-and-models')).toBe(local.indexOf('using-your-plan') + 1);
    expect(ids(R1_GROUPS, 'server')).not.toContain('agents-and-models');
  });

  it('draws the three groups as cards, with plain words and no mention of IT', async () => {
    const { target } = await draw();
    const groups = [...target.querySelectorAll('.group h4')].map(words);
    expect(groups).toEqual(['What runs your agents', 'Models for Claude Code', 'Effort for Claude Code']);
    const all = words(target);
    expect(all).toContain('Same as my organisation (Claude Code)');
    expect(all).toContain('Your agents run with Claude Code now.');
    expect(all, all).not.toMatch(/\bIT\b|harness|adapter/);
  });

  it('shows only the tools that can run, and marks one that needs setting up', async () => {
    const view = clone();
    view.harnesses[0]!.readiness = 'needs_set_up';
    const { target } = await draw(view);
    const options = [...target.querySelectorAll('select[data-control="harness"] option')].map(words);
    expect(options).toEqual(['Same as my organisation (Claude Code)', 'Claude Code — Needs setting up']);
    expect(shownHarnesses(DEFAULT).map((h) => h.harness)).toEqual(['claude_code']);
    // Copilot is drawn once it is found on this computer.
    const found = clone();
    found.harnesses[1]!.readiness = 'needs_set_up';
    expect(shownHarnesses(found).map((h) => h.harness)).toEqual(['claude_code', 'copilot']);
  });

  it('one row per level, with the default as the placeholder', async () => {
    const { target } = await draw();
    const inputs = [...target.querySelectorAll<HTMLInputElement>('[data-harness="claude_code"] input')];
    expect(inputs.map((i) => i.dataset['level'])).toEqual(['smaller', 'standard', 'most_capable']);
    expect(inputs.map((i) => i.placeholder)).toEqual(['haiku', 'sonnet', 'opus']);
    expect(inputs.every((i) => i.value === '')).toBe(true);
    expect(target.querySelector('[data-action="use-default"]')).toBeNull();
    expect(words(target.querySelector('[data-harness="claude_code"]')!)).toContain('Quick, everyday work');
  });

  it('one effort control per kind of work, each starting on Let Claude Code decide', async () => {
    const { target } = await draw();
    const selects = [...target.querySelectorAll<HTMLSelectElement>('[data-effort="claude_code"] select')];
    expect(selects.map((s) => s.dataset['kind'])).toEqual(['talking', 'coordinating', 'routine', 'documents', 'code']);
    expect(selects.every((s) => s.value === '')).toBe(true);
    expect([...selects[0]!.options].map(words)).toEqual(['Let Claude Code decide', 'Low', 'Medium', 'High', 'Very high', 'Highest']);
    const hint = [...target.querySelectorAll('.hint')].map(words).find((h) => h.startsWith('More effort'));
    expect(hint).toContain('your thinking choice in Using your plan applies when it is set');
  });

  it('a tool with no effort control says so in one line', async () => {
    const view = clone();
    view.harnesses[1]!.readiness = 'ready';
    const { target } = await draw(view);
    const copilot = target.querySelector('[data-effort="copilot"]')!;
    expect(copilot.querySelector('select')).toBeNull();
    expect(words(copilot.querySelector('[data-line="no-effort"]')!)).toBe("Copilot doesn't offer an effort setting, so it decides for itself.");
    // Its model rows have no default: the tool chooses.
    const placeholder = target.querySelector<HTMLInputElement>('[data-harness="copilot"] input')!.placeholder;
    expect(stripIsolates(placeholder)).toBe('Chosen by Copilot');
  });
});

describe('Settings › Agents and models — changes', () => {
  it('a typed model is sent, and Use the default clears it', async () => {
    const chosen = clone();
    chosen.harnesses[0]!.models[1]!.chosen = 'claude-sonnet-4.5';
    const { target, transport } = await draw(DEFAULT, () => ok(chosen));
    const standard = target.querySelector<HTMLInputElement>('[data-harness="claude_code"] input[data-level="standard"]')!;
    standard.value = 'claude-sonnet-4.5';
    standard.dispatchEvent(new Event('change', { bubbles: true }));
    await after();
    expect(transport.sentOf('app.change_agent_setting')[0]?.input).toEqual({ change: 'model', harness: 'claude_code', level: 'standard', model: 'claude-sonnet-4.5' });
    const reset = target.querySelector<HTMLButtonElement>('[data-action="use-default"]')!;
    expect(reset.getAttribute('aria-label')).toBe('Use the default');
    reset.click();
    await after();
    expect(transport.sentOf('app.change_agent_setting')[1]?.input).toEqual({ change: 'model', harness: 'claude_code', level: 'standard', model: null });
  });

  it('a model name the backend would refuse is held back with a plain line', async () => {
    const { target, transport } = await draw();
    const smaller = target.querySelector<HTMLInputElement>('input[data-level="smaller"]')!;
    smaller.value = 'two words';
    smaller.dispatchEvent(new Event('change', { bubbles: true }));
    await after();
    expect(transport.sentOf('app.change_agent_setting')).toEqual([]);
    expect(smaller.getAttribute('aria-invalid')).toBe('true');
    expect(words(target)).toContain('Use letters, numbers, dots, dashes and underscores only');
    expect(isModelName('claude-opus-4.5')).toBe(true);
    expect(isModelName('a'.repeat(101))).toBe(false);
  });

  it('an effort and the tool to use are sent as chosen', async () => {
    const { target, transport } = await draw();
    const code = target.querySelector<HTMLSelectElement>('select[data-kind="code"]')!;
    code.value = 'high';
    code.dispatchEvent(new Event('change', { bubbles: true }));
    await after();
    const harness = target.querySelector<HTMLSelectElement>('select[data-control="harness"]')!;
    harness.value = 'claude_code';
    harness.dispatchEvent(new Event('change', { bubbles: true }));
    await after();
    harness.value = '';
    harness.dispatchEvent(new Event('change', { bubbles: true }));
    await after();
    expect(transport.sentOf('app.change_agent_setting').map((e) => e.input)).toEqual([
      { change: 'effort', harness: 'claude_code', kind: 'code', effort: 'high' },
      { change: 'harness', harness: 'claude_code' },
      { change: 'harness', harness: null },
    ]);
  });

  it('a failed save offers Try again and sends the same change again', async () => {
    const b = await backendWith({
      setup: (tr) => {
        tr.on('app.agent_settings', ok(DEFAULT));
        tr.on('app.change_agent_setting', err('DSK_PREFERENCES_WRITE_FAILED', 'not saved'));
      },
    });
    const model = new AgentsSettingsModel(b.handle);
    await model.load();
    expect(await model.setEffort('claude_code', 'talking', 'low')).toBe(false);
    expect(model.error?.action).toBe('try_again');
    b.transport.on('app.change_agent_setting', ok(DEFAULT));
    model.retry();
    await settle();
    expect(b.transport.sentOf('app.change_agent_setting').map((e) => e.input)).toEqual([
      { change: 'effort', harness: 'claude_code', kind: 'talking', effort: 'low' },
      { change: 'effort', harness: 'claude_code', kind: 'talking', effort: 'low' },
    ]);
    expect(model.error).toBeNull();
  });
});
