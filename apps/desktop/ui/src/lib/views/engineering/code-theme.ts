// The code editor's and the terminal's colours, read from the theme's custom properties
// (cross-cutting/VisualDesign.md, Internal Logic step 6) and applied again on each theme change.
import type * as Monaco from 'monaco-editor';

const SYNTAX = ['keyword', 'string', 'comment', 'number', 'type', 'function'] as const;

/** One property's value in the current theme. */
export type ReadProperty = (name: string) => string;

export function readFromDocument(): ReadProperty {
  const style = getComputedStyle(document.documentElement);
  return (name) => style.getPropertyValue(name).trim();
}

/** Whether the window is in its dark theme: `data-theme`, else the operating system's choice. */
export function isDark(): boolean {
  const chosen = document.documentElement.dataset['theme'];
  if (chosen === 'dark' || chosen === 'light') return chosen === 'dark';
  return matchMedia('(prefers-color-scheme: dark)').matches;
}

/** Monaco's theme data from the properties: `ordin-dark` on `vs-dark`, else `ordin-light` on `vs`. */
export function monacoTheme(read: ReadProperty, dark: boolean): { name: string; data: Monaco.editor.IStandaloneThemeData } {
  const hex = (name: string): string => read(name).replace(/^#/, '');
  return {
    name: dark ? 'ordin-dark' : 'ordin-light',
    data: {
      base: dark ? 'vs-dark' : 'vs',
      inherit: true,
      // A token whose colour property is not set keeps the base theme's colour: Monaco throws on an empty one.
      rules: SYNTAX.map((token) => ({ token, foreground: hex(`--color-syntax-${token}`) })).filter((rule) => rule.foreground !== ''),
      colors: { 'editor.background': read('--color-surface'), 'editor.foreground': read('--color-text') },
    },
  };
}

/** The terminal's colours from the properties. */
export function terminalTheme(read: ReadProperty): { background: string; foreground: string; cursor: string } {
  return { background: read('--color-surface'), foreground: read('--color-text'), cursor: read('--color-accent') };
}

/** Calls `apply` on every theme change (the operating system's or `data-theme`); returns the stop. */
export function watchTheme(apply: () => void): () => void {
  const scheme = matchMedia('(prefers-color-scheme: dark)');
  scheme.addEventListener('change', apply);
  const attributes = new MutationObserver(apply);
  attributes.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'style'] });
  return () => {
    scheme.removeEventListener('change', apply);
    attributes.disconnect();
  };
}
