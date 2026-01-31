// Settings › Language and the Language screen (ADR-109: SM-LG-2 to SM-LG-7).
import { beforeAll, describe, expect, it } from 'vitest';

import { createHandle } from '../src/lib/backend/backend';
import type { FakeTransport } from './support/fake-backend';
import { err, hello, ok, R1_GROUPS, SERVER } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { loaded, text } from './settings-support';
import Language from '../src/lib/views/Settings/Language.svelte';
import LanguageScreen from '../src/lib/views/Settings/LanguageScreen.svelte';
import MakeLanguageDialog from '../src/lib/views/Settings/MakeLanguageDialog.svelte';
import {
  isYours,
  LanguageScreenModel,
  LanguageSettingsModel,
  MakeLanguageModel,
  MAX_LANGUAGE_FILE_BYTES,
  stateLine,
} from '../src/lib/views/Settings/language-model.svelte';

beforeAll(loadWindowCatalogue);

const clean = (markup: string) => markup.replace(/<!--.*?-->/g, '');
const summary = (over: Record<string, unknown> = {}) => ({
  locale: 'fr-FR', native_name: 'Français', direction: 'ltr', kind: 'shipped', state: 'in_use', choosable: true, offered_by_company: false,
  edited: 0, in_english: 0, needs_review: 0, needs_changing: 0, ...over,
});
const row = (over: Record<string, unknown> = {}) => ({
  message_id: 'card-open', area: 'cards', english: 'Open', text: 'Ouvrir', shipped: 'Ouvrir', source: 'shipped', safety: false, reviewed: false, needs_changing: false, ...over,
});
const offered = (locale: string, native_name: string, direction = 'ltr') => ({ locale, native_name, direction });

function settingsReplies(languages: unknown[], offeredList: unknown[], active = 'en-GB') {
  return (t: FakeTransport) => {
    t.on('localisation.get_settings', ok({ active: { locale: active, source: 'person', direction: 'ltr', fallback_chain: [active] }, following_operating_system: false, offered: offeredList }));
    t.on('localisation.list_languages', ok({ areas: ['cards', 'rail'], languages }));
  };
}

describe('Settings › Language (SM-LG-2)', () => {
  it('The picker lists a confirmed made language and not an unconfirmed one', async () => {
    const t = await loaded(R1_GROUPS, settingsReplies(
      [summary({ locale: 'ja', native_name: '日本語', kind: 'made', state: 'in_use' }), summary({ locale: 'pt-BR', native_name: 'Português', kind: 'imported', state: 'needs_review', choosable: false, needs_review: 14 })],
      [offered('en-GB', 'English (UK)'), offered('ja', '日本語'), offered('pt-BR', 'Português')],
    ));
    const model = new LanguageSettingsModel(t.handle);
    await model.load();
    const out = clean(html(Language, { backend: t.state, model }));
    // A radio group with the language in use checked (amended January 2026 (user decision)).
    const picker = out.slice(out.indexOf('class="choice-group picker'), out.indexOf('</fieldset>'));
    expect(text(picker)).toContain('日本語 — ja');
    expect(text(picker)).toContain('Made by you');
    expect(picker).not.toContain('pt-BR');
    expect(text(out)).toContain('Same as this computer');
    // "Your languages" shows pt-BR with its state line.
    expect(text(out.slice(out.indexOf('class="yours')))).toContain('Português — pt-BR');
    expect(text(out)).toContain('Needs checking: 14 approval messages');
  });

  it('choosing sends localisation.set_locale; Follow this computer sends null', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      settingsReplies([], [offered('fr-FR', 'Français')])(tr);
      tr.on('localisation.set_locale', ok({ locale: 'fr-FR', source: 'person', direction: 'ltr', fallback_chain: ['fr-FR', 'en-GB'] }));
    });
    const model = new LanguageSettingsModel(t.handle);
    await model.load();
    await model.choose('fr-FR');
    await model.choose(null);
    expect(t.transport.sentOf('localisation.set_locale').map((e) => e.input)).toEqual([{ locale: 'fr-FR' }, { locale: null }]);
  });

  it('the state line is the first match, table-driven', () => {
    const cases: Array<[Record<string, unknown>, string, string | null]> = [
      [{ draft: { done: 3, total: 10 }, needs_review: 2 }, 'language-state-drafting', 'stop'],
      [{ needs_review: 14 }, 'language-state-needs-checking', null],
      [{ state: 'needs_review' }, 'language-state-ready-to-use', null],
      [{ needs_changing: 2, in_english: 5 }, 'language-state-needs-changing', null],
      [{ in_english: 5 }, 'language-state-in-english', 'ask_ordin'],
      [{}, 'language-state-in-use', null],
    ];
    for (const [over, id, action] of cases) {
      const line = stateLine(summary(over) as never);
      expect([line.messageId, line.action], JSON.stringify(over)).toEqual([id, action]);
    }
  });

  it('Your languages: made and imported, and a shipped one only when edited', () => {
    expect(isYours(summary({ kind: 'made' }) as never)).toBe(true);
    expect(isYours(summary({ kind: 'imported' }) as never)).toBe(true);
    expect(isYours(summary({ edited: 2 }) as never)).toBe(true);
    expect(isYours(summary() as never)).toBe(false);
  });

  it('Stop drafting sends ordin.stop_language_draft', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      settingsReplies([summary({ locale: 'ja', kind: 'made', draft: { done: 3, total: 10 } })], [])(tr);
      tr.on('ordin.stop_language_draft', ok({}));
    });
    const model = new LanguageSettingsModel(t.handle);
    await model.load();
    expect(text(html(Language, { backend: t.state, model }))).toContain('Drafting 3 of 10');
    await model.stopDrafting('ja');
    expect(t.transport.sentOf('ordin.stop_language_draft')[0]?.input).toEqual({ locale: 'ja' });
  });
});

