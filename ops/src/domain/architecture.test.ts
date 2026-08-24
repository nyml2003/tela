// domain/architecture 纯函数单测：026 产品依赖方向与 030 UI 组件边界。
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { checkArchitecture, type CrateInfo } from './architecture.ts';

type Dependency = readonly [string, 'normal' | 'dev' | 'build'];

const crate = (name: string, deps: readonly Dependency[] = []): CrateInfo => ({
  name,
  deps: deps.map(([dependency, kind]) => ({ name: dependency, kind })),
});

const zeroDependencies = (): CrateInfo[] => [
  crate('tela-contract'),
  crate('tela-font-resources'),
  crate('tela-log'),
];

test('零依赖 crate 的 normal、dev、build 依赖都被拒绝', () => {
  const violations = checkArchitecture([
    ...zeroDependencies(),
    crate('tela-contract', [['tela-core', 'build']]),
  ]);

  assert.equal(violations.length, 1);
  assert.match(violations[0]!.message, /零依赖/);
});

test('Kernel Core 只依赖 Contract', () => {
  const violations = checkArchitecture([
    ...zeroDependencies(),
    crate('tela-core', [
      ['tela-contract', 'normal'],
      ['tela-render-raster', 'dev'],
    ]),
  ]);

  assert.equal(violations.length, 1);
  assert.match(violations[0]!.message, /tela-render-raster/);
});

test('Renderer 仅允许通过 dev 依赖使用 Core 进行跨层验证', () => {
  const allowed = checkArchitecture([
    ...zeroDependencies(),
    crate('tela-render-raster', [
      ['font8x8', 'normal'],
      ['png', 'normal'],
      ['tela-contract', 'normal'],
      ['tela-text-resources', 'normal'],
      ['tela-core', 'dev'],
    ]),
  ]);
  assert.deepEqual(allowed, []);

  const violations = checkArchitecture([
    ...zeroDependencies(),
    crate('tela-render-raster', [
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'],
    ]),
  ]);
  assert.ok(violations.some((violation) => /反向依赖 tela-core/.test(violation.message)));
});

test('UI Capability 不可直接认识具体资源或 renderer', () => {
  const violations = checkArchitecture([
    ...zeroDependencies(),
    crate('tela-ui-foundation', [
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'],
      ['tela-icon-resources', 'normal'],
    ]),
  ]);

  assert.ok(violations.some((violation) => /tela-icon-resources/.test(violation.message)));
});

test('Composition runtime 只依赖 Kernel 与自己的 macro helper', () => {
  const allowed = checkArchitecture([
    ...zeroDependencies(),
    crate('tela-ui-dsl-macros', [
      ['proc-macro-crate', 'normal'],
      ['proc-macro2', 'normal'],
      ['quote', 'normal'],
      ['syn', 'normal'],
    ]),
    crate('tela-ui-dsl', [
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'],
      ['tela-ui-dsl-macros', 'normal'],
      ['trybuild', 'dev'],
    ]),
  ]);
  assert.deepEqual(allowed, []);

  const violations = checkArchitecture([
    ...zeroDependencies(),
    crate('tela-ui-dsl', [
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'],
      ['tela-target-ios', 'normal'],
    ]),
  ]);
  assert.ok(violations.some((violation) => /Composition runtime/.test(violation.message)));
});

test('Presentation 通过 Contract 协议提供资源，不能反向依赖 UI kit', () => {
  const violations = checkArchitecture([
    ...zeroDependencies(),
    crate('tela-icon-resources', [
      ['tela-contract', 'normal'],
      ['tela-text-resources', 'normal'],
      ['tela-ui-foundation', 'normal'],
    ]),
  ]);

  assert.ok(violations.some((violation) => /Presentation/.test(violation.message)));
});

test('Application 可在测试中注入资源，但生产闭包不能静态耦合资源实现', () => {
  const allowed = checkArchitecture([
    ...zeroDependencies(),
    crate('tela-mobile-demo', [
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'],
      ['tela-mobile-ui-kit', 'normal'],
      ['tela-ui-dsl', 'normal'],
      ['tela-ui-foundation', 'normal'],
      ['tela-icon-resources', 'dev'],
      ['tela-render-raster', 'dev'],
      ['tela-text-resources', 'dev'],
    ]),
  ]);
  assert.deepEqual(allowed, []);

  const violations = checkArchitecture([
    ...zeroDependencies(),
    crate('tela-mobile-demo', [
      ['tela-contract', 'normal'],
      ['tela-icon-resources', 'normal'],
    ]),
  ]);
  assert.ok(violations.some((violation) => /tela-icon-resources/.test(violation.message)));
});

