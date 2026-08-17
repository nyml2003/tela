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

  await cargo.buildWasm('tela-product-desktop-guest', 'dev');
  await cargo.buildWin32('tela-target-win32', 'release');
  await cargo.buildMacos('tela-target-macos', 'dev');
  await cargo.buildIos('tela-product-ios', 'release');
  await cargo.checkPackages(['tela-contract', 'tela-core', 'tela-ui-foundation']);

  assert.deepEqual(calls, [
    {
      command: 'cargo',
      args: ['build', '--target', 'wasm32-unknown-unknown', '-p', 'tela-product-desktop-guest'],
    },
    {
      command: 'cargo-win32',
      args: ['build', '--target', 'x86_64-pc-windows-gnu', '-p', 'tela-target-win32', '--release'],
    },
    {
      command: 'cargo',
      args: ['build', '--target', 'aarch64-apple-darwin', '-p', 'tela-target-macos'],
    },
    {
      command: 'tela-ios-cargo',
      args: ['build', '--target', 'aarch64-apple-ios', '-p', 'tela-product-ios', '--release'],
    },
    {
      command: 'cargo',
      args: ['check', '-p', 'tela-contract', '-p', 'tela-core', '-p', 'tela-ui-foundation'],
    },
  ]);
});
