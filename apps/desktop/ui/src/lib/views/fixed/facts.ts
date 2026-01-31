// Pure helpers for drawing a fact: the kind in words, the agent's name, the citation line (SM-SF-6).
//
// spec: ordin-desktop/components/ScreensModule#SM-SF-6
import { formatDate, formatMoney } from '../../format/intl';
import { i18n, t } from '../../i18n/fluent';
import type {
  FactView,
  InstalledAgent,
  MemoryKind,
  ObjectInput,
  ObjectKind,
  ObjectView,
  TermOrigin,
  TermView,
  ThingView,
  TripleView,
} from '../../service-api/types.gen';

export const KIND_MESSAGE: Readonly<Record<MemoryKind, string>> = {
  happened: 'memory-kind-happened',
  known: 'memory-kind-known',
  summary: 'memory-kind-summary',
};

/** The kind label of a held write by its `fact_class`; no label for any other value (SM-SF-4.2). */
export function heldKindMessage(factClass: string | null | undefined): string | null {
  switch (factClass) {
    case 'episodic':
      return KIND_MESSAGE.happened;
    case 'semantic':
      return KIND_MESSAGE.known;
    case 'consolidated':
      return KIND_MESSAGE.summary;
    default:
      return null;
  }
}

/** Agent names by agent instance id, from an `agents.list` result (SA-UI-SF-05). */
export class AgentNames {
  readonly #by: Map<string, string>;
  constructor(agents: readonly InstalledAgent[] = []) {
    this.#by = new Map(agents.map((a) => [a.instance.id, a.instance.display_name]));
  }
  nameOf(id: string | null | undefined): string | null {
    return id === null || id === undefined ? null : (this.#by.get(id) ?? null);
  }
}

export interface MessageRef {
  id: string;
  args: Record<string, string>;
}

/**
 * The source line of an agent's citation (SM-SF-6): the agent's name, or "an agent" when none matches;
 * never a plain document source. `null` for any other origin. `origin_ref` is text, not a link.
 */
export function citationLine(fact: Pick<FactView, 'came_from'>, names: AgentNames): MessageRef | null {
  const from = fact.came_from;
  if (from.kind !== 'agent_citing') return null;
  const agent = names.nameOf(from.agent_instance_id);
  const args = { source: from.origin_ref, version: from.version };
  return agent === null ? { id: 'memory-cited-by-an-agent', args } : { id: 'memory-cited-by-agent', args: { ...args, agent } };
}

/** `true_from` / `true_until` as the line the fact shows (`memory-true-since` or `memory-true-between`). */
export function truthLine(from: string, until: string | null | undefined): MessageRef {
  return until === null || until === undefined
    ? { id: 'memory-true-since', args: { date: formatDate(from) } }
    : { id: 'memory-true-between', args: { from: formatDate(from), until: formatDate(until) } };
}

/** The longest statement a person may write when correcting a fact (MemoryService §9.4). */
export const FACT_STATEMENT_MAX = 500;

/** The scope line of a fact: a fact with a workspace is that workspace's; without one it is everywhere's. */
export function scopeMessage(fact: Pick<FactView, 'workspace_id'>): string {
  return fact.workspace_id ? 'memory-scope-workspace' : 'memory-scope-everywhere';
}

/**
 * Where a fact came from, in words (SM-SF-11). An agent's citation keeps its SM-SF-6 line. A knowledge
 * page, a team folder and the person's own writing read as such; a document reads as its `origin_ref`
 * text, never as a link.
 */
export function sourceLine(fact: Pick<FactView, 'came_from'>, names: AgentNames): MessageRef | null {
  const from = fact.came_from;
  switch (from.kind) {
    case 'agent_citing':
      return citationLine(fact, names);
    case 'conversation':
      return { id: 'memory-from-conversation', args: {} };
    case 'agent': {
      const agent = names.nameOf(from.agent_instance_id);
      return agent === null ? { id: 'memory-from-an-agent', args: {} } : { id: 'memory-from-agent', args: { agent } };
    }
    case 'document':
      return { id: 'memory-from-document', args: { source: from.origin_ref } };
    case 'knowledge_page':
      return { id: 'memory-from-page', args: {} };
    case 'team':
      return { id: 'memory-from-team', args: {} };
    case 'person':
      return { id: 'memory-from-you', args: {} };
  }
}

const sameName = (a: string, b: string): boolean => a.trim().toLocaleLowerCase() === b.trim().toLocaleLowerCase();

/**
 * A thing no live fact names is never a row, whatever its link count (SM-SF-16, replacing SM-SF-12's first
 * sentence). The backend already lists only such things; this keeps the rule should a row slip through.
 *
 * spec: ordin-desktop/components/ScreensModule#SM-SF-16
 */
export function worthListing(thing: Pick<ThingView, 'fact_count'>): boolean {
  return thing.fact_count > 0;
}

/**
 * The name a thing is drawn under (SM-SF-12): its stored name is lower case, so the same name as written
 * (an other name that differs only in case) is drawn when there is one.
 */
export function thingTitle(thing: Pick<ThingView, 'name' | 'other_names'>): string {
  return thing.other_names.find((n) => sameName(n, thing.name)) ?? thing.name;
}

/** The other names worth showing: those that are not the title again in other letters (SM-SF-12). */
export function otherNamesOf(thing: Pick<ThingView, 'name' | 'other_names'>): string[] {
  return thing.other_names.filter((n) => !sameName(n, thing.name));
}

// ---- facts as a thing, a word and a value (SM-SF-13 to SM-SF-17) ---------------------------------------

/** A page section is a passage: never drawn in a fact list (SM-SF-18). */
// spec: ordin-desktop/components/ScreensModule#SM-SF-18
export function notPassage(fact: Pick<FactView, 'passage'>): boolean {
  return fact.passage !== true;
}

const YEAR = /^\d{4}$/;
const MONTH = /^\d{4}-(0[1-9]|1[0-2])$/;
const DAY = /^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])$/;

