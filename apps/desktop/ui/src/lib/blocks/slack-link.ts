// A Slack permalink the core gave: `https://<name>.slack.com/archives/…` and nothing else (ADR-102). The
// program refuses any other address too; this is the screens' own check before sending it. Opening it is
// `app.open_slack_link`, answered by the desktop program (scope `Local`), so it goes to the local backend.
//
// spec: ordin-desktop/components/ScreensModule#UI_LINK_REFUSED
import type { BackendId } from '../backend/backend';
import type { AppContext } from '../shell/context';
import type { ServiceError } from '../service-api/types.gen';

const PERMALINK = /^https:\/\/[A-Za-z0-9-]+\.slack\.com\/archives\/[^\s]+$/;

export function isSlackPermalink(url: string): boolean {
  return PERMALINK.test(url);
}

/** Opens the message in Slack; a link that is not a Slack permalink is not opened and `UI_LINK_REFUSED` is recorded. */
export async function openInSlack(app: AppContext, fromBackend: BackendId, url: string): Promise<ServiceError | null> {
  if (!isSlackPermalink(url)) {
    app.diagnostics.record('UI_LINK_REFUSED', undefined, fromBackend);
    return null;
  }
  const out = await app.local.handle.call('app.open_slack_link', { url });
  return out.kind === 'error' ? out.error : null;
}
