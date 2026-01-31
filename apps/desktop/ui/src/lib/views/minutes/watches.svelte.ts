// The minutes state of each workspace whose **Minutes** row is drawn in the tree (ScreensModule.md SM-MIN-19,
// SM-MIN-21): the row keeps one `MinutesScreenModel` here while it is drawn, so *Needs you* can list the
// questions its jobs wait on. Keyed by backend and workspace.
//
// spec: ordin-desktop/components/ScreensModule#SM-MIN-21
import { SvelteMap } from 'svelte/reactivity';

import type { MinutesScreenModel } from './minutes-screen.svelte';

export interface MinutesWatch {
  workspaceId: string;
  model: MinutesScreenModel;
}

const watches = new SvelteMap<string, MinutesWatch>();

export function watchMinutes(backendId: string, workspaceId: string, model: MinutesScreenModel): () => void {
  const key = `${backendId}/${workspaceId}`;
  watches.set(key, { workspaceId, model });
  return () => {
    if (watches.get(key)?.model === model) watches.delete(key);
  };
}

export function minutesWatches(): MinutesWatch[] {
  return [...watches.values()];
}
