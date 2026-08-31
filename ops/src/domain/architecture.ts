// 领域层：以 cargo metadata 的真实依赖校验 026 的产品分层和 030 的组件边界。
// 这里刻意约束职责，而不是旧目录名：Product 负责选择完整链路，Target 只承载本地平台，
// Kernel 与 UI Capability 不可反向认识 Presentation、Delivery 或 Target。

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

type AllowedDeps = Readonly<Record<string, readonly string[]>>;

/** Contract、纯日志 facade 与嵌入字体数据必须保持可独立复用。 */
const ZERO_DEP_CRATES = new Set([
  'tela-contract',
  'tela-log',
  'tela-font-resources',
]);

/**
 * 所有工作区 crate 的普通依赖闭包。外部库也列入其中，避免只校验内部 crate 时让
 * 新增的技术依赖悄悄跨越分层。由 Product 选择 application、resource provider 与 target。
 */
const ALLOWED_NORMAL: AllowedDeps = {
  'tela-app-abi': ['postcard', 'serde', 'tela-app-session', 'tela-contract'],
  'tela-cc-protocol': ['postcard', 'serde', 'serde_json'],
  'tela-cc-relay': ['httparse', 'serde', 'serde_json', 'tela-cc-protocol'],
  'tela-cc-agent': ['serde', 'serde_json', 'tela-cc-protocol'],
  'tela-cc-remote': [
    'serde_json',
    'tela-cc-protocol',
    'tela-contract',
    'tela-core',
    'tela-mobile-ui-kit',
    'tela-ui-dsl',
    'tela-ui-foundation',
  ],
  'tela-app-session': ['serde', 'tela-contract'],
  'tela-app-runtime': [
    'serde',
    'serde_json',
    'tela-app-session',
    'tela-contract',
    'tela-core',
    'tela-ui-dsl',
  ],
  'tela-agent-demo': [
    'serde',
    'serde_json',
    'tela-app-runtime',
    'tela-contract',
    'tela-core',
    'tela-ui-dsl',
    'tela-ui-foundation',
  ],
  'tela-webview-probe': [
    'tela-app-runtime',
    'tela-contract',
    'tela-core',
    'tela-ui-dsl',
  ],
  'tela-bridge': ['postcard', 'serde', 'tela-utils'],
  'tela-bundle': ['hex', 'serde', 'serde_json', 'sha2', 'tela-app-abi', 'zip'],
  'tela-core': ['tela-contract'],
  'tela-desktop-demo': [
    'tela-app-runtime',
    'tela-app-session',
    'tela-contract',
    'tela-core',
    'tela-desktop-ui-dsl',
    'tela-desktop-ui-kit',
    'tela-ui-dsl',
    'tela-ui-foundation',
  ],
  'tela-desktop-runtime': ['tela-bridge', 'tela-bundle', 'tela-guest-runtime', 'tela-utils'],
  'tela-desktop-ui-kit': [
    'tela-contract',
    'tela-core',
    'tela-ui-foundation',
  ],
  'tela-desktop-ui-dsl': [
    'tela-contract',
    'tela-desktop-ui-kit',
    'tela-ui-dsl',
    'tela-ui-foundation',
  ],
  'tela-guest-runtime': [
    'serde_json',
    'tela-app-abi',
    'tela-bridge',
    'tela-bundle',
    'tela-contract',
    'wasmtime',
  ],
  'tela-icon-resources': ['tela-contract', 'tela-text-resources'],
  'tela-mobile-demo': [
    'tela-contract',
    'tela-core',
    'tela-mobile-ui-kit',
    'tela-ui-dsl',
    'tela-ui-foundation',
  ],
  'tela-mobile-ui-kit': [
    'tela-contract',
    'tela-core',
    'tela-ui-foundation',
  ],
  'tela-product-desktop-guest': [
    'tela-app-abi',
    'tela-app-runtime',
    'tela-app-session',
    'tela-bridge',
    'tela-contract',
    'tela-desktop-demo',
    'tela-icon-resources',
    'tela-text-resources',
  ],
  'tela-product-agent-demo': [
    'tela-agent-demo',
    'tela-app-abi',
    'tela-contract',
    'tela-icon-resources',
    'tela-target-webview',
    'tela-text-resources',
    'wasm-bindgen',
  ],
  'tela-product-webview-probe': [
    'tela-app-abi',
    'tela-contract',
    'tela-icon-resources',
    'tela-target-webview',
    'tela-text-resources',
    'tela-webview-probe',
    'wasm-bindgen',
  ],
  'tela-product-ios': [
    'tela-contract',
    'tela-icon-resources',
    'tela-mobile-demo',
    'tela-target-ios',
    'tela-text-resources',
  ],
  'tela-product-win32-editor': [
    'tela-app-runtime',
    'tela-contract',
    'tela-desktop-runtime',
    'tela-icon-resources',
    'tela-utils',
    'tela-target-win32',
    'tela-text-resources',
    'tela-win32-editor',
  ],
  'tela-product-win32-probe': [
    'tela-app-runtime',
    'tela-contract',
    'tela-icon-resources',
    'tela-target-win32',
    'tela-text-resources',
    'tela-win32-probe',
  ],
  'tela-product-win32-agent': [
    'tela-agent-demo',
    'tela-contract',
    'tela-icon-resources',
    'tela-target-win32',
    'tela-text-resources',
  ],
  'tela-product-speed-gear': [
    'tela-app-runtime',
    'tela-contract',
    'tela-icon-resources',
    'tela-speed-gear',
    'tela-target-win32',
    'tela-text-resources',
  ],
  'tela-product-mobile-guest': [
    'tela-app-abi',
    'tela-contract',
    'tela-icon-resources',
    'tela-mobile-demo',
    'tela-text-resources',
  ],
  'tela-product-cc-guest': [
    'tela-app-abi',
    'tela-bridge',
    'tela-cc-protocol',
    'tela-cc-remote',
    'tela-contract',
    'tela-icon-resources',
    'tela-text-resources',
  ],
  'tela-render-canvas': ['tela-contract'],
  'tela-render-raster': ['font8x8', 'png', 'tela-contract', 'tela-text-resources'],
  'tela-render-wgpu': [
    'bytemuck',
    'tela-contract',
    'tela-log',
    'tela-text-resources',
    'wgpu',
  ],
  'tela-resource-protocol': ['tela-contract'],
  'tela-target-android': [
    'jni',
    'libc',
    'ndk-sys',
    'pollster',
    'serde_json',
    'tela-app-abi',
    'tela-bridge',
    'tela-cc-protocol',
    'tela-contract',
    'tela-guest-runtime',
    'tela-log',
    'tela-render-wgpu',
    'tela-utils',
    'ureq',
    'wgpu',
    'winit',
  ],
  'tela-target-ios': [
    'objc2',
    'objc2-foundation',
    'objc2-quartz-core',
    'pollster',
    'raw-window-handle',
    'tela-contract',
    'tela-render-wgpu',
    'wgpu',
    'winit',
  ],
  'tela-target-macos': [
    'objc2',
    'objc2-app-kit',
    'objc2-foundation',
    'objc2-quartz-core',
    'pollster',
    'raw-window-handle',
    'tela-app-abi',
    'tela-bridge',
    'tela-contract',
    'tela-desktop-runtime',
    'tela-render-wgpu',
    'ureq',
    'wgpu',
  ],
  'tela-win32-editor': [
    'tela-app-runtime',
    'tela-app-session',
    'tela-bridge',
    'tela-contract',
    'tela-core',
    'tela-icon-resources',
    'tela-ui-dsl',
    'tela-ui-foundation',
  ],
  'tela-win32-probe': [
    'tela-app-runtime',
    'tela-contract',
    'tela-ui-dsl',
    'tela-ui-foundation',
  ],
  'tela-speed-gear': [
    'tela-app-runtime',
    'tela-app-session',
    'tela-contract',
    'tela-core',
    'tela-desktop-ui-kit',
    'tela-desktop-ui-dsl',
    'tela-speed-gear-protocol',
    'tela-ui-dsl',
    'tela-ui-foundation',
    'windows',
  ],
  'tela-target-webview': [
    'serde_json',
    'tela-app-abi',
    'tela-bundle',
    'tela-contract',
    'tela-render-wgpu',
    'wasm-bindgen',
    'wasm-bindgen-futures',
    'web-sys',
    'wgpu',
  ],
  'tela-utils': ['serde', 'serde_json'],
  'tela-target-win32': [
    'pollster',
    'raw-window-handle',
    'tela-app-abi',
    'tela-app-session',
    'tela-bridge',
    'tela-contract',
    'tela-desktop-runtime',
    'tela-render-wgpu',
    'ureq',
    'wgpu',
    'windows',
  ],
  'tela-text-resources': ['ab_glyph', 'tela-contract', 'tela-font-resources'],
  'tela-ui-dsl': ['tela-contract', 'tela-core', 'tela-ui-dsl-macros'],
  'tela-ui-dsl-macros': ['proc-macro-crate', 'proc-macro2', 'quote', 'syn'],
  'tela-ui-foundation': ['tela-contract', 'tela-core'],
  'tela-speed-gear-protocol': [],
  'tela-speed-gear-hook': ['tela-speed-gear-protocol', 'windows'],
};

