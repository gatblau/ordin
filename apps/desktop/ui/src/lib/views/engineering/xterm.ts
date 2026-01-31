// xterm.js is loaded on first use (the initial JavaScript budget). It receives bytes only.
import type { Terminal as XTerm } from '@xterm/xterm';

let loading: Promise<{ Terminal: typeof XTerm }> | null = null;

export function loadXterm(): Promise<{ Terminal: typeof XTerm }> {
  loading ??= (async () => {
    const [mod] = await Promise.all([import('@xterm/xterm'), import('@xterm/xterm/css/xterm.css')]);
    return { Terminal: mod.Terminal };
  })();
  return loading;
}
