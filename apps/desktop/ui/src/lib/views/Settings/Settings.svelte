<script lang="ts">
  // Settings: the sections in 11 §4's order, each listed only when the backend answers its commands
  // (SM-UP-1), one open at a time. The section list is a column of buttons beside the open section, so Tab
  // moves from one present section to the next; a narrow window shows a "Section" menu instead.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-1
  // spec: ordin-desktop/components/ScreensModule#SM-SW-1
  import { t } from '../../i18n/fluent';
  import { getApp } from '../../shell/context';
  import type { ScreenProps } from '../../shell/screens';
  import Diagnostics from '../Diagnostics.svelte';
  import About from './About.svelte';
  import AgentRepositories from './AgentRepositories.svelte';
  import AgentsSettings from './AgentsSettings.svelte';
  import CodeSection from './CodeSection.svelte';
  import Dictation from './Dictation.svelte';
  import Display from './Display.svelte';
  import Email from './Email.svelte';
  import History from './History.svelte';
  import Jira from './Jira.svelte';
  import KnowledgeAgents from './KnowledgeAgents.svelte';
  import MeetingMinutes from './MeetingMinutes.svelte';
  import MemorySettings from './MemorySettings.svelte';
  import Language from './Language.svelte';
  import Notifications from './Notifications.svelte';
  import Plan from './Plan.svelte';
  import { visibleSections } from './sections';
  import Servers from './Servers.svelte';
  import SignIns from './SignIns.svelte';
  import Slack from './Slack.svelte';
  import Telegram from './Telegram.svelte';
  import Voice from './Voice.svelte';

  interface Props extends ScreenProps {
    /** Which section is open first (a test, or `sign_in_again` opening Settings › Sign-ins). */
    section?: string;
  }
  let { backend, instance, section }: Props = $props();
  const app = getApp();
  // Settings belongs to this computer.
  const local = $derived(backend.kind === 'local' ? backend : app.local);
  const sections = $derived(visibleSections(local));
  // svelte-ignore state_referenced_locally
  let chosen = $state<string | null>(section ?? instance ?? null);
  const current = $derived(sections.find((s) => s.id === chosen) ?? sections[0] ?? null);
  /** From this width of the Settings screen itself the sections are listed down the side. */
  const WIDE = 700;
  let width = $state(0);
</script>

<!-- Laid out by its own width, not the window's: with the side panel and the conversation open, a wide window
     can leave the screen area narrow (amended January 2026; user decision; Approved by the user January 2026). -->
<div class="settings" class:wide={width >= WIDE} bind:clientWidth={width}>
  <h2 id="settings-heading" tabindex="-1" data-region-focus>{t('screen-settings')}</h2>
  <!-- One section at a time: a side list of the sections on a window at least 700 px wide, a "Section"
       menu below that (amended January 2026 (user decision)). -->
  <nav aria-labelledby="settings-heading">
    <ul class="section-list">
      {#each sections as s (s.id)}
        <li>
          <button type="button" aria-current={current?.id === s.id ? 'page' : undefined} onclick={() => (chosen = s.id)}>{t(s.titleId)}</button>
        </li>
      {/each}
    </ul>
    <label class="section-menu">
      {t('settings-section-menu')}
      <select value={current?.id ?? ''} onchange={(e) => (chosen = e.currentTarget.value)}>
        {#each sections as s (s.id)}<option value={s.id}>{t(s.titleId)}</option>{/each}
      </select>
    </label>
  </nav>
  <div class="section-body">
    {#if current !== null}
      {#if current.id === 'sign-ins'}<SignIns backend={local} />
      {:else if current.id === 'slack'}<Slack backend={local} />
      {:else if current.id === 'jira'}<Jira backend={local} />
      {:else if current.id === 'servers'}<Servers backend={local} />
      {:else if current.id === 'language'}<Language backend={local} />
      {:else if current.id === 'history'}<History backend={local} />
      {:else if current.id === 'display'}<Display backend={local} />
      {:else if current.id === 'knowledge-agents'}<KnowledgeAgents backend={local} />
      {:else if current.id === 'agent-repositories'}<AgentRepositories backend={local} />
      {:else if current.id === 'email'}<Email backend={local} />
      {:else if current.id === 'telegram'}<Telegram backend={local} />
      {:else if current.id === 'dictation'}<Dictation backend={local} />
      {:else if current.id === 'meeting-minutes'}<MeetingMinutes backend={local} />
      {:else if current.id === 'how-ordin-talks'}<Voice backend={local} />
      {:else if current.id === 'using-your-plan'}<Plan backend={local} />
      {:else if current.id === 'agents-and-models'}<AgentsSettings backend={local} />
      {:else if current.id === 'notifications'}<Notifications backend={local} />
      {:else if current.id === 'code'}<CodeSection backend={local} />
      {:else if current.id === 'what-ordin-remembers'}
        <!-- Settings only, in three groups; the facts stay on the memory page (SM-NS-1.9, amended January
             2026; user decision; Approved by the user January 2026). -->
        <MemorySettings backend={local} prefs={app.prefs} />
      {:else if current.id === 'diagnostics'}<Diagnostics backend={local} workspaceId={null} instance={null} />
      {:else if current.id === 'about'}<About backend={local} />
      {:else}
        <h3>{t(current.titleId)}</h3>
      {/if}
    {/if}
  </div>
</div>

<style>
  /* Settings (scene `settings`): a screen of at most 860 px, its open section drawn as a screen section. */
  .settings {
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    max-inline-size: 860px;
  }
  .settings > h2 {
    font-size: 18px;
  }
  /* Below 700 px of its own width: the "Section" menu above the open section; the side list is not drawn. */
  .section-list {
    display: none;
    flex-direction: column;
    gap: 2px;
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .section-menu {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
    font-size: 13px;
  }
  /* A screen area at least 700 px wide (not the window: the side panel and the conversation take room from it;
     amended January 2026, user decision; Approved by the user January 2026): the sections listed down the
     side, the open one beside them. */
  .settings.wide {
    display: grid;
    grid-template-columns: 190px minmax(0, 1fr);
    align-items: start;
  }
  .settings.wide > h2 {
    grid-column: 1 / -1;
  }
  .settings.wide .section-list {
    display: flex;
  }
  .settings.wide .section-menu {
    display: none;
  }
  .section-list button {
    inline-size: 100%;
    text-align: start;
    border-color: transparent;
    background: none;
    font-size: 13px;
    min-height: 30px;
    padding: var(--space-04) var(--space-sm);
  }
  /* The open section's button is drawn as a pressed toggle (V6). */
  .section-list button[aria-current='page'] {
    background: var(--color-accent-soft);
    border-color: var(--color-accent);
    color: var(--color-accent-ink);
    font-weight: 600;
  }
  .section-body {
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    min-inline-size: 0;
  }
  .section-body > :global(section) {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-lg);
    padding: var(--space-14);
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .section-body > :global(h3),
  .section-body > :global(section > h3) {
    font-size: 15px;
  }
</style>