test('Target 不能静态链接应用或另一个 Target', () => {
  const violations = checkArchitecture([
    ...zeroDependencies(),
    crate('tela-target-android', [
      ['jni', 'normal'],
      ['libc', 'normal'],
      ['ndk-sys', 'normal'],
      ['tela-contract', 'normal'],
      ['tela-mobile-demo', 'normal'],
      ['tela-target-ios', 'normal'],
    ]),
  ]);

  assert.ok(violations.some((violation) => /静态依赖 Application 或 Product/.test(violation.message)));
  assert.ok(violations.some((violation) => /其他 Target/.test(violation.message)));
});

test('iOS Target 保持宿主边界，静态组合由 iOS Product 负责', () => {
  const targetViolations = checkArchitecture([
    ...zeroDependencies(),
    crate('tela-target-ios', [
      ['objc2', 'normal'],
      ['objc2-foundation', 'normal'],
      ['objc2-quartz-core', 'normal'],
      ['tela-contract', 'normal'],
      ['tela-mobile-demo', 'normal'],
    ]),
  ]);
  assert.ok(targetViolations.some((violation) => /静态依赖 Application 或 Product/.test(violation.message)));

  const productViolations = checkArchitecture([
    ...zeroDependencies(),
    crate('tela-product-ios', [
      ['tela-contract', 'normal'],
      ['tela-icon-resources', 'normal'],
      ['tela-mobile-demo', 'normal'],
      ['tela-target-ios', 'normal'],
      ['tela-text-resources', 'normal'],
      ['tela-ui-foundation', 'normal'],
      ['tela-guest-runtime', 'normal'],
    ]),
  ]);
  assert.ok(productViolations.some((violation) => /动态 Delivery 链路/.test(violation.message)));
});

test('Delivery 与 Guest Runtime 不可反向持有 Target', () => {
  const violations = checkArchitecture([
    ...zeroDependencies(),
    crate('tela-guest-runtime', [
      ['tela-app-abi', 'normal'],
      ['tela-target-webview', 'normal'],
    ]),
  ]);

  assert.ok(violations.some((violation) => /Delivery 禁止依赖/.test(violation.message)));
  assert.ok(violations.some((violation) => /Guest Runtime 禁止依赖/.test(violation.message)));
});

test('动态 Product 不能被 Target 或错误应用污染', () => {
  const violations = checkArchitecture([
    ...zeroDependencies(),
    crate('tela-product-mobile-guest', [
      ['tela-app-abi', 'normal'],
      ['tela-contract', 'normal'],
      ['tela-desktop-demo', 'normal'],
      ['tela-icon-resources', 'normal'],
      ['tela-mobile-demo', 'normal'],
      ['tela-text-resources', 'normal'],
      ['tela-ui-foundation', 'normal'],
      ['tela-target-android', 'normal'],
    ]),
  ]);

  assert.ok(violations.some((violation) => /移动动态 Product/.test(violation.message)));
});