/** 单元/像素测试可在边界外读取下层实现，但不能扩大生产依赖闭包。 */
const ALLOWED_DEV: AllowedDeps = {
  'tela-agent-demo': [
    'tela-app-session',
    'tela-icon-resources',
    'tela-text-resources',
  ],
  'tela-webview-probe': ['tela-app-session', 'tela-icon-resources', 'tela-text-resources'],
  'tela-desktop-demo': ['tela-icon-resources', 'tela-render-raster', 'tela-text-resources'],
  'tela-desktop-runtime': ['serde_json', 'tela-app-abi'],
  'tela-win32-editor': ['tela-icon-resources', 'tela-render-raster', 'tela-text-resources'],
  'tela-win32-probe': ['tela-app-session', 'tela-icon-resources', 'tela-text-resources'],
  'tela-product-win32-editor': [
    'pollster',
    'tela-app-session',
    'tela-bridge',
    'tela-render-wgpu',
    'wgpu',
  ],
  'tela-mobile-demo': [
    'tela-icon-resources',
    'tela-render-raster',
    'tela-text-resources',
  ],
  'tela-cc-remote': ['tela-icon-resources', 'tela-render-raster', 'tela-text-resources'],
  'tela-render-raster': ['tela-core'],
  'tela-render-wgpu': ['naga', 'pollster'],
  'tela-ui-dsl': ['trybuild'],
  'tela-speed-gear': ['tela-icon-resources', 'tela-text-resources'],
};

