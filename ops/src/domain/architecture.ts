// 领域层：依赖方向规则（把 scripts/check-architecture.sh 的规则模型化，纯函数可测）。
// 规则来源：002-架构总览与分层 §5 + 007-绘制与渲染后端 §7.1。
// 与旧 bash 脚本等价但更严格：用 cargo metadata 的真实依赖（含 feature/构建依赖），
// 不再用正则解析 Cargo.toml。

export type DepKind = 'normal' | 'dev' | 'build';

export interface CrateDependency {
  name: string;
  kind: DepKind;
}

export interface CrateInfo {
  name: string;
  deps: readonly CrateDependency[];
}

export interface ArchViolation {
  crate: string;
  message: string;
}

/** 零依赖 crate（含 dev/build 依赖都算）。 */
const ZERO_DEP_CRATES: readonly string[] = ['tela-contract', 'tela-log', 'tela-fonts'];

/** 允许的普通依赖白名单：<crate> 只允许依赖 <列出的包>。 */
const ALLOWED_NORMAL: readonly (readonly [string, readonly string[]])[] = [
  ['tela-resource-protocol', ['tela-contract']],
  ['tela-core', ['tela-contract']],
  ['tela-text', ['tela-contract', 'tela-fonts', 'ab_glyph']],
  ['tela-icon', ['tela-contract', 'tela-core', 'tela-fonts', 'tela-text']],
  ['tela-render-raster', ['tela-contract', 'tela-text', 'png', 'font8x8']],
  ['tela-render-canvas', ['tela-contract']],
  ['tela-render-wgpu', ['tela-contract', 'tela-log', 'tela-text', 'bytemuck', 'wgpu']],
  ['tela-guest-runtime', ['tela-app-abi', 'tela-bundle', 'tela-contract', 'serde_json', 'wasmtime']],
  ['tela-native-sdk-runtime', ['tela-bundle', 'tela-guest-runtime']],
  ['tela-mobile-demo', ['tela-app-abi', 'tela-contract', 'tela-core', 'tela-fonts', 'tela-icon', 'tela-text']],
  ['tela-android-sdk', [
    'jni', 'pollster', 'tela-app-abi', 'tela-contract', 'tela-guest-runtime', 'tela-log',
    'tela-render-wgpu', 'ureq', 'wgpu', 'winit',
  ]],
  ['tela-webview-sdk', [
    'tela-app-abi', 'tela-bundle', 'tela-contract', 'tela-render-wgpu',
    'serde_json', 'wasm-bindgen', 'wasm-bindgen-futures', 'web-sys', 'wgpu',
  ]],
  ['tela-widgets', ['tela-contract', 'tela-core', 'tela-fonts']],
  ['tela-ui', ['tela-contract', 'tela-core', 'tela-fonts', 'tela-icon', 'tela-text', 'tela-widgets']],
];

/** dev-dependencies 白名单：core 的 dev 依赖仅限测试专用后端（集成测试，不进入运行时）。 */
const ALLOWED_DEV: readonly (readonly [string, readonly string[]])[] = [
  ['tela-core', ['tela-render-raster']],
  ['tela-native-sdk-runtime', ['serde_json', 'tela-app-abi']],
];

/** 校验依赖方向，返回违规列表（空 = 通过）。 */
export function checkArchitecture(crates: readonly CrateInfo[]): ArchViolation[] {
  const violations: ArchViolation[] = [];
  const byName = new Map(crates.map((c) => [c.name, c]));

  // 1. 零依赖 crate。
  for (const name of ZERO_DEP_CRATES) {
    const info = byName.get(name);
    if (!info) {
      violations.push({ crate: name, message: '缺少 crate 定义（cargo metadata 未包含）' });
      continue;
    }
    if (info.deps.length > 0) {
      violations.push({
        crate: name,
        message: `必须零依赖，实际依赖: ${info.deps.map((d) => d.name).join(', ')}`,
      });
    }
  }

  // 2. 白名单依赖（normal 与 dev 分开校验）。
  const checkList = (list: readonly (readonly [string, readonly string[]])[], kind: DepKind) => {
    for (const [crate, allowed] of list) {
      const info = byName.get(crate);
      if (!info) continue;
      for (const dep of info.deps.filter((d) => d.kind === kind)) {
        if (!allowed.includes(dep.name)) {
          violations.push({
            crate,
            message: `[${kind}] 依赖了未允许的 crate: ${dep.name}（允许: ${allowed.join(' ')}）`,
          });
        }
      }
    }
  };
  checkList(ALLOWED_NORMAL, 'normal');
  checkList(ALLOWED_DEV, 'dev');

  // 3. render 后端禁止反向依赖 tela-core（含 dev/build）。
  for (const info of crates) {
    if (info.name.startsWith('tela-render-') && info.deps.some((d) => d.name === 'tela-core')) {
      violations.push({ crate: info.name, message: 'render 后端禁止反向依赖 tela-core' });
    }
  }

  // 4. 上层组件只能向 core/contract 方向依赖；不得通过组件层耦合渲染器、宿主或业务 demo。
  const ui = byName.get('tela-ui');
  if (ui && ui.deps.some((d) => d.name.startsWith('tela-render-') || d.name === 'tela-demo')) {
    violations.push({ crate: 'tela-ui', message: '分子组件层禁止依赖 renderer 或 tela-demo' });
  }

  // 5. 浏览器 WebView 壳只消费协议与 renderer；应用 guest 必须继续来自经过验证的
  // bundle，不能把 tela-demo 重新静态链接进壳。
  const webview = byName.get('tela-webview-sdk');
  if (webview && webview.deps.some((d) => d.name === 'tela-demo')) {
    violations.push({ crate: 'tela-webview-sdk', message: 'WebView SDK 必须通过 bundle 加载 guest，禁止依赖 tela-demo' });
  }

  // 6. Android is a target host, not a static application shell. Its selected mobile Guest stays
  // in a separate dynamic bundle so future game or TUI guests cannot leak into the host closure.
  const android = byName.get('tela-android-sdk');
  if (android && android.deps.some((d) => d.name === 'tela-mobile-demo' || d.name === 'tela-demo')) {
    violations.push({ crate: 'tela-android-sdk', message: 'Android SDK 必须通过 mobile bundle 加载 guest，禁止静态依赖业务应用' });
  }

  // 7. The neutral guest runtime has no GUI loop, surface, or platform SDK dependency.
  const guestRuntime = byName.get('tela-guest-runtime');
  if (guestRuntime && guestRuntime.deps.some((d) => d.name === 'tela-android-sdk' || d.name === 'tela-webview-sdk' || d.name === 'tela-win32-sdk' || d.name === 'tela-macos-sdk')) {
    violations.push({ crate: 'tela-guest-runtime', message: 'Guest Runtime 禁止依赖任一 Target SDK' });
  }

  return violations;
}