/** `YYYY`, `YYYY-MM` or `YYYY-MM-DD`, a real day (SM-SF-15). */
export function isFactDate(value: string): boolean {
  if (YEAR.test(value) || MONTH.test(value)) return true;
  if (!DAY.test(value)) return false;
  const d = new Date(`${value}T00:00:00Z`);
  return !Number.isNaN(d.getTime()) && d.toISOString().slice(0, 10) === value;
}

/** A date by its precision: the year, the month and year, or the day by `formatDate` (SM-SF-13). */
export function dateText(value: string): string {
  if (YEAR.test(value)) return formatDate(`${value}-01-01T00:00:00Z`, { year: 'numeric', timeZone: 'UTC' });
  if (MONTH.test(value)) return formatDate(`${value}-01T00:00:00Z`, { year: 'numeric', month: 'long', timeZone: 'UTC' });
  if (DAY.test(value)) return formatDate(`${value}T00:00:00Z`, { dateStyle: 'medium', timeZone: 'UTC' });
  return value;
}

/** Money in the active locale with the fact's currency; the amount and code as written when Intl refuses them. */
export function moneyText(amount: string, currency: string): string {
  const value = Number(amount);
  if (!Number.isFinite(value)) return `${amount} ${currency}`;
  try {
    return formatMoney(value, currency);
  } catch {
    return `${amount} ${currency}`;
  }
}

/**
 * The object as the line writes it (SM-SF-13): a thing's name; a text as written; a number as `value` then
 * `unit`; money by `formatMoney`; a date by its precision; yes/no in words.
 *
 * spec: ordin-desktop/components/ScreensModule#SM-SF-13
 */
