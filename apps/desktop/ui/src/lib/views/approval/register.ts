// Registers the fixed approval card as the screen `approval:<approval id>` (picked up by
// `lib/shell/load-registrations.ts`).
import ApprovalScreen from '../../approval/ApprovalScreen.svelte';
import { registerScreen, SCREEN_IDS } from '../../shell/screens';

registerScreen({ id: SCREEN_IDS.approval, titleId: 'screen-approval', component: ApprovalScreen, commands: ['approval.decide'] });
