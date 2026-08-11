// domain/workspace 纯函数单测。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { resolveWorkspace } from './workspace.ts';

test('路径模型从仓库根派生', () => {
  const w = resolveWorkspace('/repo');
  assert.equal(w.cratesDir, '/repo/crates');
  assert.equal(w.demoDir, '/repo/demo');
  assert.equal(w.wasmArtifactPath('dev'), '/repo/target/wasm32-unknown-unknown/debug/tela_demo.wasm');
  assert.equal(w.wasmArtifactPath('release'), '/repo/target/wasm32-unknown-unknown/release/tela_demo.wasm');
  assert.equal(w.wasmDemoPath(), '/repo/demo/tela_demo.wasm');
});