export function objectText(object: ObjectView): string {
  switch (object.kind) {
    case 'thing':
      return object.thing.name;
    case 'text':
      return object.value;
    case 'number':
      return object.unit ? `${object.value} ${object.unit}` : object.value;
    case 'money':
      return moneyText(object.amount, object.currency);
    case 'date':
      return dateText(object.value);
    case 'yes_no':
      return t(object.value ? 'memory-value-yes' : 'memory-value-no');
  }
}

/** The three parts of a fact, in reading order. */
export function tripleParts(triple: TripleView): [string, string, string] {
  return [triple.subject.name, triple.predicate.label, objectText(triple.object)];
}

/** The three parts as one name: joined by spaces, never `·` (SM-SF-13's accessible name). */
export function tripleName(triple: TripleView): string {
  return tripleParts(triple).join(' ');
}

/** The three parts as drawn: joined by `·`. A fact with no triple reads as its statement. */
export function tripleLine(fact: Pick<FactView, 'triple' | 'statement'>): string {
  return fact.triple ? tripleParts(fact.triple).join(' · ') : fact.statement;
}

// ---- Correct a value (SM-SF-15) ------------------------------------------------------------------------

/** What the correction fields hold: `a` the first box (name, text, value, amount, date), `b` the second (unit, currency). */
export interface ObjectDraft {
  kind: ObjectKind;
  a: string;
  b: string;
  yes: boolean;
}

export const THING_NAME_MAX = 200;
export const TEXT_VALUE_MAX = 1000;
export const UNIT_MAX = 32;
const DECIMAL = /^-?\d+(\.\d+)?$/;
const CURRENCY = /^[A-Z]{3}$/;

export function draftOf(object: ObjectView): ObjectDraft {
  switch (object.kind) {
    case 'thing':
      return { kind: 'thing', a: object.thing?.name ?? '', b: '', yes: false };
    case 'text':
      return { kind: 'text', a: object.value ?? '', b: '', yes: false };
    case 'number':
      return { kind: 'number', a: object.value ?? '', b: object.unit ?? '', yes: false };
    case 'money':
      return { kind: 'money', a: object.amount ?? '', b: object.currency ?? '', yes: false };
    case 'date':
      return { kind: 'date', a: object.value ?? '', b: '', yes: false };
    case 'yes_no':
      return { kind: 'yes_no', a: '', b: '', yes: object.value === true };
  }
}

/**
 * The object the draft sends, or `null` while it is empty or not of its form (SM-SF-15: a name 1–200, a text
 * 1–1,000, a decimal with a unit 0–32, an amount with a three-letter currency, a date of three forms).
 */
export function objectInputOf(draft: ObjectDraft): ObjectInput | null {
  // A box the person emptied may hold null rather than text; it is empty.
  const a = (draft.a ?? '').trim();
  const b = (draft.b ?? '').trim();
  switch (draft.kind) {
    case 'thing':
      return a.length >= 1 && a.length <= THING_NAME_MAX ? { thing: { name: a } } : null;
    case 'text':
      return a.length >= 1 && a.length <= TEXT_VALUE_MAX ? { text: a } : null;
    case 'number':
      return DECIMAL.test(a) && b.length <= UNIT_MAX ? { number: { value: a, unit: b === '' ? null : b } } : null;
    case 'money':
      return DECIMAL.test(a) && CURRENCY.test(b) ? { money: { amount: a, currency: b } } : null;
    case 'date':
      return isFactDate(a) ? { date: a } : null;
    case 'yes_no':
      return { yes_no: draft.yes };
  }
}

/** True when the draft says what the object already says (Save stays off, SM-SF-15). */
export function draftUnchanged(draft: ObjectDraft, object: ObjectView): boolean {
  const was = draftOf(object);
  return (draft.a ?? '').trim() === was.a && (draft.b ?? '').trim() === was.b && draft.yes === was.yes;
}

// ---- Words (SM-SF-17) ----------------------------------------------------------------------------------

