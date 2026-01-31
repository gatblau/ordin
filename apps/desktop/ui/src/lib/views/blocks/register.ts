// Registers how a block is drawn (the conversation's view) and the screen that shows one block in full.
// Picked up by `lib/shell/load-registrations.ts`.
import BlockScreen from '../../blocks/BlockScreen.svelte';
import BlockView from '../../blocks/BlockView.svelte';
import { BLOCK_SCREEN_ID } from '../../blocks/block-screen';
import { registerBlockView, registerScreen } from '../../shell/screens';

registerBlockView(BlockView);
registerScreen({ id: BLOCK_SCREEN_ID, titleId: 'screen-unknown', component: BlockScreen, commands: ['ordin.block'] });
