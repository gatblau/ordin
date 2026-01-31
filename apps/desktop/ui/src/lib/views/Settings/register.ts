// Registers Settings, Diagnostics, the Language screens and the update line / What's new card of
// *Needs you* (picked up by lib/shell/load-registrations.ts). A screen is offered only when the backend
// answers its commands (SM-UP-1).
//
// spec: ordin-desktop/components/ScreensModule#SM-UP-1
import { registerNeedsYouSlot, registerScreen, SCREEN_IDS } from '../../shell/screens';
import Diagnostics from '../Diagnostics.svelte';
import LanguageScreen from './LanguageScreen.svelte';
import ServerLanguages from './ServerLanguages.svelte';
import Settings from './Settings.svelte';
import UpdateLine from './UpdateLine.svelte';
import WhatsNewCard from './WhatsNewCard.svelte';

registerScreen({ id: SCREEN_IDS.settings, titleId: 'screen-settings', component: Settings, commands: ['settings.get'] });
registerScreen({ id: SCREEN_IDS.diagnostics, titleId: 'screen-diagnostics', component: Diagnostics, commands: ['diagnostics.lines'] });
registerScreen({ id: 'language', titleId: 'settings-section-language', component: LanguageScreen, commands: ['localisation.list_messages'] });
registerScreen({ id: 'server-languages', titleId: 'settings-section-language', component: ServerLanguages, commands: ['localisation.list_languages'] });

// What's new first, so its card sits above the update line (SM-UP-4).
registerNeedsYouSlot({ id: 'whats-new', component: WhatsNewCard });
registerNeedsYouSlot({ id: 'update-line', component: UpdateLine });
