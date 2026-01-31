// Dev-only review page (not part of the shipped build, which bundles only `index.html`): mounts the
// real screens on the test fake with data like a prototype scene's, for the side-by-side review of
// cross-cutting/VisualDesign.md, Internal Logic step 7. Open `/review/?scene=<id>&theme=light|dark[&dir=rtl]`.
import { mount } from 'svelte';

import App from '../src/App.svelte';
import ConfirmDialog from '../src/lib/views/ConfirmDialog.svelte';
import ErrorLine from '../src/lib/views/ErrorLine.svelte';
import { startApp } from '../src/lib/app/bootstrap';
import '../src/lib/shell/load-registrations';
import '../src/styles/tokens.css';
import { FakeTransport, LOCAL, ok } from '../tests/support/fake-backend';
import { SCENES as BASE } from './scenes';

// Each area's seeds live in their own file, `scenes-<area>.ts`, exporting `SCENES`.
const AREAS = import.meta.glob<{ SCENES: typeof BASE }>('./scenes-*.ts', { eager: true });
const SCENES: typeof BASE = Object.assign({}, BASE, ...Object.values(AREAS).map((m) => m.SCENES));

const query = new URLSearchParams(location.search);
const theme = query.get('theme');
if (theme === 'light' || theme === 'dark') document.documentElement.dataset['theme'] = theme;
// `&dir=rtl` draws the window right to left (V8 *Direction*).
if (query.get('dir') === 'rtl') document.documentElement.dir = 'rtl';

const transport = new FakeTransport();
const scene = SCENES[query.get('scene') ?? 'home'];
scene?.(transport, LOCAL, ok);
const target = document.getElementById('app')!;
const app = await startApp(transport);
// Start-up clears the attribute when the person chose no appearance; the review keeps the one asked for.
if (theme === 'light' || theme === 'dark') document.documentElement.dataset['theme'] = theme;
// Start-up sets the direction from the locale; the review keeps the one asked for.
if (query.get('dir') === 'rtl') document.documentElement.dir = 'rtl';
mount(App, { target, props: { app } });
scene?.after?.(app, transport, LOCAL);

// One part on its own, over the window: `&part=confirm` or `&part=error`.
const part = query.get('part');
if (part === 'confirm') {
  mount(ConfirmDialog, {
    target: document.body,
    props: {
      title: 'Remove Q3 supplier review?',
      body: 'It goes to the Bin with its unsent changes. You can restore it from the Bin for 30 days.',
      verb: 'Remove',
      onconfirm: () => {},
      oncancel: () => {},
    },
  });
} else if (part === 'error') {
  const panel = document.getElementById('screen-panel');
  if (panel !== null) {
    const host = document.createElement('div');
    panel.prepend(host);
    mount(ErrorLine, {
      target: host,
      props: { error: { code: 'FILES_SOURCE_UNREACHABLE', message: 'SharePoint can’t be reached just now.', action: 'try_again' } as never, onretry: () => {} },
    });
  }
}
(window as { reviewReady?: boolean }).reviewReady = true;
