<script lang="ts">
  // Settings: the sections in 11 §4's order, each listed only when the backend answers its commands
  // (SM-UP-1). The section list is a row of buttons, so Tab moves from one present section to the next.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-1
  import { t } from '../../i18n/fluent';
  import { getApp } from '../../shell/context';
  import type { ScreenProps } from '../../shell/screens';
  import Diagnostics from '../Diagnostics.svelte';
  import Memory from '../fixed/Memory.svelte';
  import About from './About.svelte';
  import AgentRepositories from './AgentRepositories.svelte';
  import CodeSection from './CodeSection.svelte';
  import Dictation from './Dictation.svelte';
  import Display from './Display.svelte';
  import Email from './Email.svelte';
  import History from './History.svelte';
  import Jira from './Jira.svelte';
  import KnowledgeAgents from './KnowledgeAgents.svelte';
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
  // What Ordin remembers draws the memory view of the workspace in front (SM-NS-1.9).
  const front = $derived(app.shell.openWorkspace);
  const frontName = $derived(local.workspaces.find((w) => w.id === front)?.name ?? null);
</script>

<div class="settings">
  <h2 id="settings-heading" tabindex="-1" data-region-focus>{t('screen-settings')}</h2>
  <nav aria-labelledby="settings-heading">
    <ul class="section-list">
      {#each sections as s (s.id)}
        <li>
          <button type="button" aria-current={current?.id === s.id ? 'page' : undefined} onclick={() => (chosen = s.id)}>{t(s.titleId)}</button>
        </li>
      {/each}
    </ul>
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
      {:else if current.id === 'how-ordin-talks'}<Voice backend={local} />
      {:else if current.id === 'using-your-plan'}<Plan backend={local} />
      {:else if current.id === 'notifications'}<Notifications backend={local} />
      {:else if current.id === 'code'}<CodeSection backend={local} />
      {:else if current.id === 'what-ordin-remembers'}
        <h3>{t(current.titleId)}</h3>
        {#if front !== null && frontName !== null}
          <p>{t('settings-memory-workspace', { workspace: frontName })}</p>
          {#key front}<Memory backend={local} workspaceId={front} instance={null} />{/key}
        {:else}
          <p>{t('settings-memory-no-workspace')}</p>
        {/if}
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
  .section-list {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
    list-style: none;
    margin: 0;
    padding: 0;
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
  /* SM-FL-1: a field's label sits above its control and each field takes its own line (the Jira form read as one run of words). */
  .section-body :global(label:has(> input:not([type='checkbox']):not([type='radio']):not([type='range']))),
  .section-body :global(label:has(> select)),
  .section-body :global(label:has(> textarea)) {
    display: grid;
    gap: var(--space-04);
    margin-block-end: var(--space-sm);
    max-inline-size: 460px;
  }
  .section-body > :global(h3),
  .section-body > :global(section > h3) {
    font-size: 15px;
  }
</style>
