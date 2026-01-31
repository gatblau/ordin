import { mount } from 'svelte';

import App from './App.svelte';
import { startApp } from './lib/app/bootstrap';
import { createTauriTransport } from './lib/backend/tauri';
import './lib/shell/load-registrations';
import './styles/tokens.css';

const target = document.getElementById('app');
if (target === null) throw new Error('UI_NO_MOUNT_POINT');

/** True once the window has drawn: from then on a stray error is not the window failing to draw. */
let drawn = false;

/** A window that stays blank hides its cause, so a failure before the first draw is written on the page. */
function showFailure(error: unknown): void {
  // After the first draw an uncaught error or rejection (the code editor's own "Canceled" when it closes, for
  // one) must not replace a working window with this page (plan 04).
  if (drawn) return;
  const detail = error instanceof Error ? `${error.name}: ${error.message}\n${error.stack ?? ''}` : String(error);
  const box = document.createElement('pre');
  box.setAttribute('role', 'alert');
  box.style.cssText = 'white-space:pre-wrap;padding:16px;font:13px/1.4 ui-monospace,monospace;';
  box.textContent = `ordin could not draw its window.\n\n${detail}`;
  target!.replaceChildren(box);
}
window.addEventListener('error', (event) => showFailure(event.error ?? event.message));
window.addEventListener('unhandledrejection', (event) => showFailure(event.reason));

try {
  const app = await startApp(createTauriTransport());
  mount(App, { target, props: { app } });
  drawn = true;
} catch (error) {
  showFailure(error);
}
