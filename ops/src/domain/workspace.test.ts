// domain/workspace 纯函数单测。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { resolveWorkspace } from './workspace.ts';

test('路径模型从仓库根派生，并把原生输入放在产品根', () => {
  const workspace = resolveWorkspace('/repo');

  assert.equal(workspace.cratesDir, '/repo/crates');
  assert.equal(workspace.productsDir, '/repo/products');
  assert.equal(workspace.webviewProductDir, '/repo/products/webview');
  assert.equal(workspace.webviewTargetArtifactPath('release'), '/repo/target/wasm32-unknown-unknown/release/tela_target_webview.wasm');
  assert.equal(workspace.webviewHostGluePath(), '/repo/dist/tela_webview_host.js');
  assert.equal(workspace.webviewHostWasmPath(), '/repo/dist/tela_webview_host_bg.wasm');
  assert.equal(workspace.agentDemoArtifactPath('release'), '/repo/target/wasm32-unknown-unknown/release/tela_product_agent_demo.wasm');
  assert.equal(workspace.agentDemoGluePath(), '/repo/dist/tela_agent_demo.js');
  assert.equal(workspace.agentDemoWasmPath(), '/repo/dist/tela_agent_demo_bg.wasm');

  const desktop = workspace.bundle('desktop');
  assert.equal(desktop.guestCrate, 'tela-product-desktop-guest');
  assert.deepEqual(desktop.guestFeatures, []);
  assert.equal(desktop.guestWasmArtifactPath('release'), '/repo/target/wasm32-unknown-unknown/release/tela_product_desktop_guest.wasm');
  assert.equal(desktop.archivePath(), '/repo/dist/tela-dev/tela-desktop-guest.tela');
  assert.equal(desktop.indexPath(), '/repo/dist/tela-dev/latest.json');

  const mobile = workspace.bundle('mobile');
  assert.equal(mobile.guestCrate, 'tela-product-mobile-guest');
  assert.equal(mobile.guestWasmArtifactPath('release'), '/repo/target/wasm32-unknown-unknown/release/tela_product_mobile_guest.wasm');
  assert.equal(mobile.archivePath(), '/repo/dist/tela-mobile/tela-mobile-guest.tela');
  assert.equal(mobile.archiveUrl, '/tela-mobile/tela-mobile-guest.tela');

  assert.equal(workspace.win32ArtifactPath('dev'), '/repo/target/x86_64-pc-windows-gnu/debug/tela-win32-host.exe');
  assert.equal(workspace.win32DistPath(), '/repo/dist/win32/tela-win32-host.exe');
  assert.equal(workspace.macosExecutablePath(), '/repo/dist/macos/Tela.app/Contents/MacOS/tela-macos-host');
  assert.equal(workspace.macosInfoPlistSourcePath(), '/repo/products/macos/resources/Info.plist');
  assert.equal(workspace.macosArtifactPath('release'), '/repo/target/aarch64-apple-darwin/release/tela-macos-host');
  assert.equal(workspace.androidProjectDir(), '/repo/products/android');
  assert.equal(workspace.androidJniAbiDir(), '/repo/products/android/app/src/main/jniLibs/arm64-v8a');
  assert.equal(workspace.androidDebugApkPath(), '/repo/products/android/app/build/outputs/apk/debug/app-debug.apk');
  assert.equal(workspace.iosProjectDir(), '/repo/products/ios');
  assert.equal(workspace.iosRustStaticLibraryPath('release'), '/repo/target/aarch64-apple-ios/release/libtela_product_ios.a');
  assert.equal(workspace.iosXcodeStaticLibraryPath(), '/repo/products/ios/build/rust/libtela_product_ios.a');
});

test('六个产品显式选择自身的应用、交付、renderer 与 Target', () => {
  const workspace = resolveWorkspace('/repo');

  const core = workspace.product('core');
  assert.equal(core.delivery, 'none');
  assert.deepEqual(core.packages, ['tela-contract', 'tela-core', 'tela-ui-foundation']);

  const webview = workspace.product('webview');
  assert.deepEqual(webview, {
    id: 'webview',
    root: '/repo/products/webview',
    application: 'tela-product-desktop-guest',
    delivery: 'dynamic-bundle',
    renderer: 'tela-render-wgpu',
    target: 'tela-target-webview',
    packages: ['tela-product-desktop-guest', 'tela-target-webview'],
  });

  const android = workspace.product('android');
  assert.equal(android.application, 'tela-product-mobile-guest');
  assert.equal(android.target, 'tela-target-android');
  assert.equal(android.root, '/repo/products/android');

  assert.deepEqual(workspace.product('agent-demo'), {
    id: 'agent-demo',
    root: '/repo/products/agent-demo',
    application: 'tela-agent-demo',
    delivery: 'static-link',
    renderer: 'tela-render-wgpu',
    target: 'tela-target-webview',
    packages: ['tela-product-agent-demo'],
  });

  const ios = workspace.product('ios');
  assert.equal(ios.application, 'tela-product-ios');
  assert.equal(ios.delivery, 'static-link');
  assert.equal(ios.target, 'tela-target-ios');

  assert.equal(workspace.product('win32').target, 'tela-target-win32');
  assert.equal(workspace.product('win32-editor').target, 'tela-target-win32');
  assert.deepEqual(workspace.product('win32-probe'), {
    id: 'win32-probe',
    root: '/repo/products/win32-probe',
    application: 'tela-win32-probe',
    delivery: 'static-link',
    target: 'tela-target-win32',
    packages: ['tela-product-win32-probe'],
  });
  assert.equal(workspace.win32ProbeArtifactPath('release'), '/repo/target/x86_64-pc-windows-gnu/release/tela-win32-probe-host.exe');
  assert.equal(workspace.win32ProbeDistPath(), '/repo/dist/win32-probe/tela-win32-probe-host.exe');
  assert.deepEqual(workspace.product('win32-agent'), {
    id: 'win32-agent',
    root: '/repo/products/win32-agent',
    application: 'tela-agent-demo',
    delivery: 'static-link',
    renderer: 'tela-render-wgpu',
    target: 'tela-target-win32',
    packages: ['tela-product-win32-agent'],
  });
  assert.equal(workspace.win32AgentArtifactPath('release'), '/repo/target/x86_64-pc-windows-gnu/release/tela-win32-agent-host.exe');
  assert.equal(workspace.win32AgentDistPath(), '/repo/dist/win32-agent/tela-win32-agent-host.exe');
  assert.equal(workspace.product('speed-gear').target, 'tela-target-win32');
  assert.equal(workspace.product('macos').target, 'tela-target-macos');
});