describe('Make a new language… (SM-LG-4)', () => {
  it('The estimate is shown before drafting, and Cancel drafts nothing', async () => {
    const t = await loaded(R1_GROUPS, (tr) =>
      tr.on('ordin.estimate_language_draft', ok({ messages: 1200, estimate: { ask_first: false, band: 'medium', based_on: [], max_turns: 15, min_turns: 12, typical_day_permille: 100 } })),
    );
    const make = new MakeLanguageModel(t.handle);
    make.name = 'Japanese';
    make.tag = 'ja';
    expect(await make.next()).toBe(true);
    expect(t.transport.sentOf('ordin.estimate_language_draft')[0]?.input).toEqual({ locale: 'ja' });
    const out = text(html(MakeLanguageDialog, { make, backend: t.handle, onclose: () => undefined }));
    expect(out).toContain('Estimated use: medium, about 12 to 15 model turns, for 1200 messages.');
    expect(out).toContain('Before you use it, you check the wording on approval cards.');
    // Draft it is not the default button; Cancel is.
    const markup = html(MakeLanguageDialog, { make, backend: t.handle, onclose: () => undefined });
    expect(markup).toMatch(/<button[^>]*data-default="true"[^>]*>Cancel/);
    expect(markup).not.toMatch(/data-default="true"[^>]*>Draft it/);
    expect(t.transport.sentOf('ordin.draft_language')).toEqual([]);
  });

  it('without an estimate it says how many messages there are to draft', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('ordin.estimate_language_draft', ok({ messages: 40 })));
    const make = new MakeLanguageModel(t.handle);
    make.name = 'Welsh';
    make.tag = 'cy';
    await make.next();
    expect(text(html(MakeLanguageDialog, { make, backend: t.handle, onclose: () => undefined }))).toContain('40 messages to draft.');
  });

  it('Draft it sends ordin.draft_language with the tag and the name; nothing is sent before it', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('ordin.estimate_language_draft', ok({ messages: 3 }));
      tr.on('ordin.draft_language', ok({ accepted: true, messages: 3 }));
    });
    const make = new MakeLanguageModel(t.handle);
    make.name = ' Japanese ';
    make.tag = 'ja';
    expect(await make.draft()).toBe(false);
    await make.next();
    expect(await make.draft()).toBe(true);
    expect(t.transport.sentOf('ordin.draft_language')[0]?.input).toEqual({ locale: 'ja', name: 'Japanese' });
  });

  it('table: field lengths (name 1–60, code 2–35) gate Next', () => {
    const t = { call: async () => ({ kind: 'ok', result: {} }) } as never;
    for (const [name, tag, valid] of [['J', 'ja', true], ['', 'ja', false], ['J', 'j', false], ['x'.repeat(61), 'ja', false], ['J', 'x'.repeat(36), false], ['J', 'x'.repeat(35), true]] as const) {
      const m = new MakeLanguageModel(t);
      m.name = name;
      m.tag = tag;
      expect(m.validFields, `${name.length}/${tag.length}`).toBe(valid);
    }
  });

  it('errors are drawn in the dialog with focus on the field they concern', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('ordin.estimate_language_draft', err('LOC_TAG_INVALID', 'That language code is not valid.')));
    const make = new MakeLanguageModel(t.handle);
    make.name = 'X';
    make.tag = 'zz';
    await make.next();
    expect(make.errorField).toBe('tag');
    expect(text(html(MakeLanguageDialog, { make, backend: t.handle, onclose: () => undefined }))).toContain('That language code is not valid.');
  });

  it('Ask Ordin to draft them opens the dialog with the tag and name filled', async () => {
    const t = await loaded(R1_GROUPS, settingsReplies([summary({ locale: 'ja', native_name: '日本語', kind: 'made', in_english: 20 })], []));
    const model = new LanguageSettingsModel(t.handle);
    await model.load();
    model.openMake({ locale: 'ja', name: '日本語' });
    expect(model.make?.tag).toBe('ja');
    expect(model.make?.name).toBe('日本語');
  });
});

