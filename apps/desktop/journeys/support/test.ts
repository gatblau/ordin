// The `app` every journey gets: the real window, attached, and the two checks every journey ends with —
// no error message appeared on screen, and the log holds no WARNING or ERROR line the journey did not expect.
import { test as base, expect, type Browser, type Page, type TestInfo } from '@playwright/test';
import { attach, isRunning, startOrdin, stopOrdin } from './ordin';
import { describe, eventOf, expectedEverywhere, isProblem, LogWatch, type LogLine } from './log';

/** Records the text of every error message the screens draw (`role="alert"`, ErrorLine), as it appears. */
const WATCH_ERRORS = `(() => {
  if (window.__journeyErrors) return;
  window.__journeyErrors = [];
  const scan = () => document.querySelectorAll('[role=alert], .error-line').forEach((el) => {
    const text = (el.textContent || '').replace(/[\\u2068\\u2069]/g, '').replace(/\\s+/g, ' ').trim();
    if (text && el.__journeySeen !== text) { el.__journeySeen = text; window.__journeyErrors.push(text); }
  });
  const start = () => { new MutationObserver(scan).observe(document.documentElement, { subtree: true, childList: true, characterData: true }); scan(); };
  if (document.documentElement) start(); else document.addEventListener('DOMContentLoaded', start);
})()`;

export class App {
  page!: Page;
  private browser?: Browser;
  readonly log = new LogWatch();
  private problems: LogLine[] = [];
  private expectedMessages: string[] = [];
  private expectedErrors: RegExp[] = [];
  private shownErrors: string[] = [];
  private shots = 0;

  constructor(private readonly info: TestInfo) {}

  /** Attaches to the program, starting it first when it is not running. */
  async open(): Promise<void> {
    if (!isRunning()) startOrdin();
    const { browser, page } = await attach();
    this.browser = browser;
    this.page = page;
    page.setDefaultTimeout(30_000);
    await page.addInitScript(WATCH_ERRORS);
    // The page may be loading again (the first start moves from setup to Home): the init script covers it.
    await page.evaluate(WATCH_ERRORS).catch(() => undefined);
    // A journey starts with nothing open over the window: a search or a dialog the one before it left.
    for (let i = 0; i < 2; i++) await page.keyboard.press('Escape');
  }

  /** Stops the program at once (as a power cut would) and starts it again. */
  async restart(): Promise<void> {
    await this.collect();
    await this.browser?.close().catch(() => undefined);
    await stopOrdin();
    startOrdin();
    await this.open();
  }

  /** Detaches without stopping the program, for a step that stops it some other way. */
  async detach(): Promise<void> {
    await this.collect().catch(() => undefined);
    await this.browser?.close().catch(() => undefined);
    this.browser = undefined;
  }

  /** Log messages this journey expects at WARNING or ERROR, beyond expected-log.txt. */
  expectLog(...messages: string[]): void {
    this.expectedMessages.push(...messages);
  }

  /** Error messages this journey expects to see on screen. */
  expectError(...texts: RegExp[]): void {
    this.expectedErrors.push(...texts);
  }

  /**
   * Waits until the program logs `message` (an event the person cannot see, such as a fact being stored).
   * The lines read on the way still go to the log check at the end of the journey.
   */
  async waitForLog(message: string, timeoutMs = 60_000): Promise<void> {
    const until = Date.now() + timeoutMs;
    for (;;) {
      const lines = this.log.read();
      this.problems.push(...lines.filter(isProblem));
      if (lines.some((l) => eventOf(l) === message)) return;
      if (Date.now() > until) throw new Error(`the log did not show ${message} within ${timeoutMs / 1000} s`);
      await new Promise((r) => setTimeout(r, 250));
    }
  }

  /** A screenshot in the report, named after the step. */
  async shot(name: string): Promise<void> {
    this.shots += 1;
    const path = this.info.outputPath(`${String(this.shots).padStart(2, '0')}-${name}.png`);
    await this.page.screenshot({ path });
    await this.info.attach(name, { path, contentType: 'image/png' });
  }

  /** One step of the journey: what the person does, then a screenshot of what they see. */
  async step(name: string, body: () => Promise<void>): Promise<void> {
    await test.step(name, async () => {
      try {
        await body();
      } finally {
        await this.shot(name.replace(/[^A-Za-z0-9]+/g, '-').toLowerCase()).catch(() => undefined);
      }
    });
  }

  private async collect(): Promise<void> {
    this.problems.push(...this.log.read().filter(isProblem));
    if (this.page && !this.page.isClosed()) {
      const shown = await this.page.evaluate(() => (window as unknown as { __journeyErrors?: string[] }).__journeyErrors?.splice(0) ?? []).catch(() => []);
      this.shownErrors.push(...shown);
    }
  }

  /** The two checks every journey ends with. */
  async check(): Promise<void> {
    await this.collect();
    const allowed = [...expectedEverywhere(), ...this.expectedMessages];
    const unexpectedLog = this.problems.filter((l) => !allowed.includes(eventOf(l)));
    const unexpectedErrors = this.shownErrors.filter((t) => !this.expectedErrors.some((re) => re.test(t)));
    const report = [
      ...unexpectedErrors.map((t) => `error message on screen: ${t}`),
      ...unexpectedLog.map((l) => `log: ${describe(l)}`),
    ];
    if (report.length > 0) await this.info.attach('problems', { body: report.join('\n'), contentType: 'text/plain' });
    expect(unexpectedErrors, 'no error message appears that the journey did not expect').toEqual([]);
    expect(unexpectedLog.map(describe), 'the log holds no WARNING or ERROR line outside expected-log.txt and the journey\'s own list').toEqual([]);
  }

  async close(): Promise<void> {
    await this.browser?.close().catch(() => undefined);
  }
}

export const test = base.extend<{ app: App }>({
  app: async ({}, use, info) => {
    const app = new App(info);
    // What the log held before this journey belongs to the one before it.
    app.log.skip();
    await app.open();
    await use(app);
    if (info.status !== info.expectedStatus) await app.shot('at-failure').catch(() => undefined);
    try {
      if (info.status === info.expectedStatus) await app.check();
    } finally {
      await app.close();
    }
  },
});

export { expect };
