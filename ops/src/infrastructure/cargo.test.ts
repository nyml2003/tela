import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { ProcessPort } from '../domain/ports.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import { CargoPort } from './cargo.ts';

test('Win32 构建使用隔离的 nix 交叉 cargo，WASM 保持普通 cargo', async () => {
  const calls: { command: string; args: string[] }[] = [];
  const process: ProcessPort = {
    run: async (command, args) => {
      calls.push({ command, args });
      return { code: 0, stdout: '', stderr: '' };
    },
  };
  const cargo = new CargoPort(process, resolveWorkspace('/repo'));

  await cargo.buildWasm('tela-demo', 'dev', ['app-wasm']);
  await cargo.buildWin32('tela-win32-sdk', 'release');

  assert.deepEqual(calls, [
    {
      command: 'cargo',
      args: ['build', '--target', 'wasm32-unknown-unknown', '-p', 'tela-demo', '--features', 'app-wasm'],
    },
    {
      command: 'cargo-win32',
      args: ['build', '--target', 'x86_64-pc-windows-gnu', '-p', 'tela-win32-sdk', '--release'],
    },
  ]);
});
