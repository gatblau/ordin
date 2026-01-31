// Registers Home, Earlier conversations (the list and one read-only conversation) and *Search or go to…*
// (picked up by lib/shell/load-registrations.ts). Entries are offered only when the backend answers their
// commands (SM-UP-1).
//
// spec: ordin-desktop/components/ScreensModule#SM-UP-1
import { registerOverlay, registerScreen, SCREEN_IDS } from '../../shell/screens';
import EarlierConversation from '../EarlierConversation.svelte';
import EarlierConversations from '../EarlierConversations.svelte';
import Home from '../Home.svelte';
import RunningNow from '../RunningNow.svelte';
import RunScreen from '../RunScreen.svelte';
import SearchDialog from '../SearchDialog.svelte';

registerScreen({ id: SCREEN_IDS.home, titleId: 'rail-home', component: Home, commands: ['home.view'] });
registerScreen({ id: SCREEN_IDS.earlierConversations, titleId: 'conv-earlier-name', component: EarlierConversations, commands: ['ordin.list_conversations'] });
registerScreen({ id: 'earlier-conversation', titleId: 'conv-earlier-name', component: EarlierConversation, commands: ['ordin.read_conversation'] });
// SM-RN-1: the workspace tree's Running now.
registerScreen({ id: SCREEN_IDS.running, titleId: 'tree-running', component: RunningNow, commands: ['home.view'] });
// SM-RS-1: one run, from Home, Running now and progress cards.
registerScreen({ id: SCREEN_IDS.run, titleId: 'screen-run', component: RunScreen, commands: ['runs.get'] });
registerOverlay({ id: 'search', component: SearchDialog });
