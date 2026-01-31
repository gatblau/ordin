// "Open in Slack" opens only a permalink the core gave, and only one of the form
// `https://<name>.slack.com/archives/…`; anything else is refused with a diagnostic and nothing opens.
//
// spec: ordin-desktop/components/ScreensModule#UI_LINK_REFUSED
import type { DiagnosticSink } from '../../backend/backend';
import type { Handle } from './util';

export function isSlackPermalink(url: string): boolean {
  try {
    const u = new URL(url);
    return u.protocol === 'https:' && /^[a-z0-9-]+\.slack\.com$/i.test(u.hostname) && u.username === '' && u.password === '' && u.pathname.startsWith('/archives/');
  } catch {
    return false;
  }
}

/** Sends `app.open_slack_link`; a URL that is not a Slack permalink sends nothing and records `UI_LINK_REFUSED`. */
export async function openSlackPermalink(handle: Handle, url: string, diagnostics?: DiagnosticSink): Promise<boolean> {
  if (!isSlackPermalink(url)) {
    diagnostics?.record('UI_LINK_REFUSED', undefined, handle.id);
    return false;
  }
  const out = await handle.call('app.open_slack_link', { url });
  return out.kind === 'ok';
}