/**
 * A message id is kebab-case (the catalogue build refuses any other), and a seed key is camelCase
 * (`worksFor`), so the id spells the key in kebab-case: `memory-term-works-for`.
 */
// SPEC-GAP-3: SM-SF-17 names the id `memory-term-<key>`, which the catalogue build refuses for a
// camelCase key (LOC_BUILD_CATALOGUE_INVALID); the key is written in kebab-case in the id.
const termId = (key: string): string => `memory-term-${key.replace(/([a-z0-9])([A-Z])/g, '$1-$2').toLowerCase()}`;

/** A seed word never renamed reads from the catalogue in the active locale when it has it, else as stored. */
// spec: ordin-desktop/components/ScreensModule#SM-SF-17
export function wordLabel(term: Pick<TermView, 'key' | 'label' | 'seed_unrenamed'>): string {
  const id = termId(term.key);
  return term.seed_unrenamed && i18n.has(id) ? t(id) : term.label;
}

export function wordMeaning(term: Pick<TermView, 'key' | 'meaning' | 'seed_unrenamed'>): string {
  const id = `${termId(term.key)}-meaning`;
  return term.seed_unrenamed && i18n.has(id) ? t(id) : term.meaning;
}

export const WORD_OBJECT_MESSAGE: Readonly<Record<ObjectKind, string>> = {
  thing: 'memory-word-object-thing',
  text: 'memory-word-object-text',
  number: 'memory-word-object-number',
  money: 'memory-word-object-money',
  date: 'memory-word-object-date',
  yes_no: 'memory-word-object-yes-no',
};

/** Who added a word, in words. */
export function addedByLine(origin: TermOrigin, names: AgentNames): MessageRef {
  switch (origin.kind) {
    case 'seed':
      return { id: 'memory-word-by-seed', args: {} };
    case 'ordin':
      return { id: 'memory-word-by-ordin', args: {} };
    case 'agent': {
      const agent = names.nameOf(origin.agent_instance_id);
      return agent === null ? { id: 'memory-word-by-an-agent', args: {} } : { id: 'memory-word-by-agent', args: { agent } };
    }
    case 'hosted_agent':
      return { id: 'memory-word-by-hosted-agent', args: {} };
    case 'team':
      return { id: 'memory-word-by-team', args: {} };
    case 'person':
      return { id: 'memory-word-by-you', args: {} };
  }
}

export const WORD_LABEL_MAX = 64;

// ---- Grouping, search and filters (SM-SF-27) -----------------------------------------------------------

/** What the Facts list groups by (SM-SF-27); remembered per workspace in the view preferences. */
export type GroupBy = 'thing' | 'kind' | 'source' | 'added_by' | 'topic';
export const GROUP_BY: readonly GroupBy[] = ['thing', 'kind', 'source', 'added_by', 'topic'];
export const GROUP_BY_MESSAGE: Readonly<Record<GroupBy, string>> = {
  thing: 'memory-group-by-thing',
  kind: 'memory-group-by-kind',
  source: 'memory-group-by-source',
  added_by: 'memory-group-by-added-by',
  topic: 'memory-group-by-topic',
};
export function isGroupBy(v: unknown): v is GroupBy {
  return typeof v === 'string' && (GROUP_BY as readonly string[]).includes(v);
}

/** The lines a group draws before **Show more**, and the groups drawn before **Show more groups**. */
export const GROUP_PAGE = 50;
export const GROUPS_PAGE = 100;

/** The filter chips (SM-SF-27): each `true` keeps only the facts it names. */
export interface FactFilters {
  conflicts: boolean;
  outOfDate: boolean;
  older: boolean;
  /** `all`: both; `workspace`: this workspace's facts only; `everywhere`: facts of every workspace only. */
  scope: 'all' | 'workspace' | 'everywhere';
  /** `null`: every topic; `''`: things with no topic; else that topic. */
  topic: string | null;
}
export const NO_FILTERS: FactFilters = { conflicts: false, outOfDate: false, older: false, scope: 'all', topic: null };

