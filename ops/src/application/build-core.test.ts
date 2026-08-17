import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { Reporter } from '../domain/ports.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import { runBuildCore } from './build-core.ts';

class TestReporter implements Reporter {
  readonly messages: string[] = [];
  section(message: string): void { this.messages.push(message); }
  ok(message: string): void { this.messages.push(message); }
  fail(message: string): void { this.messages.push(message); }
  info(message: string): void { this.messages.push(message); }
  warn(message: string): void { this.messages.push(message); }
  table(): void {}
}

test('core 产品只检查 Kernel 和 UI foundation 闭包', async () => {
  let packages: readonly string[] = [];
  const result = await runBuildCore({
    cargo: {
      checkPackages: async (selected: readonly string[]) => {
        packages = selected;
        return { id: 'build' as const, passed: true, durationMs: 1 };
      },
    } as never,
    reporter: new TestReporter(),
    workspace: resolveWorkspace('/repo'),
  });

  assert.deepEqual(result, { ok: true });
  assert.deepEqual(packages, ['tela-contract', 'tela-core', 'tela-ui-foundation']);
});

test('core 闭包失败时保留 cargo 失败详情', async () => {
  const reporter = new TestReporter();
  const result = await runBuildCore({
    cargo: {
      checkPackages: async () => ({
        id: 'build' as const,
        passed: false,
        durationMs: 1,
        detail: 'missing dependency',
      }),
    } as never,
    reporter,
    workspace: resolveWorkspace('/repo'),
  });

  assert.deepEqual(result, { ok: false });
  assert.ok(reporter.messages.includes('missing dependency'));
});
