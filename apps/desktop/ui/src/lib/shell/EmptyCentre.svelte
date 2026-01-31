<script lang="ts">
  // The centre with nothing open: the brand's logo, large and faint, as a watermark; under it the version and a
  // small Check for updates button, the same check as Settings › Updates (SM-UP-2.1). The side rail's Home
  // opens Home again (amended January 2026; user decision; Approved by the user January 2026).
  import { onMount } from 'svelte';

  import { answers } from '../backend/groups';
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import { AboutModel, updateStoreFor } from '../views/Settings/updates.svelte';
  import { getApp } from './context';

  const app = getApp();
  const local = app.local;
  const model = new AboutModel(local.handle, () => app.transport.appVersion(), updateStoreFor(local.handle));
  const status = $derived(model.status);
  const canCheck = $derived(answers(local, 'setup.check_update'));

  /** The logo of each theme as a data URL; `null` while it is not there or the brand has none readable. */
  let logos = $state<{ light: string | null; dark: string | null }>({ light: null, dark: null });

  async function logo(kind: 'logo_light' | 'logo_dark'): Promise<string | null> {
    try {
      const out = await local.handle.call('brand.get_asset', { asset: { kind } });
      if (out.kind !== 'ok') return null;
      const { media_type, base64 } = out.result;
      return media_type === 'image/svg+xml' || media_type === 'image/png' ? `data:${media_type};base64,${base64}` : null;
    } catch {
      return null;
    }
  }

  onMount(() => {
    // The version only: the offer is the shell's to ask for (SM-UP-3, one `setup.update_offer` per start).
    void app.transport
      .appVersion()
      .then((v) => (model.version = v))
      .catch(() => undefined);
    void Promise.all([logo('logo_light'), logo('logo_dark')]).then(([light, dark]) => (logos = { light, dark: dark ?? light }));
    return () => model.close();
  });
</script>

<div class="empty-centre">
  <div class="watermark" aria-hidden="true">
    {#if logos.light !== null}<img class="logo-light" src={logos.light} alt="" />{/if}
    {#if logos.dark !== null}<img class="logo-dark" src={logos.dark} alt="" />{/if}
  </div>
  {#if model.version !== ''}<p class="version" tabindex="-1" data-region-focus>{t('about-version', { version: model.version })}</p>{/if}
  {#if canCheck}
    <button type="button" class="check" disabled={model.busy} onclick={() => void model.check()}><Icon name="history" size="small" />{t('about-check')}</button>
    <p class="status" role="status" aria-live="polite">
      {#if status.kind === 'checking'}{t('about-checking')}
      {:else if status.kind === 'up_to_date'}{t('about-up-to-date')}
      {:else if status.kind === 'downloading'}{t('about-downloading', { version: status.version })}
      {:else if status.kind === 'ready'}{t('about-ready', { version: status.version })}
      {:else if status.kind === 'administrator_needed'}{t('update-ask-administrator', { version: status.version })}
      {:else if status.kind === 'could_not_check'}{t('about-could-not-check')}{/if}
    </p>
  {/if}
</div>

<style>
  .empty-centre {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-sm);
    min-block-size: 100%;
    color: var(--color-text-muted);
    text-align: center;
  }
  .watermark img {
    inline-size: min(60%, 420px);
    min-inline-size: 160px;
    opacity: 0.08;
    user-select: none;
    pointer-events: none;
  }
  .watermark {
    display: flex;
    justify-content: center;
    inline-size: 100%;
  }
  .logo-dark {
    display: none;
  }
  :global(:root[data-theme='dark']) .logo-dark {
    display: block;
  }
  :global(:root[data-theme='dark']) .logo-light {
    display: none;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme='light'])) .logo-dark {
      display: block;
    }
    :global(:root:not([data-theme='light'])) .logo-light {
      display: none;
    }
  }
  .version,
  .status {
    margin: 0;
    font-size: 13px;
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: var(--space-06);
    min-height: 0;
    padding: var(--space-04) var(--space-10);
    font-size: 13px;
  }
</style>
