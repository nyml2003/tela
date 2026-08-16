// domain/workspace 纯函数单测。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { resolveWorkspace } from './workspace.ts';

test('路径模型从仓库根派生', () => {
  const w = resolveWorkspace('/repo');
  assert.equal(w.cratesDir, '/repo/crates');
  assert.equal(w.distDir, '/repo/dist');
  assert.equal(w.appGuestWasmArtifactPath('dev'), '/repo/target/wasm32-unknown-unknown/debug/tela_demo.wasm');
  assert.equal(w.appGuestWasmArtifactPath('release'), '/repo/target/wasm32-unknown-unknown/release/tela_demo.wasm');
  assert.equal(w.webviewSdkArtifactPath('release'), '/repo/target/wasm32-unknown-unknown/release/tela_webview_sdk.wasm');
  assert.equal(w.webviewSdkGluePath(), '/repo/dist/tela_webview_sdk.js');
  assert.equal(w.webviewSdkWasmPath(), '/repo/dist/tela_webview_sdk_bg.wasm');
  assert.equal(w.bundleDir(), '/repo/dist/tela-dev');
  assert.equal(w.bundleArchivePath(), '/repo/dist/tela-dev/tela-demo.tela');
  assert.equal(w.bundleIndexPath(), '/repo/dist/tela-dev/latest.json');
  const mobile = w.bundle('mobile');
  assert.equal(mobile.guestCrate, 'tela-mobile-demo');
  assert.equal(mobile.guestWasmArtifactPath('release'), '/repo/target/wasm32-unknown-unknown/release/tela_mobile_demo.wasm');
  assert.equal(mobile.archivePath(), '/repo/dist/tela-mobile/tela-mobile-demo.tela');
  assert.equal(mobile.indexPath(), '/repo/dist/tela-mobile/latest.json');
  assert.equal(mobile.archiveUrl, '/tela-mobile/tela-mobile-demo.tela');
  assert.equal(w.win32ArtifactPath('dev'), '/repo/target/x86_64-pc-windows-gnu/debug/tela-win32-sdk.exe');
  assert.equal(w.win32DistPath(), '/repo/dist/win32/tela-win32-sdk.exe');
  assert.equal(w.macosAppDir(), '/repo/dist/macos/Tela.app');
  assert.equal(w.macosInfoPlistPath(), '/repo/dist/macos/Tela.app/Contents/Info.plist');
  assert.equal(w.macosExecutablePath(), '/repo/dist/macos/Tela.app/Contents/MacOS/tela-macos-sdk');
  assert.equal(w.macosInfoPlistSourcePath(), '/repo/crates/tela-macos-sdk/resources/Info.plist');
  assert.equal(w.macosArtifactPath('release'), '/repo/target/aarch64-apple-darwin/release/tela-macos-sdk');
  assert.equal(w.androidProjectDir(), '/repo/android');
  assert.equal(w.androidJniAbiDir(), '/repo/android/app/src/main/jniLibs/arm64-v8a');
  assert.equal(w.androidRustNativeLibraryPath(), '/repo/target/aarch64-linux-android/release/libmain.so');
  assert.equal(w.androidNativeLibraryPath(), '/repo/android/app/src/main/jniLibs/arm64-v8a/libmain.so');
  assert.equal(w.androidDebugApkPath(), '/repo/android/app/build/outputs/apk/debug/app-debug.apk');
  assert.equal(w.androidDistPath(), '/repo/dist/android/tela-mobile-debug.apk');
});
