// 基础设施层：进程执行适配器（Node 子进程，零依赖）。
import { spawn } from 'node:child_process';
import type { ProcessOptions, ProcessPort, ProcessResult } from '../domain/ports.ts';

/** 子进程执行：捕获 stdout/stderr（可能超长截断尾部），失败不抛异常、返回码为准。 */
export class NodeProcessPort implements ProcessPort {
  private static readonly MAX_CAPTURE = 64 * 1024;

  async run(cmd: string, args: string[], opts: ProcessOptions = {}): Promise<ProcessResult> {
    return new Promise((resolve, reject) => {
      const child = spawn(cmd, args, {
        cwd: opts.cwd,
        stdio: ['ignore', 'pipe', 'pipe'],
      });
      let stdout = '';
      let stderr = '';
      const cap = (buf: Buffer, sink: (s: string) => void) => {
        const s = buf.toString('utf8');
        sink(s);
      };
      child.stdout.on('data', (d: Buffer) => cap(d, (s) => { stdout += s; }));
      child.stderr.on('data', (d: Buffer) => cap(d, (s) => { stderr += s; }));
      child.on('error', (err) => reject(err));
      child.on('close', (code) => {
        // 截断过长输出，保留头尾（clippy/test 可能刷屏）。
        const truncate = (s: string): string =>
          s.length > NodeProcessPort.MAX_CAPTURE
            ? `${s.slice(0, NodeProcessPort.MAX_CAPTURE / 2)}\n…(截断)…\n${s.slice(-NodeProcessPort.MAX_CAPTURE / 2)}`
            : s;
        resolve({ code: code ?? -1, stdout: truncate(stdout), stderr: truncate(stderr) });
      });
    });
  }
}
