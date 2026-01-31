// The Settings sections, in 11 §4's order (ScreensModule.md step 9, Settings row, and the Slack, Jira,
// Language, Knowledge agents and About amendments). A section is listed for a backend only when the
// backend answers every command the section sends (SM-UP-1); a section whose commands are all absent is
// left out and the next present section takes its place in keyboard order.
//
// spec: ordin-desktop/components/ScreensModule#SM-UP-1
import { answersAll, type HasGroups } from '../../backend/groups';

export interface SectionDef {
  /** The part after `settings-section-`. */
  id: string;
  titleId: string;
  /** The commands the section sends to the backend. */
  commands: readonly string[];
  /** Drawn for the local backend only (About). */
  localOnly?: boolean;
}

export const SECTIONS: readonly SectionDef[] = [
  { id: 'sign-ins', titleId: 'settings-section-sign-ins', commands: ['identity.list'] },
  { id: 'agent-repositories', titleId: 'settings-section-agent-repositories', commands: ['catalogue.list_repositories'] },
  { id: 'email', titleId: 'settings-section-email', commands: ['mail.settings'] },
  { id: 'telegram', titleId: 'settings-section-telegram', commands: ['telegram.status'] },
  { id: 'slack', titleId: 'settings-section-slack', commands: ['slack.status'] },
  { id: 'jira', titleId: 'settings-section-jira', commands: ['jira.connect_token'] },
  { id: 'servers', titleId: 'settings-section-servers', commands: ['servers.list'] },
  { id: 'dictation', titleId: 'settings-section-dictation', commands: ['dictation.get_availability'] },
  { id: 'how-ordin-talks', titleId: 'settings-section-how-ordin-talks', commands: ['voice.get'] },
  { id: 'language', titleId: 'settings-section-language', commands: ['localisation.get_settings', 'localisation.list_languages'] },
  { id: 'using-your-plan', titleId: 'settings-section-using-your-plan', commands: ['usage.get_settings'] },
  { id: 'notifications', titleId: 'settings-section-notifications', commands: ['settings.get', 'settings.change'] },
  { id: 'history', titleId: 'settings-section-history', commands: ['history.get', 'history.set_days'] },
  { id: 'display', titleId: 'settings-section-display', commands: ['settings.get', 'settings.change'] },
  { id: 'code', titleId: 'settings-section-code', commands: ['projects.ides', 'projects.language_plugins'] },
  { id: 'what-ordin-remembers', titleId: 'settings-section-what-ordin-remembers', commands: ['memory.list'] },
  { id: 'knowledge-agents', titleId: 'settings-section-knowledge-agents', commands: ['knowledge_agents.list'] },
  { id: 'diagnostics', titleId: 'settings-section-diagnostics', commands: ['diagnostics.lines'] },
  { id: 'about', titleId: 'settings-section-about', commands: ['setup.check_update'], localOnly: true },
];

/** The sections a backend's Settings lists, in order. */
export function visibleSections(backend: HasGroups & { readonly kind?: 'local' | 'server' }): SectionDef[] {
  return SECTIONS.filter((s) => !(s.localOnly === true && backend.kind === 'server') && answersAll(backend, s.commands));
}
