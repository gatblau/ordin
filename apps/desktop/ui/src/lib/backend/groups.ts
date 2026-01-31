// SM-UP-1: the one place that decides whether a backend answers a command.
//
// spec: ordin-desktop/components/ScreensModule#SM-UP-1

/** Anything that carries the `groups` of a backend's `service.hello`; `BackendState` does. */
export interface HasGroups {
  readonly groups: ReadonlySet<string>;
}

/** The group of a command: the part before the first `.` (`CommandMeta.module`); `null` when there is none. */
export function groupOf(command: string): string | null {
  const dot = command.indexOf('.');
  return dot > 0 ? command.slice(0, dot) : null;
}

/** `service` is never gated: the hello itself is how the set is learned. */
export function answers(backend: HasGroups, command: string): boolean {
  const group = groupOf(command);
  if (group === null) return false;
  return group === 'service' || backend.groups.has(group);
}

/** An entry that sends several commands is drawn only when the backend answers every one. */
export function answersAll(backend: HasGroups, commands: readonly string[]): boolean {
  return commands.every((c) => answers(backend, c));
}
