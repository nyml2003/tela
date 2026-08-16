import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { ProcessPort } from '../domain/ports.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import { CargoPort } from './cargo.ts';

test('Win32、iOS 使用各自隔离交叉 cargo，macOS 与 WASM 保持既有命令', async () => {
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
  await cargo.buildMacos('tela-macos-sdk', 'dev');
  await cargo.buildIos('tela-ios-sdk', 'release');

  assert.deepEqual(calls, [
    {
      command: 'cargo',
      args: ['build', '--target', 'wasm32-unknown-unknown', '-p', 'tela-demo', '--features', 'app-wasm'],
    },
    {
      command: 'cargo-win32',
      args: ['build', '--target', 'x86_64-pc-windows-gnu', '-p', 'tela-win32-sdk', '--release'],
    },
    {
      command: 'cargo',
      args: ['build', '--target', 'aarch64-apple-darwin', '-p', 'tela-macos-sdk'],
    },
    {
      command: 'tela-ios-cargo',
      args: ['build', '--target', 'aarch64-apple-ios', '-p', 'tela-ios-sdk', '--release'],
    },
  ]);
});
