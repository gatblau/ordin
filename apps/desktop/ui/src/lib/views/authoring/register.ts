// Registers the authoring screens: the document page, the workflow screen, the agent draft and the agent
// repository screens (SM-NS-4 to SM-NS-7). Each is offered only when the backend answers the commands that
// open it (SM-UP-1); an entry on a screen whose group is absent is not drawn.
//
// spec: ordin-desktop/components/ScreensModule#SM-CU-2
// spec: ordin-desktop/components/ScreensModule#SM-UP-1
import { registerScreen, SCREEN_IDS } from '../../shell/screens';
import AgentDraft from './AgentDraft.svelte';
import DocumentPage from './DocumentPage.svelte';
import PartDraft from './PartDraft.svelte';
import RepositoryParts from './RepositoryParts.svelte';
import WithdrawPart from './WithdrawPart.svelte';
import WorkflowScreen from './WorkflowScreen.svelte';

registerScreen({ id: SCREEN_IDS.document, titleId: 'screen-document', component: DocumentPage, commands: ['files.preview', 'files.versions', 'home.document_opened'] });
registerScreen({ id: 'workflow', titleId: 'tree-workflows', component: WorkflowScreen, commands: ['workflows.get'] });
registerScreen({ id: 'agent-draft', titleId: 'tree-agents', component: AgentDraft, commands: ['authoring.agent_draft', 'catalogue.list_repositories'] });
registerScreen({ id: 'repository-parts', titleId: 'repo-tab-parts', component: RepositoryParts, commands: ['catalogue.list_repositories', 'catalogue.search'] });
registerScreen({ id: 'part-draft', titleId: 'repo-tab-parts', component: PartDraft, commands: ['authoring.part_draft'] });
registerScreen({ id: 'withdraw-part', titleId: 'repo-withdraw', component: WithdrawPart, commands: ['catalogue.part', 'catalogue.used_by', 'catalogue.search', 'authoring.withdraw'] });
