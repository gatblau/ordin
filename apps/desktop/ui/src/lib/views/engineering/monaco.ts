// Monaco is loaded on first use, never at start, so the initial JavaScript stays inside its 600 KB
// gzipped budget (Performance, "initial JavaScript"). Monaco receives text only.
import type * as Monaco from 'monaco-editor';

import { isDark, monacoTheme, readFromDocument, watchTheme } from './code-theme';

let loading: Promise<typeof Monaco> | null = null;

export function loadMonaco(): Promise<typeof Monaco> {
  loading ??= (async () => {
    const [monaco, worker] = await Promise.all([import('monaco-editor'), import('monaco-editor/editor/editor.worker?worker')]);
    const EditorWorker = worker.default;
    (globalThis as { MonacoEnvironment?: unknown }).MonacoEnvironment = { getWorker: () => new EditorWorker() };
    // Monaco's theme is global: defined from the current theme's properties, and again on each change.
    const apply = (): void => {
      const theme = monacoTheme(readFromDocument(), isDark());
      monaco.editor.defineTheme(theme.name, theme.data);
      monaco.editor.setTheme(theme.name);
    };
    apply();
    watchTheme(apply);
    return monaco;
  })();
  return loading;
}

/** A Monaco language id for a path, by extension; `plaintext` when none is known. */
export function languageFor(path: string, known: Array<{ id: string; extensions?: string[] }>): string {
  const dot = path.lastIndexOf('.');
  if (dot < 0) return 'plaintext';
  const ext = path.slice(dot).toLowerCase();
  return known.find((l) => l.extensions?.includes(ext))?.id ?? 'plaintext';
}
