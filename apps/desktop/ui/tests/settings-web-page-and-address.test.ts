// The Settings walk-through's web pages and a server's address (ScreensModule.md SM-SW-5, SM-SW-11 and
// SM-SW-12, closing G-SW-1 and G-SW-3; ServiceApi.md SVC-WEB-1 and SVC-SRV-1; amended January 2026 (user
// decision)).
import { beforeAll, describe, expect, it } from 'vitest';

import { err, ok, R1_GROUPS } from './support/fake-backend';
import { uid } from './support/ids';
import { html, loadWindowCatalogue } from './support/ssr';
import { loaded, text } from './settings-support';
import Jira from '../src/lib/views/Settings/Jira.svelte';
import Servers from '../src/lib/views/Settings/Servers.svelte';
import Telegram from '../src/lib/views/Settings/Telegram.svelte';
import { ServersModel } from '../src/lib/views/Settings/servers-model.svelte';
import { JiraModel } from '../src/lib/views/Settings/signins-model.svelte';
import { TelegramModel } from '../src/lib/views/Settings/telegram-model.svelte';
import { WebPageOpener } from '../src/lib/views/Settings/web-page.svelte';

beforeAll(loadWindowCatalogue);

const clean = (markup: string) => markup.replace(/<!--.*?-->/g, '');
const apostrophes = (s: string) => s.replace(/&#39;|&apos;|’/g, "'");
const GROUPS = [...R1_GROUPS, 'telegram'];
const BROWSER = "ordin couldn't open your web browser. Open it yourself and try again.";

describe('SM-SW-5 — Open BotFather in Telegram', () => {
  it('Happy path — the step draws the button, and pressing it sends app.open_web_page with the page name only', async () => {
    const t = await loaded(GROUPS, (tr) => tr.on('telegram.status', ok({ offered: true, connected: false })).on('app.open_web_page', ok({})));
    const model = new TelegramModel(t.handle);
    await model.load();
    const pages = new WebPageOpener(t.handle);
    expect(text(clean(html(Telegram, { backend: t.state, model, pages })))).toContain('Open BotFather in Telegram');
    expect(await pages.open('telegram_botfather')).toBe(true);
    expect(t.transport.sentOf('app.open_web_page').map((r) => r.input)).toEqual([{ page: 'telegram_botfather' }]);
    expect(pages.error).toBeNull();
  });
});

describe('SM-SW-11 — Open Atlassian’s token page', () => {
  it('Happy path — the button sits beside Copy the address, which stays as the fallback', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('identity.list', ok({ items: [] })).on('app.open_web_page', ok({})));
    const model = new JiraModel(t.handle);
    await model.load();
    const pages = new WebPageOpener(t.handle);
    const out = apostrophes(text(clean(html(Jira, { backend: t.state, model, pages }))));
    expect(out).toContain("Open Atlassian's token page");
    expect(out).toContain('Copy the address');
    expect(out).toContain('https://id.atlassian.com/manage-profile/security/api-tokens');
    await pages.open('atlassian_api_tokens');
    expect(t.transport.sentOf('app.open_web_page')[0]?.input).toEqual({ page: 'atlassian_api_tokens' });
  });

  it('Error path — the browser does not open: its sentence is drawn with Try again, which asks again', async () => {
    const t = await loaded(R1_GROUPS, (tr) =>
      tr.on('identity.list', ok({ items: [] })).on('app.open_web_page', err('CRED_BROWSER_FAILED', BROWSER, { action: 'try_again' })),
    );
    const model = new JiraModel(t.handle);
    await model.load();
    const pages = new WebPageOpener(t.handle);
    expect(await pages.open('atlassian_api_tokens')).toBe(false);
    expect(pages.error?.code).toBe('CRED_BROWSER_FAILED');
    const out = apostrophes(text(clean(html(Jira, { backend: t.state, model, pages }))));
    expect(out).toContain(BROWSER);
    expect(out).toContain('Copy the address');
    await pages.retry();
    expect(t.transport.sentOf('app.open_web_page')).toHaveLength(2);
  });
});

describe('SM-SW-12 — Connect again… fills in the address', () => {
  const server = { server_connection_id: uid('s1'), display_name: 'Team server', state: 'not_connected', backend: null, address: '10.20.4.17:8443' };

  it('Happy path — the address box holds the server’s address and the code box is empty', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('servers.list', ok([server])));
    const model = new ServersModel(t.handle, () => undefined);
    await model.load();
    model.code = 'left over';
    model.openConnect(uid('s1'));
    expect(model.address).toBe('10.20.4.17:8443');
    expect(model.code).toBe('');
    expect(clean(html(Servers, { backend: t.state, model }))).toContain('value="10.20.4.17:8443"');
  });

  it('Edge case — Connect to a new server opens with an empty address', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('servers.list', ok([server])));
    const model = new ServersModel(t.handle, () => undefined);
    await model.load();
    model.openConnect();
    expect(model.address).toBe('');
  });
});
