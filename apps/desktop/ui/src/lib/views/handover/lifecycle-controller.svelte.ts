// The lifecycle screen's state (SM-NS-9): the stage the workspace follows (`null` when it follows none) and
// the lifecycle that stage belongs to. The draft of a lifecycle and Show as diagram are not drawn (G-CU-4).
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-9
import type { LifecycleView, ServiceError, StageView } from '../../service-api/types.gen';
import type { Handle } from '../fixed/util';

export class LifecycleController {
  /** `undefined` until read; `null` when the workspace follows no stage. */
  stage = $state.raw<StageView | null | undefined>(undefined);
  view = $state.raw<LifecycleView | null>(null);
  error = $state.raw<ServiceError | null>(null);
  readonly #handle: Handle;
  readonly #workspaceId: string;

  constructor(handle: Handle, workspaceId: string) {
    this.#handle = handle;
    this.#workspaceId = workspaceId;
  }

  async load(): Promise<void> {
    const stage = await this.#handle.call('handover.stage_of', { workspace: this.#workspaceId });
    if (stage.kind === 'error') {
      this.error = stage.error;
      return;
    }
    this.stage = stage.result;
    this.error = null;
    if (stage.result === null) {
      this.view = null;
      return;
    }
    const view = await this.#handle.call('handover.lifecycle_view', { id: stage.result.lifecycle.id });
    if (view.kind === 'ok') this.view = view.result;
    else this.error = view.error;
  }
}