describe('Import… and Export… (SM-LG-5, SM-LG-6)', () => {
  const report = (over: Record<string, unknown> = {}) => ({
    locale: 'pt-BR', stored: 1180, dropped_unknown: 3, in_english: 20, left_out: 0, left_out_ids: [],
    summary: summary({ locale: 'pt-BR', native_name: 'Português', kind: 'imported', state: 'needs_review', needs_review: 14 }), ...over,
  });
  const file = (size: number, name = 'ordin-language-pt-BR.ftl') => ({ name, size, text: async () => 'card-open = Abrir' });

  it('An import reports what it did and opens on Needs checking', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      settingsReplies([], [])(tr);
      tr.on('localisation.import_language', ok(report()));
    });
    const model = new LanguageSettingsModel(t.handle);
    const result = await model.importFile(file(2000));
    expect(t.transport.sentOf('localisation.import_language')[0]?.input).toEqual({ file_name: 'ordin-language-pt-BR.ftl', ftl: 'card-open = Abrir', replace: false });
    expect(result?.summary.needs_review).toBe(14);
    expect(text(html(Language, { backend: t.state, model }))).toContain('Imported 1180 messages; 3 not known here were left out; 20 show in English.');
  });

  it('left-out messages are listed behind Show which', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      settingsReplies([], [])(tr);
      tr.on('localisation.import_language', ok(report({ left_out: 2, left_out_ids: ['a-b', 'c-d'] })));
    });
    const model = new LanguageSettingsModel(t.handle);
    await model.importFile(file(10));
    model.showLeftOut = true;
    const out = text(html(Language, { backend: t.state, model }));
    expect(out).toContain("2 messages didn't pass the checks and were left out.");
    expect(out).toContain('Show which');
    expect(out).toContain('a-b');
  });

  it('Importing a language the person already has asks first', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      settingsReplies([summary({ locale: 'pt-BR', native_name: 'Português', kind: 'imported' })], [])(tr);
      tr.on('localisation.import_language', (i) => ((i as { replace: boolean }).replace ? ok(report()) : err('LOC_LANGUAGE_EXISTS', 'You already have that language.')));
    });
    const model = new LanguageSettingsModel(t.handle);
    await model.load();
    expect(await model.importFile(file(10))).toBeNull();
    const dialog = html(Language, { backend: t.state, model });
    expect(text(dialog)).toContain('Replace your Português?');
    // Replace is not the default button; Cancel is.
    expect(dialog).toMatch(/<button[^>]*data-default="true"[^>]*>\s*Cancel/);
    expect(dialog).not.toMatch(/data-default="true"[^>]*>\s*Replace/);
    t.transport.sent.length = 0;
    model.cancelReplace();
    expect(t.transport.sent).toEqual([]);
    // The verb resends with replace: true.
    await model.importFile(file(10));
    await model.confirmReplace();
    expect(t.transport.sentOf('localisation.import_language').map((e) => (e.input as { replace: boolean }).replace)).toEqual([false, true]);
  });

  it('A large file is not read', async () => {
    const t = await loaded(R1_GROUPS, settingsReplies([], []));
    const model = new LanguageSettingsModel(t.handle);
    let read = false;
    await model.importFile({ name: 'big.ftl', size: 3 * 1024 * 1024, text: async () => ((read = true), '') });
    expect(read).toBe(false);
    expect(t.transport.sentOf('localisation.import_language')).toEqual([]);
    expect(text(html(Language, { backend: t.state, model }))).toContain('That file is too large to be a language file.');
    expect(MAX_LANGUAGE_FILE_BYTES).toBe(2_097_152);
    // The limit itself is read.
    await model.importFile({ name: 'edge.ftl', size: MAX_LANGUAGE_FILE_BYTES, text: async () => 'x = y' });
    expect(t.transport.sentOf('localisation.import_language')).toHaveLength(1);
  });

  it('any other import error is drawn by the error line and nothing changes', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      settingsReplies([], [])(tr);
      tr.on('localisation.import_language', err('LOC_FILE_UNREADABLE', "That file isn't an ordin language file."));
    });
    const model = new LanguageSettingsModel(t.handle);
    await model.importFile(file(10));
    expect(text(html(Language, { backend: t.state, model }))).toContain("That file isn't an ordin language file.");
    expect(model.report).toBeNull();
  });

  it('Export sends localisation.export_language, then files.save_export to the local backend', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('localisation.export_language', ok({ file_name: 'ordin-language-fr-FR.ftl', ftl: 'card-open = Ouvrir' }));
      tr.on('files.save_export', ok({ saved_path: '/tmp/x.ftl' }));
    });
    const model = new LanguageSettingsModel(t.handle);
    await model.exportLanguage('fr-FR');
    expect(t.transport.commandsSent()).toEqual(['localisation.export_language', 'files.save_export']);
    expect(t.transport.sentOf('files.save_export')[0]?.input).toEqual({ file_name: 'ordin-language-fr-FR.ftl', text: 'card-open = Ouvrir' });
  });

  it('Export cancelled saves nothing', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('localisation.export_language', ok({ file_name: 'f.ftl', ftl: 'x = y' }));
      tr.on('files.save_export', err('DSK_DIALOG_CANCELLED', 'Cancelled.'));
    });
    const model = new LanguageSettingsModel(t.handle);
    await model.exportLanguage('fr-FR');
    expect(model.error).toBeNull();
    expect(text(html(Language, { backend: t.state, model }))).not.toContain('Cancelled.');
  });

  it('Export offers every language but English (UK)', async () => {
    const t = await loaded(R1_GROUPS, settingsReplies([], [offered('en-GB', 'English (UK)'), offered('fr-FR', 'Français')]));
    const model = new LanguageSettingsModel(t.handle);
    await model.load();
    expect(model.exportable.map((o) => o.locale)).toEqual(['fr-FR']);
  });
});

