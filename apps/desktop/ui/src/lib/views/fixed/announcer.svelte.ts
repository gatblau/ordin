import { stripIsolates } from '../../i18n/fluent';

// A polite live-region message: set empty first so a repeated sentence is announced again.
export class Announcer {
  text = $state('');
  say(text: string): void {
    this.text = '';
    queueMicrotask(() => {
      this.text = stripIsolates(text);
    });
  }
}
