// pdf.js under the window's frozen `Object.prototype` (OrdinDesktop `tauri.conf.json`: `freezePrototype`;
// ScreensModule.md SM-PDF-1, amended January 2026, user decision). As for ECharts
// (echarts-frozen-prototype.test.ts), a Node process freezes the prototype first, as the window does, then
// loads the build the screens use and reads a one-page PDF's text. In Node pdf.js runs its worker in the same
// thread, so the worker's code is checked under the frozen prototype too.
//
// spec: ordin-desktop/components/ScreensModule#SM-PDF-1
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const UI = fileURLToPath(new URL('..', import.meta.url));

function read(frozen: boolean): string {
  const script = `
    const parts = ['%PDF-1.4\\n'];
    const objects = [
      '<< /Type /Catalog /Pages 2 0 R >>',
      '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
      '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>',
      null,
      '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
    ];
    const stream = 'BT /F1 18 Tf 20 100 Td (Rates go up in Q3) Tj ET';
    objects[3] = '<< /Length ' + stream.length + ' >>\\nstream\\n' + stream + '\\nendstream';
    const offsets = [];
    objects.forEach((body, i) => { offsets.push(parts.join('').length); parts.push((i + 1) + ' 0 obj\\n' + body + '\\nendobj\\n'); });
    const xref = parts.join('').length;
    parts.push('xref\\n0 6\\n0000000000 65535 f \\n' + offsets.map((o) => String(o).padStart(10, '0') + ' 00000 n \\n').join(''));
    parts.push('trailer\\n<< /Size 6 /Root 1 0 R >>\\nstartxref\\n' + xref + '\\n%%EOF');
    const data = new Uint8Array(Buffer.from(parts.join(''), 'latin1'));
    if (${frozen}) Object.freeze(Object.prototype);
    try {
      const lib = await import('pdfjs-dist/legacy/build/pdf.mjs');
      lib.GlobalWorkerOptions.workerSrc = new URL('./node_modules/pdfjs-dist/legacy/build/pdf.worker.mjs', 'file://' + process.cwd() + '/').href;
      const task = lib.getDocument({ data, isEvalSupported: false, enableXfa: false, disableRange: true, disableStream: true, disableAutoFetch: true, useWasm: false, verbosity: 0 });
      const doc = await task.promise;
      const page = await doc.getPage(1);
      const text = await page.getTextContent();
      console.log(doc.numPages + ':' + text.items.map((i) => i.str).join(''));
      await task.destroy();
    } catch (error) {
      console.log('failed: ' + error.message);
    }
  `;
  return execFileSync(process.execPath, ['--input-type=module', '-e', script], { cwd: UI, encoding: 'utf8' }).trim();
}

describe('pdf.js loads under the frozen Object.prototype (SM-PDF-1)', () => {
  it('reads a one-page PDF with the prototype as it ships', () => {
    expect(read(false)).toBe('1:Rates go up in Q3');
  });

  it('reads the same PDF with Object.prototype frozen, as the window freezes it', () => {
    expect(read(true)).toBe('1:Rates go up in Q3');
  });
});