describe('A personal server’s languages (SM-LG-7)', () => {
  it('A personal server’s languages', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('localisation.get_settings', (_i, e) =>
        ok({ active: { locale: 'en-GB', source: 'person', direction: 'ltr', fallback_chain: ['en-GB'] }, following_operating_system: true, offered: [] }),
      );
      tr.on('localisation.list_languages', ok({ areas: [], languages: [] }));
    });
    const handle = createHandle({ id: SERVER, kind: 'server', transport: t.transport });
    const model = new LanguageSettingsModel(handle, t.handle);
    await model.load();
    expect(t.transport.sentOf('localisation.list_languages')[0]?.backend).toBe(SERVER);
    void hello;
  });

  it('LOC_LANGUAGES_UNAVAILABLE is drawn as its sentence and the actions are not offered', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('localisation.get_settings', err('LOC_LANGUAGES_UNAVAILABLE', "Languages can't be changed here."));
      tr.on('localisation.list_languages', err('LOC_LANGUAGES_UNAVAILABLE', "Languages can't be changed here."));
    });
    const model = new LanguageSettingsModel(t.handle);
    await model.load();
    const out = text(html(Language, { backend: t.state, model }));
    expect(out).toContain("Languages can't be changed here.");
    expect(out).not.toContain('Edit wording');
    expect(out).not.toContain('Import…');
  });
});

