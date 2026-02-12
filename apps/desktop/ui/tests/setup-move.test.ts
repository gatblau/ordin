// The question to move ordin to the person's Applications folder (ScreensModule.md SM-FS-14): one test per
// Gherkin scenario.
import { beforeAll, describe, expect, it } from 'vitest';

import App from '../src/App.svelte';
import { err, ok, settle } from './support/fake-backend';
import { appWith, html, loadWindowCatalogue } from './support/ssr';
import { text } from './settings-support';

beforeAll(loadWindowCatalogue);

const NOT_INSTALLED = ok({ installed: false, target: '/Users/asha/Applications/ordin.app' });
const READY = ok({ needs_guided: false, progress: { done: 0, total: 0, running: false, steps: [] }, attachments: [] });

describe('The question to move ordin (SM-FS-14)', () => {
  it('Happy path — the question comes before the checks, and Move ordin sends the move', async () => {
    const { app, transport } = await appWith((t) => {
      t.on('app.install_place', NOT_INSTALLED);
      t.on('app.move_to_applications', () => new Promise(() => undefined) as never);
    });
    expect(app.firstStart.phase).toBe('move');
    expect(transport.commandsSent()).not.toContain('setup.status');
    const out = text(html(App, { app }, app));
    expect(out).toContain('Move ordin to your Applications folder?');
    expect(out).toContain('No administrator is needed.');
    expect(out).toContain('Move ordin');
    expect(out).toContain('Not now');
    void app.firstStart.moveToApplications();
    await settle();
    expect(transport.sentOf('app.move_to_applications')).toHaveLength(1);
    expect(app.firstStart.moving).toBe(true);
    expect(text(html(App, { app }, app))).toContain('Moving ordin…');
  });

  it('Edge case — Not now carries on with first start, and nothing is moved', async () => {
    const { app, transport } = await appWith((t) => {
      t.on('app.install_place', NOT_INSTALLED);
      t.on('setup.status', READY);
    });
    await app.firstStart.notNow();
    expect(app.firstStart.phase).toBe('regions');
    expect(transport.sentOf('app.move_to_applications')).toHaveLength(0);
    expect(transport.sentOf('app.install_place')).toHaveLength(1);
  });

  it('Error path — a failed copy is drawn with Try again, and Not now stays', async () => {
    const { app } = await appWith((t) => {
      t.on('app.install_place', NOT_INSTALLED);
      t.on('app.move_to_applications', err('DSK_MOVE_COPY_FAILED', "ordin couldn't copy itself to your Applications folder.", { action: 'try_again' }));
    });
    await app.firstStart.moveToApplications();
    expect(app.firstStart.moving).toBe(false);
    expect(app.firstStart.moveError?.code).toBe('DSK_MOVE_COPY_FAILED');
    const out = text(html(App, { app }, app));
    expect(out).toContain("ordin couldn't copy itself to your Applications folder.");
    expect(out).toContain('Try again');
    expect(out).toContain('Not now');
  });

  it('Edge case — ordin already installed, or a backend without the command: nothing new is drawn', async () => {
    for (const answer of [ok({ installed: true, target: '' }), err('SVC_NOT_IN_THIS_VERSION', 'Not here.')]) {
      const { app, transport } = await appWith((t) => {
        t.on('app.install_place', answer);
        t.on('setup.status', READY);
      });
      expect(app.firstStart.phase).toBe('regions');
      expect(transport.commandsSent()).toContain('setup.status');
    }
  });
});
