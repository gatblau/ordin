<script lang="ts">
  // Shown on `ordin://quit-requested`. Quit is not the default button; it sends `app.quit`.
  import type { BackendHandle } from '../backend/backend';
  import { t } from '../i18n/fluent';
  import ConfirmDialog from './ConfirmDialog.svelte';

  interface Props {
    count: number;
    backend: BackendHandle;
    oncancel: () => void;
  }
  let { count, backend, oncancel }: Props = $props();

  async function quit(): Promise<void> {
    await backend.call('app.quit', {});
  }
</script>

<ConfirmDialog
  title={t('quit-title')}
  body={t('quit-body', { count })}
  verb={t('quit-verb')}
  onconfirm={() => void quit()}
  {oncancel}
/>