describe('The Language screen (SM-LG-3)', () => {
  async function screen(rows: unknown[], over: Record<string, unknown> = {}, setup: Parameters<typeof loaded>[1] = () => undefined) {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('localisation.list_languages', ok({ areas: ['cards'], languages: [summary(over)] }));
      tr.on('localisation.list_messages', ok({ items: rows, next_cursor: null }));
      setup(tr);
    });
    const model = new LanguageScreenModel(t.handle, 'fr-FR');
    await model.load();
    return { ...t, model };
  }
  const draw = (t: { state: never; model: LanguageScreenModel }) => clean(html(LanguageScreen, { backend: t.state, workspaceId: null, instance: 'fr-FR', model: t.model }));

  it('An edit that is refused keeps the person’s text', async () => {
    const t = await screen([row({ message_id: 'run-progress-documents-read', english: 'Read { $count } documents', text: 'Lu { $count } documents' })], {}, (tr) =>
      tr.on('localisation.edit_message', err('LOC_MESSAGE_REFUSED', '“run-progress-documents-read” wasn’t saved: it leaves out { $count }, which the English uses.')),
    );
    t.model.startEdit(t.model.rows[0]!);
    const ok1 = await t.model.save('Lu documents');
    expect(ok1).toBe(false);
    expect(t.transport.sentOf('localisation.edit_message')[0]?.input).toEqual({ locale: 'fr-FR', message_id: 'run-progress-documents-read', ftl: 'Lu documents' });
    const out = draw(t as never);
    expect(text(out)).toContain('wasn’t saved');
    // The text box still holds the person's text and the error is tied to it.
    expect(out).toContain('Lu documents</textarea>');
    expect(out).toMatch(/aria-describedby="edit-error-run-progress-documents-read"/);
    // The saved text is unchanged.
    expect(t.model.rows[0]?.text).toBe('Lu { $count } documents');
  });

  it('a saved edit redraws the row from the reply', async () => {
    const t = await screen([row()], {}, (tr) => tr.on('localisation.edit_message', ok(row({ text: 'Ouvrir ça', source: 'edited' }))));
    t.model.startEdit(t.model.rows[0]!);
    expect(await t.model.save('Ouvrir ça')).toBe(true);
    expect(t.model.editing).toBeNull();
    expect(text(draw(t as never))).toContain('Ouvrir ça');
    expect(text(draw(t as never))).toContain('Edited by you');
  });

  it('Restoring one message', async () => {
    const t = await screen([row({ source: 'edited', text: 'Ouvrir ça' })], {}, (tr) => tr.on('localisation.restore_message', ok(row())));
    expect(text(draw(t as never))).toContain('Restore');
    await t.model.restore(t.model.rows[0]!);
    expect(t.transport.sentOf('localisation.restore_message')[0]?.input).toEqual({ locale: 'fr-FR', message_id: 'card-open' });
    expect(text(draw(t as never))).toContain('Came with ordin');
  });

  it('Restore is offered only when the message has a shipped text and is not at it', async () => {
    const t = await screen([row({ source: 'shipped' }), row({ message_id: 'b', source: 'edited', shipped: null })]);
    expect(text(draw(t as never))).not.toContain('Restore');
  });

  it('Use this language waits for the approval wording', async () => {
    const t = await screen([row({ message_id: 'approval-x', safety: true, reviewed: false })], { state: 'needs_review', needs_review: 3 }, (tr) => {
      tr.on('localisation.mark_reviewed', ok(summary({ state: 'needs_review', needs_review: 0 })));
      tr.on('localisation.confirm_language', ok(summary({ state: 'in_use' })));
    });
    expect(t.model.canUse).toBe(false);
    const before = text(draw(t as never));
    expect(before).toContain('Use this language');
    expect(before).toContain('Check the approval wording first');
    expect(draw(t as never)).toMatch(/<button[^>]*disabled[^>]*>Use this language/);
    await t.model.use();
    expect(t.transport.sentOf('localisation.confirm_language')).toEqual([]);
    await t.model.markChecked(['approval-x']);
    expect(t.transport.sentOf('localisation.mark_reviewed')[0]?.input).toEqual({ locale: 'fr-FR', message_ids: ['approval-x'] });
    expect(t.model.canUse).toBe(true);
    await t.model.use();
    expect(t.transport.sentOf('localisation.confirm_language')).toHaveLength(1);
  });

  it('Mark all shown as checked sends at most 200 ids per call', async () => {
    const t = await screen([], { state: 'needs_review', needs_review: 450 }, (tr) => tr.on('localisation.mark_reviewed', ok(summary())));
    const ids = Array.from({ length: 450 }, (_, i) => `approval-${i}`);
    await t.model.markChecked(ids);
    expect(t.transport.sentOf('localisation.mark_reviewed').map((e) => (e.input as { message_ids: string[] }).message_ids.length)).toEqual([200, 200, 50]);
  });

  it('shows the English, the language’s text or "Shows in English", the source and the approval wording mark', async () => {
    const t = await screen([row({ text: null, source: 'english' }), row({ message_id: 'approval-y', safety: true, text: 'Valider' })]);
    const out = text(draw(t as never));
    expect(out).toContain('Shows in English');
    expect(out).toContain('In English');
    expect(out).toContain('Approval wording');
  });

  it('text is drawn as text and carries the language’s own direction and tag', async () => {
    const t = await screen([row({ text: '<img src=x onerror=alert(1)>', english: 'Open' })], { locale: 'fr-FR', direction: 'rtl' });
    const out = draw(t as never);
    expect(out).not.toContain('<img');
    expect(out).toMatch(/class="text" dir="rtl" lang="fr-FR"/);
    expect(out).toMatch(/class="english" dir="ltr" lang="en-GB"/);
  });

  it('Find: text, area and one filter at a time; Show more reads the next page', async () => {
    const t = await screen([row()], {}, (tr) =>
      tr.on('localisation.list_messages', (i) => ((i as { cursor?: string }).cursor === undefined ? ok({ items: [row()], next_cursor: 'c2' }) : ok({ items: [row({ message_id: 'b' })], next_cursor: null }))),
    );
    t.model.text = 'document';
    t.model.area = 'cards';
    await t.model.toggleOnly('in_english');
    expect(t.transport.sentOf('localisation.list_messages').at(-1)?.input).toEqual({ locale: 'fr-FR', text: 'document', area: 'cards', only: 'in_english' });
    await t.model.toggleOnly('edited');
    expect(t.model.only).toBe('edited');
    await t.model.toggleOnly('edited');
    expect(t.model.only).toBeNull();
    await t.model.showMore();
    expect(t.model.rows.map((r) => r.message_id)).toEqual(['card-open', 'b']);
    expect(text(draw(t as never))).toContain('2 messages');
  });

  it('Restore all and Remove this language ask first; neither verb is the default button', async () => {
    const t = await screen([row()], { edited: 4 }, (tr) => {
      tr.on('localisation.restore_language', ok(summary()));
      tr.on('localisation.remove_language', ok({}));
    });
    t.model.confirm = 'restore_all';
    const dialog = html(LanguageScreen, { backend: t.state, workspaceId: null, instance: 'fr-FR', model: t.model });
    expect(text(dialog)).toContain('Restore all of Français — fr-FR to the wording ordin came with?');
    expect(text(dialog)).toContain('Your 4 changes are removed.');
    expect(dialog).not.toMatch(/data-default="true"[^>]*>\s*Restore all/);
    await t.model.restoreAll();
    expect(t.transport.sentOf('localisation.restore_language')).toHaveLength(1);
  });

  it('the tab opens with focus on the heading', async () => {
    const t = await screen([row()]);
    expect(draw(t as never)).toMatch(/<h2 id="language-screen-heading"[^>]*tabindex="-1"[^>]*data-region-focus/);
  });

  it('removing a made language closes the tab', async () => {
    const t = await screen([], { kind: 'made' }, (tr) => tr.on('localisation.remove_language', ok({})));
    await t.model.removeLanguage();
    expect(t.model.removed).toBe(true);
    expect(text(draw(t as never))).toContain('Remove this language…');
  });
});
