import assert from 'node:assert/strict';
import { test } from 'node:test';
import { runBuildWin32Probe } from './build-win32-probe.ts';
import type { FsPort, Reporter } from '../domain/ports.ts';
import { resolveWorkspace } from '../domain/workspace.ts';

test('Win32 surface probe build only copies the static native binary', async () => {
  const calls: string[] = [];
  const cargo = {
    buildWin32: async (crate: string, profile: 'dev' | 'release') => {
      calls.push(`${crate}:${profile}`);
      return { id: 'build' as const, passed: true, durationMs: 1 };
    },
  } as never;
  const fs: FsPort = {
    ensureDir: async (path) => calls.push(`mkdir:${path}`),
    copyFile: async (from, to) => calls.push(`copy:${from}->${to}`),
    statSize: async () => 1024,
  } as never;
  const reporter: Reporter = {
    section: () => undefined,
    ok: () => undefined,
    info: () => undefined,
    warn: () => undefined,
    fail: () => undefined,
  };
  const workspace = resolveWorkspace('/repo');
  const result = await runBuildWin32Probe({ cargo, fs, reporter, workspace }, 'release');

  assert.equal(result.ok, true);
  assert.deepEqual(calls, [
    'tela-product-win32-probe:release',
    'mkdir:/repo/dist/win32-probe',
    'copy:/repo/target/x86_64-pc-windows-gnu/release/tela-win32-probe-host.exe->/repo/dist/win32-probe/tela-win32-probe-host.exe',
  ]);
});
