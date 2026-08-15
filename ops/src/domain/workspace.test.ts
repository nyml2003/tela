// domain/workspace 纯函数单测。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { resolveWorkspace } from './workspace.ts';

test('路径模型从仓库根派生', () => {
  const w = resolveWorkspace('/repo');
  assert.equal(w.cratesDir, '/repo/crates');
  assert.equal(w.distDir, '/repo/dist');
  assert.equal(w.wasmArtifactPath('dev'), '/repo/target/wasm32-unknown-unknown/debug/tela_demo.wasm');
  assert.equal(w.wasmArtifactPath('release'), '/repo/target/wasm32-unknown-unknown/release/tela_demo.wasm');
  assert.equal(w.wasmDistPath(), '/repo/dist/tela_demo.wasm');
  assert.equal(w.bundleDir(), '/repo/dist/tela-dev');
  assert.equal(w.bundleArchivePath(), '/repo/dist/tela-dev/tela-demo.tela');
  assert.equal(w.bundleIndexPath(), '/repo/dist/tela-dev/latest.json');
  assert.equal(w.win32ArtifactPath('dev'), '/repo/target/x86_64-pc-windows-gnu/debug/tela-win32-sdk.exe');
  assert.equal(w.win32DistPath(), '/repo/dist/win32/tela-win32-sdk.exe');
});
