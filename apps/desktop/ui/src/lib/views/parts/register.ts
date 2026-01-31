// Registers the import screen, Show the installed files and the part pages (SM-NS-10, SM-NS-11,
// SM-NS-15), each offered only when the backend answers every command it sends (SM-UP-1). The part page
// is offered when it can draw either of its two forms, so its commands are those of the installed form
// plus the library form's.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-10
// spec: ordin-desktop/components/ScreensModule#SM-NS-11
// spec: ordin-desktop/components/ScreensModule#SM-NS-15
import { registerScreen } from '../../shell/screens';
import ImportScreen from './ImportScreen.svelte';
import InstalledFilesScreen from './InstalledFilesScreen.svelte';
import PartPageScreen from './PartPageScreen.svelte';

export const IMPORT_SCREEN = 'import';
export const INSTALLED_FILES_SCREEN = 'installed-files';
export const PART_PAGE_SCREEN = 'part-page';

registerScreen({ id: IMPORT_SCREEN, titleId: 'import-title', component: ImportScreen, commands: ['authoring.import', 'catalogue.list_repositories', 'app.pick_folder'] });
registerScreen({ id: INSTALLED_FILES_SCREEN, titleId: 'installed-files-title', component: InstalledFilesScreen, commands: ['agents.installed_files'] });
registerScreen({ id: PART_PAGE_SCREEN, titleId: 'check-it', component: PartPageScreen, commands: ['agents.part_page', 'catalogue.part', 'catalogue.used_by'] });