/** A thing's id to its topic (`memory.thing_topics`). */
export type TopicMap = Readonly<Record<string, string>>;

/** The thing a fact is about: its triple's subject, else its first thing; `null` for neither. */
export function subjectOf(fact: Pick<FactView, 'triple' | 'things'>): { id: string; name: string } | null {
  if (fact.triple) return fact.triple.subject;
  return fact.things?.[0] ?? null;
}

/** The key a thing's name is looked up by in `topicsByName`: trimmed, case aside. */
const nameKey = (name: string): string => name.trim().toLocaleLowerCase();

/**
 * A thing's topic by its name and each other name, for the things whose ids are known (SM-SF-27, amended
 * January 2026; user decision; Approved by the user January 2026). A fact whose subject the memory component
 * had not yet tied to a thing when the fact was read carries an empty subject id; it finds its topic by name.
 */
export function topicsByName(things: readonly Pick<ThingView, 'id' | 'name' | 'other_names'>[], topics: TopicMap): TopicMap {
  const out: Record<string, string> = {};
  for (const thing of things) {
    const topic = topics[thing.id];
    if (topic === undefined) continue;
    for (const name of [thing.name, ...thing.other_names]) {
      const key = nameKey(name);
      if (key !== '' && out[key] === undefined) out[key] = topic;
    }
  }
  return out;
}

/** The topic of the thing a fact is about, by its id, else by its name; `null` when it has none. */
export function topicOf(fact: Pick<FactView, 'triple' | 'things'>, topics: TopicMap, byName: TopicMap = {}): string | null {
  const subject = subjectOf(fact);
  if (subject === null) return null;
  if (subject.id !== '' && topics[subject.id] !== undefined) return topics[subject.id] ?? null;
  return byName[nameKey(subject.name)] ?? null;
}

/** The parts a search reads: the subject, the word and the value; a fact with no triple, its statement. */
export function searchParts(fact: Pick<FactView, 'triple' | 'statement'>): string[] {
  return fact.triple ? tripleParts(fact.triple) : [fact.statement];
}

/** SM-SF-27: one search over all three parts, case and accents aside. */
export function factMatches(fact: Pick<FactView, 'triple' | 'statement'>, query: string): boolean {
  const q = fold(query.trim());
  if (q === '') return true;
  return searchParts(fact).some((part) => fold(part).includes(q));
}

const fold = (s: string): string => s.normalize('NFD').replace(/\p{M}/gu, '').toLocaleLowerCase();

/** `text` cut into runs, `hit` where it matches `query` (case and accents aside), for `<mark>`. */
export function highlightParts(text: string, query: string): { text: string; hit: boolean }[] {
  const q = fold(query.trim());
  if (q === '') return [{ text, hit: false }];
  // Folding keeps one code unit per letter for the text a person types, so positions map back; a text
  // whose folded form differs in length is drawn unmarked.
  const folded = fold(text);
  if (folded.length !== text.length) return [{ text, hit: false }];
  const out: { text: string; hit: boolean }[] = [];
  let at = 0;
  for (let i = folded.indexOf(q); i !== -1; i = folded.indexOf(q, at)) {
    if (i > at) out.push({ text: text.slice(at, i), hit: false });
    out.push({ text: text.slice(i, i + q.length), hit: true });
    at = i + q.length;
  }
  if (at < text.length) out.push({ text: text.slice(at), hit: false });
  return out;
}

/** The facts the chips and the search keep, in the order given. */
export function filterFacts(facts: readonly FactView[], filters: FactFilters, query: string, topics: TopicMap, byName: TopicMap = {}): FactView[] {
  return facts.filter((f) => {
    if (filters.conflicts && (f.conflicts_with ?? []).length === 0) return false;
    if (filters.outOfDate && !f.may_be_out_of_date) return false;
    if (filters.older && !(f.triple == null && f.older_note === true)) return false;
    if (filters.scope === 'workspace' && !f.workspace_id) return false;
    if (filters.scope === 'everywhere' && f.workspace_id) return false;
    if (filters.topic !== null && (topicOf(f, topics, byName) ?? '') !== filters.topic) return false;
    return factMatches(f, query);
  });
}

