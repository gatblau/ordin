import { flushSync } from 'svelte';

/** Runs `read` inside a `$derived`, as a template does, and returns what it gave. Svelte throws `state_unsafe_mutation` if `read` writes reactive state. */
export function readReactively<T>(read: () => T): T {
  let result!: T;
  const stop = $effect.root(() => {
    const value = $derived(read());
    $effect(() => {
      result = value;
    });
  });
  flushSync();
  stop();
  return result;
}
