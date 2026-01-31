// The only file in this module that imports @tauri-apps/api (SA-UI-01). Everything else reaches the
// Command Bridge through the `Transport` interface of backend.ts.
//
// spec: ordin-desktop/components/ScreensModule#SA-UI-01
import { getVersion } from '@tauri-apps/api/app';
import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import type {
  BackendId,
  CommandEnvelope,
  CommandReply,
  DesktopEventName,
  EventBatch,
  EventsOpened,
  StreamMessage,
  Transport,
} from './backend';

export function createTauriTransport(): Transport {
  return {
    invoke(ipcName: string, envelope: CommandEnvelope<unknown>): Promise<CommandReply<unknown>> {
      return invoke<CommandReply<unknown>>(`plugin:ordin|${ipcName}`, { envelope });
    },
    openEvents(after: Record<BackendId, number>, onBatch: (batch: EventBatch) => void): Promise<EventsOpened> {
      const onEvents = new Channel<EventBatch>();
      onEvents.onmessage = onBatch;
      return invoke<EventsOpened>('plugin:ordin|service_events', { after, onEvents });
    },
    async openStreams(onMessage: (message: StreamMessage) => void): Promise<void> {
      const onStream = new Channel<StreamMessage>();
      onStream.onmessage = onMessage;
      await invoke<void>('plugin:ordin|service_streams', { onStream });
    },
    listenDesktop(name: DesktopEventName, onEvent: (payload: unknown) => void): Promise<() => void> {
      return listen<unknown>(`ordin://${name}`, (e) => onEvent(e.payload));
    },
    appVersion(): Promise<string> {
      return getVersion();
    },
  };
}