/** One collapsible section of the Facts list: its key, its name, and its facts. */
export interface FactGroup {
  key: string;
  name: string;
  facts: FactView[];
}

/** Where a fact came from, as a group name (SM-SF-27): the source document, or the origin in words. */
function sourceName(fact: FactView, names: AgentNames): { key: string; name: string } {
  const from = fact.came_from;
  if (from.kind === 'document' || from.kind === 'agent_citing') return { key: `doc:${from.origin_ref}`, name: from.origin_ref };
  const line = sourceLine(fact, names);
  return line === null ? { key: 'other', name: '' } : { key: `${line.id}:${line.args['agent'] ?? ''}`, name: t(line.id, line.args) };
}

/** Who added a fact, as a group name (SM-SF-27): you, Ordin, your team, or the named agent. */
export function addedBy(fact: Pick<FactView, 'came_from'>, names: AgentNames): { key: string; name: string } {
  const from = fact.came_from;
  switch (from.kind) {
    case 'person':
      return { key: 'you', name: t('memory-added-by-you') };
    case 'team':
      return { key: 'team', name: t('memory-added-by-team') };
    case 'agent':
    case 'agent_citing': {
      const agent = names.nameOf(from.agent_instance_id);
      return agent === null ? { key: 'agent', name: t('memory-added-by-an-agent') } : { key: `agent:${from.agent_instance_id}`, name: agent };
    }
    default:
      return { key: 'ordin', name: t('memory-added-by-ordin') };
  }
}

/**
 * SM-SF-27: the facts in groups, each group in the order its first fact is met, except that the group of
 * facts about no thing (Thing) and the group of things with no topic (Topic) come last.
 */
export function groupFacts(facts: readonly FactView[], by: GroupBy, names: AgentNames, topics: TopicMap, byName: TopicMap = {}): FactGroup[] {
  const groups = new Map<string, FactGroup>();
  const put = (key: string, name: string, fact: FactView): void => {
    const g = groups.get(key) ?? { key, name, facts: [] };
    g.facts.push(fact);
    groups.set(key, g);
  };
  for (const fact of facts) {
    switch (by) {
      case 'thing': {
        const s = subjectOf(fact);
        if (s === null) put('none', t('memory-group-no-thing'), fact);
        else put(`t:${s.id || s.name.toLocaleLowerCase()}`, s.name, fact);
        break;
      }
      case 'kind':
        // "Kind of fact" is the fact's word (its predicate, as on the Kinds of fact tab), not its memory
        // class: nearly every fact is "Something that happened", so grouping by the class drew one group.
        // A fact written before facts were triples has no word and falls back to its class.
        if (fact.triple) put(`word:${fact.triple.predicate.key}`, fact.triple.predicate.label, fact);
        else put(`class:${fact.kind}`, t(KIND_MESSAGE[fact.kind]), fact);
        break;
      case 'source': {
        const s = sourceName(fact, names);
        put(s.key, s.name, fact);
        break;
      }
      case 'added_by': {
        const a = addedBy(fact, names);
        put(a.key, a.name, fact);
        break;
      }
      case 'topic': {
        const topic = topicOf(fact, topics, byName);
        if (topic === null) put('none', t('memory-group-no-topic'), fact);
        else put(`topic:${topic.toLocaleLowerCase()}`, topic, fact);
        break;
      }
    }
  }
  const all = [...groups.values()];
  return [...all.filter((g) => g.key !== 'none'), ...all.filter((g) => g.key === 'none')];
}

/** A thing's topic: 1–40 characters (MemoryService.md MEM-TOP-2). */
export const TOPIC_MAX = 40;
