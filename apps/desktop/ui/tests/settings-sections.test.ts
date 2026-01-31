// Settings: the section list is built by SM-UP-1 (Tab order, absent groups) and each section's own words.
import { beforeAll, describe, expect, it } from 'vitest';

import { R1_GROUPS } from './support/fake-backend';
import { visibleSections } from '../src/lib/views/Settings/sections';
import Settings from '../src/lib/views/Settings/Settings.svelte';
import { appWith, html, loadWindowCatalogue } from './support/ssr';
import { hello, LOCAL, ok } from './support/fake-backend';
import { text } from './settings-support';

beforeAll(loadWindowCatalogue);

const ids = (groups: string[], kind: 'local' | 'server' = 'local') => visibleSections({ groups: new Set(groups), kind }).map((s) => s.id);

describe('Settings list (SM-UP-1)', () => {
  it('Happy path — an entry of a group the build lacks is not drawn', async () => {
    const groups = R1_GROUPS.filter((g) => g !== 'mail' && g !== 'telegram');
    const { app } = await appWith((t) => t.on('service.hello', ok(hello(LOCAL, groups))));
    const out = text(html(Settings, { backend: app.local, workspaceId: null, instance: null }, app));
    expect(out).not.toContain('Email');
    expect(out).not.toContain('Telegram');
    expect(out).toContain('Sign-ins');
  });

  it('with groups lacking mail and telegram, the Email and Telegram sections are absent and Tab moves from Slack to Jira', () => {
    const groups = [...R1_GROUPS, 'slack', 'jira'];
    const full = ids([...groups, 'mail', 'telegram']);
    expect(full.indexOf('email')).toBeGreaterThan(-1);
    const present = ids(groups);
    expect(present).not.toContain('email');
    expect(present).not.toContain('telegram');
    expect(present[present.indexOf('slack') + 1]).toBe('jira');
  });

  it('sections are in 11 §4 order, About last after Diagnostics', () => {
    const all = ids([...R1_GROUPS, 'mail', 'telegram', 'slack', 'jira', 'knowledge_agents', 'diagnostics']);
    expect(all.slice(0, 7)).toEqual(['sign-ins', 'agent-repositories', 'email', 'telegram', 'slack', 'jira', 'servers']);
    expect(all.slice(-2)).toEqual(['diagnostics', 'about']);
    expect(all.indexOf('knowledge-agents')).toBeLessThan(all.indexOf('diagnostics'));
    expect(all.indexOf('language')).toBeGreaterThan(all.indexOf('how-ordin-talks'));
    expect(all.indexOf('knowledge-agents')).toBeGreaterThan(all.indexOf('what-ordin-remembers'));
  });

  it('an empty set hides every section', () => {
    expect(ids([])).toEqual([]);
  });

  it('About is drawn for the local backend only', () => {
    expect(ids(R1_GROUPS, 'local')).toContain('about');
    expect(ids(R1_GROUPS, 'server')).not.toContain('about');
  });

  it('a section whose every command is absent leaves the next present section in its place', () => {
    const present = ids(R1_GROUPS.filter((g) => g !== 'agents' && g !== 'catalogue'));
    expect(present).not.toContain('agent-repositories');
    expect(present[1]).toBe('servers');
  });
});

// spec: ordin-desktop/components/ScreensModule#SM-FL-1
describe('Form fields (SM-FL-1)', () => {
  it('Edge case — in every form a label holding a text, email or password field, a select or a text area is its own block with the label above', async () => {
    const { readFileSync } = await import('node:fs');
    const { join } = await import('node:path');
    const css = readFileSync(join(__dirname, '../src/styles/tokens.css'), 'utf8');
    for (const holder of ['input:not([type=\'checkbox\']):not([type=\'radio\']):not([type=\'range\'])', 'select', 'textarea']) {
      expect(css).toContain(`form label:has(> ${holder})`);
    }
    expect(css).toMatch(/form label:has\(> textarea\) \{\s*max-inline-size: 860px;/);
  });
});
