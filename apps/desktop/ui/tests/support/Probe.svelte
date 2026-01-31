<script lang="ts">
  import type { ScreenProps } from '../../src/lib/shell/screens';
  let { workspaceId, instance }: ScreenProps = $props();
</script>

<p class="probe">probe {workspaceId} {instance}</p>
