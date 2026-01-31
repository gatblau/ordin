// Settings › Sign-ins: the services ordin can use, always listed, each with the accounts saved for it
// (SM-NS-2.5). A service whose sign-in this version cannot complete is greyed out with one line saying so
// and offers no control (SM-NS-2.9; amended January 2026; user decision; Approved by the user January
// 2026). Jira is connected in Settings › Jira, not here (SM-NS-2.10). Confluence is always available where
// the backend answers `confluence.connect_token`: it can be connected with an API token the person makes
// (SM-NS-2.11; amended January 2026; user decision; Approved by the user January 2026).
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-2
// spec: ordin-desktop/components/ScreensModule#SM-SW-3
// spec: ordin-desktop/components/ScreensModule#SM-NS-2.9
// spec: ordin-desktop/components/ScreensModule#SM-NS-2.11
import type { CredentialKind, CredentialView, MailSignInMethod, OrganisationProfileDocument } from '../../service-api/types.gen';

export type ServiceId = 'github' | 'gitlab' | 'microsoft' | 'google' | 'confluence' | 'slack' | 'mail' | 'oidc';

export interface ServiceDef {
  id: ServiceId;
  kinds: readonly CredentialKind[];
  /** `signin-kind-*` of the product name. */
  nameId: string;
  /** `signin-for-*`: what ordin uses it for. */
  forId: string;
}

/** The services in the order drawn. */
export const SERVICES: readonly ServiceDef[] = [
  { id: 'github', kinds: ['github'], nameId: 'signin-kind-github', forId: 'signin-for-github' },
  { id: 'gitlab', kinds: ['gitlab'], nameId: 'signin-kind-gitlab', forId: 'signin-for-gitlab' },
  { id: 'microsoft', kinds: ['microsoft'], nameId: 'signin-kind-microsoft', forId: 'signin-for-microsoft' },
  { id: 'google', kinds: ['google'], nameId: 'signin-kind-google', forId: 'signin-for-google' },
  { id: 'confluence', kinds: ['confluence'], nameId: 'signin-kind-confluence', forId: 'signin-for-confluence' },
  { id: 'slack', kinds: ['slack'], nameId: 'signin-kind-slack', forId: 'signin-for-slack' },
  { id: 'mail', kinds: ['mail_server_single_sign_on', 'mail_server_password', 'mail_server_app_password'], nameId: 'signin-kind-mail-server-password', forId: 'signin-for-mail' },
  { id: 'oidc', kinds: ['oidc'], nameId: 'signin-kind-oidc', forId: 'signin-for-oidc' },
];

/**
 * Whether this version can complete the service's sign-in (SM-NS-2.9). GitHub and GitLab always can (a
 * token the person makes), and so can Confluence wherever the backend answers `confluence.connect_token`
 * (an API token the person makes, SM-NS-2.11). Microsoft 365, Google, Confluence otherwise and the company
 * account sign in through an app the organisation profile names; Slack and the company's mail server also need their build switch
 * (`answers` says whether the backend answers a command). With no profile read, the profile's part is not
 * known and counts as present.
 */
export function serviceAvailable(id: ServiceId, document: OrganisationProfileDocument | null, answers: (command: string) => boolean): boolean {
  switch (id) {
    case 'github':
    case 'gitlab':
      return true;
    case 'microsoft':
      return document === null || document.microsoft != null;
    case 'google':
      return document === null || document.google != null;
    case 'confluence':
      if (answers('confluence.connect_token')) return true;
      return document === null || document.confluence != null;
    case 'slack':
      if (!answers('slack.status')) return false;
      return document === null || (document.slack?.tools_enabled !== false && !!document.slack?.workspace && !!document.slack?.client_id);
    case 'mail':
      if (!answers('mail.settings')) return false;
      return document === null || (document.email != null && document.email.enabled !== false && document.email.system === 'company_server');
    case 'oidc':
      return document?.identity_provider != null;
  }
}

/** Whether GitLab's one-click sign-in is offered: the profile names a GitLab app (SA-CLOSE-03). */
export function gitlabOneClick(document: OrganisationProfileDocument | null): boolean {
  return !!document?.gitlab?.client_id;
}

/** Whether Confluence's one-click sign-in is offered: the profile names an Atlassian app (SM-NS-2.11). */
export function confluenceOneClick(document: OrganisationProfileDocument | null): boolean {
  return !!document?.confluence?.oauth_client_id;
}

/** The first sign-in method the profile names for the company's mail server (a password when it names none). */
export function mailMethod(document: OrganisationProfileDocument | null): MailSignInMethod {
  return document?.email?.sign_in_methods?.[0] ?? 'mail_password';
}

export interface ServiceRow {
  service: ServiceDef;
  /** A saved account, or `null` for a service with none. */
  credential: CredentialView | null;
  /** False: greyed out, "Not available in this version yet", no control. */
  available: boolean;
  /** A stable key: the credential id, or the service id for a row with none. */
  key: string;
}

/**
 * One row per saved account, under its service; a service with no account has one row of its own. A saved
 * account is always available, so it can be signed out. The company account is listed only when the
 * profile names an identity provider or an account is saved.
 */
export function serviceRows(items: readonly CredentialView[], document: OrganisationProfileDocument | null, answers: (command: string) => boolean = () => true): ServiceRow[] {
  const rows: ServiceRow[] = [];
  for (const service of SERVICES) {
    const available = serviceAvailable(service.id, document, answers);
    const mine = items.filter((c) => service.kinds.includes(c.kind));
    if (mine.length === 0) {
      if (service.id === 'oidc' && !available) continue;
      rows.push({ service, credential: null, available, key: service.id });
    } else for (const credential of mine) rows.push({ service, credential, available: true, key: credential.id });
  }
  return rows;
}