const TARGET_CRATES = new Set([
  'tela-target-android',
  'tela-target-ios',
  'tela-target-macos',
  'tela-target-webview',
  'tela-target-win32',
]);

const APPLICATION_CRATES = new Set([
  'tela-agent-demo',
  'tela-cc-remote',
  'tela-desktop-demo',
  'tela-mobile-demo',
  'tela-speed-gear',
  'tela-webview-probe',
  'tela-win32-probe',
]);
const PRODUCT_CRATES = new Set([
  'tela-product-agent-demo',
  'tela-product-cc-guest',
  'tela-product-desktop-guest',
  'tela-product-ios',
  'tela-product-mobile-guest',
  'tela-product-speed-gear',
  'tela-product-webview-probe',
  'tela-product-win32-agent',
  'tela-product-win32-probe',
]);
const DELIVERY_CRATES = new Set([
  'tela-app-abi',
  'tela-app-session',
  'tela-bundle',
  'tela-desktop-runtime',
  'tela-guest-runtime',
]);
const RENDERER_CRATES = new Set([
  'tela-render-canvas',
  'tela-render-raster',
  'tela-render-wgpu',
]);
const PRESENTATION_CRATES = new Set([
  'tela-font-resources',
  'tela-icon-resources',
  'tela-resource-protocol',
  'tela-text-resources',
  ...RENDERER_CRATES,
]);
const UI_CRATES = new Set([
  'tela-ui-foundation',
  'tela-desktop-ui-kit',
  'tela-mobile-ui-kit',
]);
const COMPOSITION_CRATES = new Set(['tela-ui-dsl', 'tela-ui-dsl-macros']);

