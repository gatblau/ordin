// Realistic ids for test data. The backend refuses an id that is not a lowercase hyphenated UUID
// (version 7 for every 2C identifier), so a test that sends one must use that form too
// (`make ui-contract-check`). `uid('p1')` keeps the test readable and always gives the same id for the
// same label.

/** 32-bit FNV-1a of `text`, from `seed`. */
function fnv(text: string, seed: number): number {
  let h = seed >>> 0;
  for (let i = 0; i < text.length; i += 1) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h;
}

/** A UUID v7 (version nibble 7, variant 8) whose last 12 hex digits are derived from `label`. */
export function uid(label: string): string {
  const a = fnv(label, 0x811c9dc5).toString(16).padStart(8, '0');
  const b = (fnv(label, 0x01000193) & 0xffff).toString(16).padStart(4, '0');
  return `0192f000-0000-7000-8000-${a}${b}`;
}

/** A memory "thing" id: `entity:` then a UUID. */
export const thingId = (label: string): string => `entity:${uid(label)}`;

/** A memory "link" id: `relationship:` then a UUID. */
export const linkId = (label: string): string => `relationship:${uid(label)}`;
