// The source scan: what the module must never contain, and the one file allowed to reach the Bridge.
// It reads every file under src/ at test time, so another worker's files are covered with no change here.
import { describe, expect, it } from 'vitest';

const files = import.meta.glob(['../src/**/*.ts', '../src/**/*.svelte'], { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const sources = Object.entries(files)
  .map(([path, content]) => ({ path: path.replace('../src/', 'src/'), content }))
  .filter((f) => !f.path.endsWith('.gen.ts'));

const offenders = (pattern: RegExp, except: (path: string) => boolean = () => false): string[] =>
  sources.filter((f) => !except(f.path) && pattern.test(f.content)).map((f) => f.path);

describe('source scan', () => {
  it('finds the module sources', () => {
    expect(sources.length).toBeGreaterThan(20);
    expect(sources.some((f) => f.path === 'src/lib/backend/tauri.ts')).toBe(true);
  });

  it('no markup from data: {@html, innerHTML and the other ways to build elements from text', () => {
    for (const pattern of [/\{@html/, /\binnerHTML\b/, /\bouterHTML\b/, /\binsertAdjacentHTML\b/, /document\.write\b/, /\bcreateContextualFragment\b/]) {
      expect(offenders(pattern), String(pattern)).toEqual([]);
    }
    // The one parser: a brand pack's icon set, read into a detached document and copied element by element
    // through an allowlist with createElementNS (cross-cutting/VisualDesign.md, Internal Logic step 8).
    expect(offenders(/\bDOMParser\b/, (p) => p === 'src/lib/icons/sprite.ts')).toEqual([]);
  });

  it('no network call of its own: fetch, XMLHttpRequest, WebSocket, EventSource', () => {
    for (const pattern of [/\bfetch\s*\(/, /\bXMLHttpRequest\b/, /\bWebSocket\b/, /\bEventSource\b/, /\bsendBeacon\b/]) {
      expect(offenders(pattern), String(pattern)).toEqual([]);
    }
  });

  it('SA-UI-01 — only lib/backend/tauri.ts imports @tauri-apps/api', () => {
    // An import, a re-export or a dynamic import of the package; a comment naming it is not one.
    const imports = /(?:\bfrom\s*|\bimport\s*\(?\s*|\brequire\s*\(\s*)['"]@tauri-apps\//;
    expect(offenders(imports, (p) => p === 'src/lib/backend/tauri.ts')).toEqual([]);
    expect(offenders(imports)).toEqual(['src/lib/backend/tauri.ts']);
  });

  // Plan 04: Svelte's getContext works only while a component is set up. Called from a click handler, an
  // effect or a helper it throws, appOrNull() turns that into null, and the click silently does nothing
  // (a PDF row in Documents, test.1). The app context is read once, at the top level of a component's
  // script (two-space indent), and passed on from there.
  it('the app context is read only while a component is set up, never in a handler or a helper', () => {
    const lookup = /\b(?:appOrNull|getApp|getContext)\(/;
    const found: string[] = [];
    for (const { path, content } of sources) {
      if (path === 'src/lib/views/fixed/util.ts' || path === 'src/lib/shell/context.ts') continue;
      if (!path.endsWith('.svelte')) {
        content.split('\n').forEach((line, i) => {
          if (lookup.test(line) && !/^\s*(\/\/|\*)/.test(line)) found.push(`${path}:${i + 1}`);
        });
        continue;
      }
      let inScript = false;
      content.split('\n').forEach((line, i) => {
        if (/<script\b/.test(line)) inScript = true;
        else if (/<\/script>/.test(line)) inScript = false;
        else if (lookup.test(line) && !/^\s*\/\//.test(line) && !(inScript && /^ {2}\S/.test(line))) found.push(`${path}:${i + 1}`);
      });
    }
    expect(found).toEqual([]);
  });

  it('no eval or dynamic function construction', () => {
    expect(offenders(/\beval\s*\(|new Function\s*\(/)).toEqual([]);
  });

  it('SA-I18N-01 — no literal user-facing words in templates: the words come from the catalogue', () => {
    const found: string[] = [];
    for (const { path, content } of sources.filter((f) => f.path.endsWith('.svelte'))) {
      let template = content
        .replace(/<script[\s\S]*?<\/script>/g, '')
        .replace(/<style[\s\S]*?<\/style>/g, '')
        .replace(/<!--[\s\S]*?-->/g, '');
      // Remove expressions, innermost braces first, so `=>` and `{ ... }` bodies do not look like text.
      for (let i = 0; i < 12 && /\{[^{}]*\}/.test(template); i++) template = template.replace(/\{[^{}]*\}/g, '');
      for (const attr of template.matchAll(/\b(aria-label|aria-description|title|placeholder|alt)="([^"]*[A-Za-z]{2,}[^"]*)"/g)) found.push(`${path}: ${attr[1]}="${attr[2]}"`);
      const text = template.replace(/<[^>]*>/g, '\n');
      for (const line of text.split('\n')) if (/[A-Za-z]{2,}/.test(line)) found.push(`${path}: ${line.trim()}`);
    }
    expect(found).toEqual([]);
  });
});

// Icons (ScreensModule.md amendment *Icons*, SM-IC-2, SM-IC-3; cross-cutting/VisualDesign.md V8
// *Accessibility*): the whole-window rules, checked over the markup of every component.
describe('icons — the whole window', () => {
  const svelte = sources.filter((f) => f.path.endsWith('.svelte'));
  /** A component's markup: no script, no style, no comment. */
  const markup = (content: string): string =>
    content.replace(/<script[\s\S]*?<\/script>/g, '').replace(/<style[\s\S]*?<\/style>/g, '').replace(/<!--[\s\S]*?-->/g, '');

  it('no character stands in for an icon: no ›, × or ⋯ in markup (prose comments exempt)', () => {
    const found = svelte.filter((f) => /[›×⋯]/.test(markup(f.content))).map((f) => f.path);
    expect(found).toEqual([]);
  });

  it('every icon is drawn through Icon.svelte: no other component writes <use> or an #i- reference', () => {
    const found = sources.filter((f) => f.path !== 'src/lib/icons/Icon.svelte' && (/<use\b/.test(f.path.endsWith('.svelte') ? markup(f.content) : '') || /['"]#i-[a-z]/.test(f.content))).map((f) => f.path);
    expect(found).toEqual([]);
    const icon = svelte.find((f) => f.path === 'src/lib/icons/Icon.svelte');
    expect(markup(icon!.content)).toContain('aria-hidden="true" focusable="false"');
  });

  it('every icon-only button is one of the six allowed kinds, with a non-empty aria-label and title', () => {
    // V8 *Accessibility*: a tree row's action and More actions, a document screen's More actions, a screen
    // tab's or conversation tab's Close, Hide the conversation and Dictate. Each lives in one of these files.
    const allowed = new Set([
      'src/lib/shell/WorkspaceTree.svelte',
      'src/lib/shell/EarlierTreeGroup.svelte',
      'src/lib/shell/DocumentsTreeGroup.svelte',
      'src/lib/views/authoring/DocumentPage.svelte',
      'src/lib/shell/ScreenTabs.svelte',
      'src/lib/shell/ConversationPanel.svelte',
      'src/lib/shell/MessageBox.svelte',
    ]);
    const offenders: string[] = [];
    let seen = 0;
    for (const f of svelte) {
      // Attributes may hold `{…}` expressions (with `=>` and one level of braces inside).
      for (const m of markup(f.content).matchAll(/<(button|summary)\b((?:[^>{]|\{(?:[^{}]|\{[^{}]*\})*\})*)>([\s\S]*?)<\/\1>/g)) {
        const [, , attributes, content] = m as unknown as [string, string, string, string];
        if (!/^\s*<Icon\b[^>]*\/>\s*$/.test(content)) continue;
        seen += 1;
        const named = /aria-label=\{[^}]+\}|aria-label="[^"]+"/.test(attributes) && /\btitle=\{[^}]+\}|\btitle="[^"]+"/.test(attributes);
        if (!allowed.has(f.path) || !named) offenders.push(`${f.path}: ${m[0].slice(0, 120)}`);
      }
    }
    expect(seen).toBeGreaterThan(0);
    expect(offenders).toEqual([]);
  });
});
