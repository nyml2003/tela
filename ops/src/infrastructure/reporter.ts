// 基础设施层：终端报告器（ANSI 颜色 + 表格，零依赖）。
import type { Reporter } from '../domain/ports.ts';

const ANSI = {
  reset: '\x1b[0m',
  green: '\x1b[32m',
  red: '\x1b[31m',
  yellow: '\x1b[33m',
  cyan: '\x1b[36m',
  dim: '\x1b[2m',
  bold: '\x1b[1m',
};

/** TTY 是否可用（管道输出时禁用颜色，保证 CI 日志干净）。 */
function colorEnabled(): boolean {
  return Boolean(process.stdout.isTTY) && !process.env.NO_COLOR;
}

export class TerminalReporter implements Reporter {
  private readonly color: boolean;

  constructor(color: boolean = colorEnabled()) {
    this.color = color;
  }

  private paint(code: string, text: string): string {
    return this.color ? `${code}${text}${ANSI.reset}` : text;
  }

  section(title: string): void {
    console.log('');
    console.log(this.paint(ANSI.bold + ANSI.cyan, `── ${title} ──`));
  }

  ok(msg: string): void {
    console.log(this.paint(ANSI.green, `✓ ${msg}`));
  }

  fail(msg: string): void {
    console.error(this.paint(ANSI.red, `✗ ${msg}`));
  }

  info(msg: string): void {
    console.log(this.paint(ANSI.dim, `  ${msg}`));
  }

  warn(msg: string): void {
    console.log(this.paint(ANSI.yellow, `! ${msg}`));
  }

  table(headers: readonly string[], rows: readonly (readonly string[])[]): void {
    const widths = headers.map((h, i) =>
      Math.max(h.length, ...rows.map((r) => (r[i] ?? '').length)),
    );
    const row = (cells: readonly string[]): string =>
      cells.map((c, i) => c.padEnd((widths[i] ?? 0) + 1)).join(' ').trimEnd();
    console.log(this.paint(ANSI.bold, row(headers)));
    for (const r of rows) console.log(row(r));
  }
}
