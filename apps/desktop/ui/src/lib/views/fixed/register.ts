// Registers the fixed screens of 11 §5.15 that this module draws, each offered only when the backend
// answers every command it sends (SM-UP-1). The identifiers are the fixed ones of `SCREEN_IDS`; the Slack
// channel screen's is chosen here.
//
// Not registered: Email (drawn by lib/views/mail/register.ts: `workspaces.email_links` returns
// `EmailLink.display_name`, which names a link, SM-NS-12) and the screens other modules own (Home, Settings, Diagnostics, Earlier conversations,
// Running now, Approval, Document, Run, Crew).
//
// spec: ordin-desktop/components/ScreensModule#SM-UP-1
import { registerScreen, SCREEN_IDS } from '../../shell/screens';
import Agents from './Agents.svelte';
import Dashboards from './Dashboards.svelte';
import Documents from './Documents.svelte';
import Knowledge from './Knowledge.svelte';
import Library from './Library.svelte';
import Memory from './Memory.svelte';
import Slack from './Slack.svelte';
import SlackChannel from './SlackChannel.svelte';
import Telegram from './Telegram.svelte';
import Workflows from './Workflows.svelte';

export const SLACK_CHANNEL_SCREEN = 'slack-channel';

registerScreen({ id: SCREEN_IDS.documents, titleId: 'tree-documents', component: Documents, commands: ['files.list_folder'] });
registerScreen({ id: SCREEN_IDS.agents, titleId: 'tree-agents', component: Agents, commands: ['agents.list', 'agents.made_of', 'checker.check_installed'] });
// spec: ordin-desktop/components/ScreensModule#SM-AL-1
registerScreen({ id: SCREEN_IDS.library, titleId: 'library-title', component: Library, commands: ['catalogue.search', 'agents.preview_add', 'agents.add'] });
registerScreen({ id: SCREEN_IDS.workflows, titleId: 'tree-workflows', component: Workflows, commands: ['workflows.list'] });
registerScreen({ id: SCREEN_IDS.dashboards, titleId: 'tree-dashboards', component: Dashboards, commands: ['dashboards.list'] });
registerScreen({ id: SCREEN_IDS.telegram, titleId: 'tree-telegram', component: Telegram, commands: ['telegram.links'] });
registerScreen({ id: SCREEN_IDS.slack, titleId: 'tree-slack', component: Slack, commands: ['slack.links'] });
registerScreen({
  id: SLACK_CHANNEL_SCREEN,
  titleId: 'tree-slack',
  component: SlackChannel,
  commands: ['slack.links', 'slack.messages', 'slack.set_ordin_reads', 'slack.unlink', 'app.open_slack_link'],
});
registerScreen({ id: SCREEN_IDS.knowledge, titleId: 'tree-knowledge', component: Knowledge, commands: ['knowledge.list', 'knowledge.get'] });
registerScreen({ id: SCREEN_IDS.memory, titleId: 'tree-memory', component: Memory, commands: ['memory.list', 'memory.things'] });
