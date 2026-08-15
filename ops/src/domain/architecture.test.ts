// domain/architecture 纯函数单测：依赖方向规则（node:test，零依赖）。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { checkArchitecture, type CrateInfo } from './architecture.ts';

const crate = (name: string, deps: [string, string][]): CrateInfo => ({
  name,
  deps: deps.map(([n, k]) => ({ name: n, kind: k as CrateInfo['deps'][number]['kind'] })),
});

/** 合法基线：全部零依赖 crate（其他用例基于它叠加违规）。 */
const base = (): CrateInfo[] => [
  crate('tela-contract', []),
  crate('tela-log', []),
  crate('tela-fonts', []),
];

test('零依赖 crate 有依赖时报违规', () => {
  const violations = checkArchitecture([
    ...base(),
    crate('tela-contract', [['tela-core', 'normal']]),
  ]);
  assert.equal(violations.length, 1);
  assert.match(violations[0]!.message, /零依赖/);
});

test('tela-contract 零依赖时通过', () => {
  assert.deepEqual(checkArchitecture(base()), []);
});

test('core 依赖白名单之外的 crate 报违规', () => {
  const violations = checkArchitecture([
    ...base(),
    crate('tela-core', [
      ['tela-contract', 'normal'],
      ['tela-render-raster', 'normal'], // 运行时反向依赖后端，不允许
    ]),
  ]);
  assert.equal(violations.length, 1);
  assert.match(violations[0]!.message, /tela-render-raster/);
});

test('core 的 dev 依赖仅允许测试后端', () => {
  const ok = checkArchitecture([...base(), crate('tela-core', [['tela-render-raster', 'dev']])]);
  assert.deepEqual(ok, []);
  const bad = checkArchitecture([...base(), crate('tela-core', [['tela-widgets', 'dev']])]);
  assert.equal(bad.length, 1);
});

test('render 后端禁止反向依赖 core', () => {
  const violations = checkArchitecture([
    ...base(),
    crate('tela-render-raster', [
      ['tela-contract', 'normal'],
      ['tela-text', 'normal'],
      ['tela-core', 'dev'], // dev 也不行
    ]),
  ]);
  assert.equal(violations.length, 1);
  assert.match(violations[0]!.message, /禁止反向依赖 tela-core/);
});

test('完整合法 workspace 通过', () => {
  const crates: CrateInfo[] = [
    crate('tela-contract', []),
    crate('tela-fonts', []),
    crate('tela-resource-protocol', [['tela-contract', 'normal']]),
    crate('tela-core', [
      ['tela-contract', 'normal'],
      ['tela-render-raster', 'dev'],
    ]),
    crate('tela-text', [
      ['tela-contract', 'normal'],
      ['tela-fonts', 'normal'],
      ['ab_glyph', 'normal'],
    ]),
    crate('tela-icon', [
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'],
      ['tela-fonts', 'normal'],
      ['tela-text', 'normal'],
    ]),
    crate('tela-render-raster', [
      ['tela-contract', 'normal'],
      ['tela-text', 'normal'],
      ['png', 'normal'],
      ['font8x8', 'normal'],
    ]),
    crate('tela-render-canvas', [['tela-contract', 'normal']]),
    crate('tela-render-wgpu', [
      ['tela-contract', 'normal'],
      ['tela-log', 'normal'],
      ['tela-text', 'normal'],
      ['bytemuck', 'normal'],
      ['wgpu', 'normal'],
    ]),
    crate('tela-webview-sdk', [
      ['tela-app-abi', 'normal'],
      ['tela-bundle', 'normal'],
      ['tela-contract', 'normal'],
      ['tela-render-wgpu', 'normal'],
      ['serde_json', 'normal'],
      ['wasm-bindgen', 'normal'],
      ['wasm-bindgen-futures', 'normal'],
      ['web-sys', 'normal'],
      ['wgpu', 'normal'],
    ]),
    crate('tela-widgets', [
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'],
      ['tela-fonts', 'normal'],
    ]),
    crate('tela-ui', [
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'],
      ['tela-fonts', 'normal'],
      ['tela-icon', 'normal'],
      ['tela-text', 'normal'],
      ['tela-widgets', 'normal'],
    ]),
    crate('tela-log', []),
  ];
  assert.deepEqual(checkArchitecture(crates), []);
});

test('tela-icon 禁止依赖 renderer、widgets 或演示宿主', () => {
  const violations = checkArchitecture([
    ...base(),
    crate('tela-icon', [
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'],
      ['tela-fonts', 'normal'],
      ['tela-text', 'normal'],
      ['tela-render-raster', 'normal'],
    ]),
  ]);
  assert.equal(violations.length, 1);
  assert.match(violations[0]!.message, /tela-render-raster/);
});

test('tela-ui 禁止依赖渲染器或演示宿主', () => {
  const violations = checkArchitecture([
    ...base(),
    crate('tela-ui', [
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'],
      ['tela-widgets', 'normal'],
      ['tela-render-raster', 'normal'],
    ]),
  ]);
  assert.ok(violations.some((violation) => /禁止依赖 renderer/.test(violation.message)));
});

test('wgpu 后端白名单之外依赖报违规', () => {
  const violations = checkArchitecture([
    ...base(),
    crate('tela-render-wgpu', [
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'], // render 后端反向依赖 core，双违规
    ]),
  ]);
  assert.ok(violations.length >= 1);
});

test('tela-text 禁止反向依赖 core 或 renderer', () => {
  const violations = checkArchitecture([
    ...base(),
    crate('tela-text', [
      ['tela-contract', 'normal'],
      ['tela-fonts', 'normal'],
      ['tela-core', 'normal'],
    ]),
  ]);
  assert.equal(violations.length, 1);
  assert.match(violations[0]!.message, /tela-core/);
});

test('WebView SDK 不能静态依赖应用 guest', () => {
  const violations = checkArchitecture([
    ...base(),
    crate('tela-webview-sdk', [
      ['tela-app-abi', 'normal'],
      ['tela-demo', 'normal'],
    ]),
  ]);
  assert.ok(violations.some((violation) => /bundle 加载 guest/.test(violation.message)));
});