const MANAGED_CRATES = new Set([
  ...ZERO_DEP_CRATES,
  ...Object.keys(ALLOWED_NORMAL),
]);

function formatAllowed(allowed: readonly string[]): string {
  return allowed.length === 0 ? '无' : allowed.join(' ');
}

function dependenciesOf(info: CrateInfo, kind: DepKind): readonly string[] {
  return info.deps.filter((dep) => dep.kind === kind).map((dep) => dep.name);
}

function reportForbiddenDependencies(
  violations: ArchViolation[],
  info: CrateInfo,
  forbidden: ReadonlySet<string>,
  message: string,
): void {
  const found = info.deps.filter((dep) => forbidden.has(dep.name));
  if (found.length > 0) {
    violations.push({
      crate: info.name,
      message: `${message}: ${found.map((dep) => dep.name).join(', ')}`,
    });
  }
}

/** 校验 026 产品架构与 030 UI 组件边界，返回违规列表（空 = 通过）。 */
export function checkArchitecture(crates: readonly CrateInfo[]): ArchViolation[] {
  const violations: ArchViolation[] = [];

  for (const info of crates) {
    if (!MANAGED_CRATES.has(info.name)) {
      violations.push({ crate: info.name, message: '未在 026/030 架构依赖表中登记的 crate' });
      continue;
    }

    if (ZERO_DEP_CRATES.has(info.name)) {
      if (info.deps.length > 0) {
        violations.push({
          crate: info.name,
          message: `必须零依赖，实际依赖: ${info.deps.map((dep) => dep.name).join(', ')}`,
        });
      }
      continue;
    }

    const allowedNormal = ALLOWED_NORMAL[info.name] ?? [];
    const allowedDev = ALLOWED_DEV[info.name] ?? [];
    const allowedByKind: Readonly<Record<DepKind, readonly string[]>> = {
      normal: allowedNormal,
      dev: allowedDev,
      build: [],
    };

    for (const kind of ['normal', 'dev', 'build'] as const) {
      for (const dep of dependenciesOf(info, kind)) {
        const allowed = allowedByKind[kind];
        if (!allowed.includes(dep)) {
          violations.push({
            crate: info.name,
            message: `[${kind}] 依赖了未允许的 crate: ${dep}（允许: ${formatAllowed(allowed)}）`,
          });
        }
      }
    }
  }

  for (const info of crates) {
    if (RENDERER_CRATES.has(info.name)) {
      const productionCore = info.deps.some(
        (dep) => dep.name === 'tela-core' && dep.kind !== 'dev',
      );
      if (productionCore) {
        violations.push({ crate: info.name, message: 'Renderer 生产闭包禁止反向依赖 tela-core' });
      }
    }

    if (UI_CRATES.has(info.name)) {
      reportForbiddenDependencies(
        violations,
        info,
        new Set([
          ...COMPOSITION_CRATES,
          ...RENDERER_CRATES,
          ...DELIVERY_CRATES,
          ...TARGET_CRATES,
          ...APPLICATION_CRATES,
          ...PRODUCT_CRATES,
        ]),
        'UI Capability 禁止依赖 Composition、Renderer、Delivery、Target、Application 或 Product',
      );
    }

    if (COMPOSITION_CRATES.has(info.name)) {
      reportForbiddenDependencies(
        violations,
        info,
        new Set([
          ...UI_CRATES,
          ...PRESENTATION_CRATES,
          ...DELIVERY_CRATES,
          ...TARGET_CRATES,
          ...APPLICATION_CRATES,
          ...PRODUCT_CRATES,
        ]),
        'Application Composition runtime 禁止依赖 UI、Presentation、Delivery、Target、Application 或 Product',
      );
    }

    if (PRESENTATION_CRATES.has(info.name)) {
      reportForbiddenDependencies(
        violations,
        info,
        new Set([
          ...UI_CRATES,
          ...COMPOSITION_CRATES,
          ...APPLICATION_CRATES,
          ...DELIVERY_CRATES,
          ...TARGET_CRATES,
          ...PRODUCT_CRATES,
        ]),
        'Presentation 禁止依赖 UI、Composition、Application、Delivery、Target 或 Product',
      );
    }

    if (DELIVERY_CRATES.has(info.name)) {
      reportForbiddenDependencies(
        violations,
        info,
        new Set([
          ...TARGET_CRATES,
          ...COMPOSITION_CRATES,
          ...APPLICATION_CRATES,
          ...PRODUCT_CRATES,
        ]),
        'Delivery 禁止依赖 Composition、Target、Application 或 Product',
      );
    }

    if (TARGET_CRATES.has(info.name)) {
      reportForbiddenDependencies(
        violations,
        info,
        new Set([...COMPOSITION_CRATES, ...APPLICATION_CRATES, ...PRODUCT_CRATES]),
        'Target 只承载本地宿主，禁止静态依赖 Application 或 Product，也不得依赖 Composition',
      );
      const foreignTarget = info.deps.find(
        (dep) => TARGET_CRATES.has(dep.name) && dep.name !== info.name,
      );
      if (foreignTarget) {
        violations.push({ crate: info.name, message: `Target 禁止依赖其他 Target: ${foreignTarget.name}` });
      }
    }
  }

  const guestRuntime = crates.find((info) => info.name === 'tela-guest-runtime');
  if (guestRuntime) {
    reportForbiddenDependencies(
      violations,
      guestRuntime,
      TARGET_CRATES,
      'Guest Runtime 禁止依赖任一 Target',
    );
  }

  const desktopGuest = crates.find((info) => info.name === 'tela-product-desktop-guest');
  if (desktopGuest) {
    reportForbiddenDependencies(
      violations,
      desktopGuest,
      new Set([
        'tela-mobile-demo',
        ...TARGET_CRATES,
        'tela-bundle',
        'tela-desktop-runtime',
        'tela-guest-runtime',
      ]),
      '桌面动态 Product 只能装配桌面应用与资源，不得认识 Target 或 Delivery Runtime',
    );
  }

  const mobileGuest = crates.find((info) => info.name === 'tela-product-mobile-guest');
  if (mobileGuest) {
    reportForbiddenDependencies(
      violations,
      mobileGuest,
      new Set([
        'tela-desktop-demo',
        ...TARGET_CRATES,
        'tela-bundle',
        'tela-desktop-runtime',
        'tela-guest-runtime',
      ]),
      '移动动态 Product 只能装配移动应用与资源，不得认识 Target 或 Delivery Runtime',
    );
  }

  const iosProduct = crates.find((info) => info.name === 'tela-product-ios');
  if (iosProduct) {
    reportForbiddenDependencies(
      violations,
      iosProduct,
      new Set([
        'tela-desktop-demo',
        'tela-app-abi',
        'tela-bundle',
        'tela-desktop-runtime',
        'tela-guest-runtime',
        'tela-product-desktop-guest',
        'tela-product-mobile-guest',
      ]),
      'iOS 静态 Product 禁止进入动态 Delivery 链路或桌面应用',
    );
  }

  return violations;
}
