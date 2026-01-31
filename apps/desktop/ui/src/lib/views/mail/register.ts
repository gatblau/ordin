// Registers the email screens (SM-NS-12): a link's messages under the reserved `email` identifier, one
// message and one draft under the identifiers chosen by the spec. Each is offered only when the backend
// answers every command it sends (SM-UP-1).
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-12
import { registerScreen, SCREEN_IDS } from '../../shell/screens';
import EmailDraftScreen from './EmailDraftScreen.svelte';
import EmailListScreen from './EmailListScreen.svelte';
import EmailMessageScreen from './EmailMessageScreen.svelte';

export const EMAIL_MESSAGE_SCREEN = 'email-message';
export const EMAIL_DRAFT_SCREEN = 'email-draft';

registerScreen({ id: SCREEN_IDS.email, titleId: 'tree-email', component: EmailListScreen, commands: ['workspaces.email_links', 'mail.list_messages', 'mail.mark_looked'] });
registerScreen({ id: EMAIL_MESSAGE_SCREEN, titleId: 'tree-email', component: EmailMessageScreen, commands: ['mail.open_message', 'mail.save_attachment', 'ordin.send_message'] });
registerScreen({ id: EMAIL_DRAFT_SCREEN, titleId: 'tree-email', component: EmailDraftScreen, commands: ['mail.get_draft', 'mail.edit_draft', 'mail.send', 'mail.save_to_mail_drafts', 'mail.discard_draft'] });
