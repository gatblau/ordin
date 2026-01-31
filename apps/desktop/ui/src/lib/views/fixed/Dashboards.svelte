<script lang="ts">
  import { t } from '../../i18n/fluent';
  import type { ScreenProps } from '../../shell/screens';
  import { loadDashboards } from './lists.svelte';
  import LoadedList from './LoadedList.svelte';

  let { backend, workspaceId }: ScreenProps = $props();
</script>

<LoadedList {backend} {workspaceId} heading={t('tree-dashboards')} load={() => loadDashboards(backend.handle, workspaceId ?? '')} icon="chart" tint="sheet" />
