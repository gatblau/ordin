// Plan 04, safety net B: a fake backend that answers every command with a sample result built from the
// command's own result schema (schema.gen.json, generated from the Rust types). A list has one item, every
// id is a valid UUID and every date a valid timestamp, so each screen has a row to click and the requests
// it sends back carry well-formed ids. A test still replaces any reply with `on()`.
import schema from '../../src/lib/service-api/schema.gen.json';
import type { CommandName } from '../../src/lib/backend/backend';
import { FakeTransport, ok, R1_GROUPS } from './fake-backend';

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
interface Schema {
  $ref?: string;
  type?: string | string[];
  format?: string;
  enum?: Json[];
  const?: Json;
  oneOf?: Schema[];
  anyOf?: Schema[];
  allOf?: Schema[];
  properties?: Record<string, Schema>;
  required?: string[];
  items?: Schema | false;
  prefixItems?: Schema[];
  minItems?: number;
  minimum?: number;
  minLength?: number;
  additionalProperties?: Schema | boolean;
}

const defs = (schema as unknown as { $defs: Record<string, Schema> }).$defs;
const commands = (schema as unknown as { 'x-ordin-commands': { name: string; result: Schema }[] })['x-ordin-commands'];

/** A deterministic UUID v7 for the n-th id handed out (version 7, variant 8). */
export function sampleId(n: number): string {
  const tail = n.toString(16).padStart(12, '0');
  return `0192f000-0000-7000-8000-${tail}`;
}

export const SAMPLE_TIME = '2026-01-10T10:43:58.120Z';

/** Words for the string fields a person reads, so a row has a title to click. */
const SAMPLE_TEXT: Record<string, string> = {
  name: 'Quarterly plan',
  display_name: 'Quarterly plan',
  title: 'Quarterly plan',
  path: 'Q3/Quarterly plan.pdf',
  relative_path: 'Q3/Quarterly plan.pdf',
  file_name: 'Quarterly plan.pdf',
  media_type: 'application/pdf',
  kind: 'file',
  locale: 'en-GB',
  url: 'https://example.invalid/item',
  email: 'sam.lee@example.invalid',
  site: 'example.atlassian.net',
};

export class SampleData {
  #next = 1;

  id(): string {
    return sampleId(this.#next++);
  }

  /** A sample value of `s`; `key` is the property name it is for, when there is one. */
  value(s: Schema | undefined, key = '', depth = 0): Json {
    if (s === undefined || depth > 12) return null;
    // One page only: a screen that reads every page would otherwise never stop asking for the next.
    if (key === 'next_cursor') return null;
    if (s.$ref !== undefined) {
      const name = s.$ref.replace('#/$defs/', '');
      const def = defs[name];
      // Ids whose type says their form: the backend checks it, so the requests built from them must carry it.
      if (name === 'CardRef') return 'Mm5Ve8Ux2Tw4Sy6Rz9Q1Kd';
      if (name === 'MemoryThingId') return `entity:${this.id()}`;
      if (name === 'MemoryLinkId') return `relationship:${this.id()}`;
      if (def?.type === 'string' && def.enum === undefined && /Id$/.test(name)) return this.id();
      return this.value(def, key, depth + 1);
    }
    if (s.const !== undefined) return s.const;
    if (s.enum !== undefined) return s.enum[0] ?? null;
    if (s.allOf !== undefined) return s.allOf.reduce<Json>((acc, part) => merge(acc, this.value(part, key, depth + 1)), {});
    const choice = s.oneOf ?? s.anyOf;
    if (choice !== undefined) {
      // The first choice that is not `null`: a nullable field gets a value, so its part of the screen is drawn.
      const first = choice.find((c) => c.type !== 'null') ?? choice[0];
      return this.value(first, key, depth + 1);
    }
    const type = Array.isArray(s.type) ? (s.type.find((t) => t !== 'null') ?? 'null') : s.type;
    switch (type) {
      case 'object': {
        const out: Record<string, Json> = {};
        for (const [name, prop] of Object.entries(s.properties ?? {})) out[name] = this.value(prop, name, depth + 1);
        return out;
      }
      case 'array': {
        if (s.prefixItems !== undefined) return s.prefixItems.map((p) => this.value(p, key, depth + 1));
        if (s.items === false || s.items === undefined) return [];
        return [this.value(s.items, key, depth + 1)];
      }
      case 'string':
        return this.text(s, key);
      case 'integer':
      case 'number':
        return Math.max(s.minimum ?? 1, 1);
      case 'boolean':
        return false;
      case 'null':
        return null;
      default:
        return null;
    }
  }

  text(s: Schema, key: string): string {
    if (s.format === 'uuid' || key === 'id' || key.endsWith('_id')) return this.id();
    if (s.format === 'date-time' || key.endsWith('_at')) return SAMPLE_TIME;
    if (s.format === 'uri') return 'https://example.invalid/item';
    const word = SAMPLE_TEXT[key];
    if (word !== undefined) return word;
    return 'Sample';
  }

  /** The sample result of `command`. */
  result(command: string): Json {
    const row = commands.find((c) => c.name === command);
    if (row === undefined) throw new Error(`no command ${command} in schema.gen.json`);
    return this.value(row.result);
  }
}

function merge(a: Json, b: Json): Json {
  if (a !== null && b !== null && typeof a === 'object' && typeof b === 'object' && !Array.isArray(a) && !Array.isArray(b)) return { ...a, ...b };
  return b;
}

/** The replies the window needs fixed to draw at all, kept from `FakeTransport`; `servers.list` stays empty
 * so no second backend is connected. */
const FIXED = new Set<string>([
  'service.hello',
  'localisation.get_settings',
  'localisation.get_catalogue',
  'brand.get',
  'settings.get',
  'servers.list',
  'app.memory_models',
  'setup.status',
]);

/** A `FakeTransport` whose every other command answers with a sample result. */
export class SampleTransport extends FakeTransport {
  readonly data = new SampleData();
  /** The workspace the screens are opened in. */
  readonly workspaceId = sampleId(0);

  constructor(options: { localGroups?: string[] } = {}) {
    super({ localGroups: options.localGroups ?? R1_GROUPS });
    for (const { name } of commands) {
      if (FIXED.has(name)) continue;
      this.on(name as CommandName, () => ok(this.data.result(name)));
    }
  }
}