test('完整的 026/030 workspace 依赖闭包通过', () => {
  const crates: CrateInfo[] = [
    crate('tela-contract'),
    crate('tela-font-resources'),
    crate('tela-log'),
    crate('tela-app-abi', [
      ['postcard', 'normal'],
      ['serde', 'normal'],
      ['tela-contract', 'normal'],
    ]),
    crate('tela-bundle', [
      ['hex', 'normal'],
      ['serde', 'normal'],
      ['serde_json', 'normal'],
      ['sha2', 'normal'],
      ['tela-app-abi', 'normal'],
      ['zip', 'normal'],
    ]),
    crate('tela-core', [['tela-contract', 'normal']]),
    crate('tela-ui-dsl-macros', [
      ['proc-macro-crate', 'normal'],
      ['proc-macro2', 'normal'],
      ['quote', 'normal'],
      ['syn', 'normal'],
    ]),
    crate('tela-ui-dsl', [
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'],
      ['tela-ui-dsl-macros', 'normal'],
      ['trybuild', 'dev'],
    ]),
    crate('tela-desktop-demo', [
      ['serde', 'normal'],
      ['serde_json', 'normal'],
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'],
      ['tela-desktop-ui-dsl', 'normal'],
      ['tela-desktop-ui-kit', 'normal'],
      ['tela-ui-dsl', 'normal'],
      ['tela-ui-foundation', 'normal'],
      ['tela-icon-resources', 'dev'],
      ['tela-render-raster', 'dev'],
      ['tela-text-resources', 'dev'],
    ]),
    crate('tela-desktop-runtime', [
      ['tela-bundle', 'normal'],
      ['tela-guest-runtime', 'normal'],
      ['serde_json', 'dev'],
      ['tela-app-abi', 'dev'],
    ]),
    crate('tela-desktop-ui-kit', [
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'],
      ['tela-ui-foundation', 'normal'],
    ]),
    crate('tela-guest-runtime', [
      ['serde_json', 'normal'],
      ['tela-app-abi', 'normal'],
      ['tela-bundle', 'normal'],
      ['tela-contract', 'normal'],
      ['wasmtime', 'normal'],
    ]),
    crate('tela-icon-resources', [
      ['tela-contract', 'normal'],
      ['tela-text-resources', 'normal'],
    ]),
    crate('tela-mobile-demo', [
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'],
      ['tela-mobile-ui-kit', 'normal'],
      ['tela-ui-dsl', 'normal'],
      ['tela-ui-foundation', 'normal'],
      ['tela-icon-resources', 'dev'],
      ['tela-render-raster', 'dev'],
      ['tela-text-resources', 'dev'],
    ]),
    crate('tela-mobile-ui-kit', [
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'],
      ['tela-ui-foundation', 'normal'],
    ]),
    crate('tela-product-desktop-guest', [
      ['tela-app-abi', 'normal'],
      ['tela-contract', 'normal'],
      ['tela-desktop-demo', 'normal'],
      ['tela-icon-resources', 'normal'],
      ['tela-text-resources', 'normal'],
    ]),
    crate('tela-product-ios', [
      ['tela-contract', 'normal'],
      ['tela-icon-resources', 'normal'],
      ['tela-mobile-demo', 'normal'],
      ['tela-target-ios', 'normal'],
      ['tela-text-resources', 'normal'],
    ]),
    crate('tela-product-mobile-guest', [
      ['tela-app-abi', 'normal'],
      ['tela-contract', 'normal'],
      ['tela-icon-resources', 'normal'],
      ['tela-mobile-demo', 'normal'],
      ['tela-text-resources', 'normal'],
    ]),
    crate('tela-render-canvas', [['tela-contract', 'normal']]),
    crate('tela-render-raster', [
      ['font8x8', 'normal'],
      ['png', 'normal'],
      ['tela-contract', 'normal'],
      ['tela-text-resources', 'normal'],
      ['tela-core', 'dev'],
    ]),
    crate('tela-render-wgpu', [
      ['bytemuck', 'normal'],
      ['tela-contract', 'normal'],
      ['tela-log', 'normal'],
      ['tela-text-resources', 'normal'],
      ['wgpu', 'normal'],
      ['naga', 'dev'],
      ['pollster', 'dev'],
    ]),
    crate('tela-resource-protocol', [['tela-contract', 'normal']]),
    crate('tela-target-android', [
      ['jni', 'normal'],
      ['libc', 'normal'],
      ['ndk-sys', 'normal'],
      ['pollster', 'normal'],
      ['tela-app-abi', 'normal'],
      ['tela-contract', 'normal'],
      ['tela-guest-runtime', 'normal'],
      ['tela-log', 'normal'],
      ['tela-render-wgpu', 'normal'],
      ['ureq', 'normal'],
      ['wgpu', 'normal'],
      ['winit', 'normal'],
    ]),
    crate('tela-target-ios', [
      ['objc2', 'normal'],
      ['objc2-foundation', 'normal'],
      ['objc2-quartz-core', 'normal'],
      ['pollster', 'normal'],
      ['raw-window-handle', 'normal'],
      ['tela-contract', 'normal'],
      ['tela-render-wgpu', 'normal'],
      ['wgpu', 'normal'],
      ['winit', 'normal'],
    ]),
    crate('tela-target-macos', [
      ['objc2', 'normal'],
      ['objc2-app-kit', 'normal'],
      ['objc2-foundation', 'normal'],
      ['objc2-quartz-core', 'normal'],
      ['pollster', 'normal'],
      ['raw-window-handle', 'normal'],
      ['tela-app-abi', 'normal'],
      ['tela-contract', 'normal'],
      ['tela-desktop-runtime', 'normal'],
      ['tela-render-wgpu', 'normal'],
      ['ureq', 'normal'],
      ['wgpu', 'normal'],
    ]),
    crate('tela-target-webview', [
      ['serde_json', 'normal'],
      ['tela-app-abi', 'normal'],
      ['tela-bundle', 'normal'],
      ['tela-contract', 'normal'],
      ['tela-render-wgpu', 'normal'],
      ['wasm-bindgen', 'normal'],
      ['wasm-bindgen-futures', 'normal'],
      ['web-sys', 'normal'],
      ['wgpu', 'normal'],
    ]),
    crate('tela-target-win32', [
      ['pollster', 'normal'],
      ['raw-window-handle', 'normal'],
      ['tela-app-abi', 'normal'],
      ['tela-contract', 'normal'],
      ['tela-desktop-runtime', 'normal'],
      ['tela-render-wgpu', 'normal'],
      ['ureq', 'normal'],
      ['wgpu', 'normal'],
      ['windows', 'normal'],
    ]),
    crate('tela-text-resources', [
      ['ab_glyph', 'normal'],
      ['tela-contract', 'normal'],
      ['tela-font-resources', 'normal'],
    ]),
    crate('tela-ui-foundation', [
      ['tela-contract', 'normal'],
      ['tela-core', 'normal'],
    ]),
  ];

  assert.deepEqual(checkArchitecture(crates), []);
});
