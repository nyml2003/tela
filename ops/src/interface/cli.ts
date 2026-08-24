#!/usr/bin/env node
// tela-ops CLI 入口（接口层）：解析参数 → 组装依赖（依赖倒置）→ 分发到应用层用例。
// 运行时零第三方依赖：Node 24 原生执行 TS（type stripping，erasableSyntaxOnly）。
// 用法：
//   ops check                    五道验证门（fmt/clippy/test/WGPU visual/arch）
//   ops build <core|webview|frontend|bundle|android|ios|win32|win32-editor|speed-gear|macos> [--release]  构建显式产品闭包
//   ops verify bundle [desktop|mobile] [--build]  验证已发布的应用 guest
//   ops serve [port]             开发静态服务器（默认 8000）
import { parseArgs } from 'node:util';
import { fileURLToPath } from 'node:url';
import type { BundleChannel, WorkspacePaths } from '../domain/workspace.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import type { Reporter, ServeResult } from '../domain/ports.ts';
import { TerminalReporter } from '../infrastructure/reporter.ts';
import { NodeProcessPort } from '../infrastructure/process.ts';
import { NodeFsPort } from '../infrastructure/fs.ts';
import { CargoPort } from '../infrastructure/cargo.ts';
import { HttpServerPort } from '../infrastructure/server.ts';
import { runCheck } from '../application/check.ts';
import { runBuildWebview } from '../application/build-webview.ts';
import { runBuildCore } from '../application/build-core.ts';
import { runBuildFrontend } from '../application/build-frontend.ts';
import { runBuildBundle } from '../application/build-bundle.ts';
import { runBuildWin32 } from '../application/build-win32.ts';
import { runBuildWin32Editor } from '../application/build-win32-editor.ts';
import { runBuildSpeedGear } from '../application/build-speed-gear.ts';
import { runBuildMacos } from '../application/build-macos.ts';
import { runBuildAndroid } from '../application/build-android.ts';
import { runBuildIos } from '../application/build-ios.ts';
import { runVerifyBundle } from '../application/verify-bundle.ts';
import { runServe } from '../application/serve.ts';
import { runAndroidServe } from '../application/serve-android.ts';
import { runDeployAndroid } from '../application/deploy-android.ts';
import { runDeployIos } from '../application/deploy-ios.ts';
import { runVerifyGpu } from '../application/verify-gpu.ts';
import { TelemetryStore } from '../infrastructure/telemetry-store.ts';
import { WindowsAdbPort } from '../infrastructure/windows-adb.ts';

const USAGE = `tela-ops — tela 开发运维工作流（DDD 分层，运行时零依赖）

用法:
  ops check                   五道验证门（fmt / clippy / test / WGPU visual / arch）
  ops build <core|webview|frontend|bundle [desktop|mobile]|android|ios|win32|win32-editor|speed-gear|macos> [--release]
                              每次显式选择一个产品或其受控子产物。bundle desktop/mobile 是
                              两个独立 product guest；webview/win32/macos 先构建 desktop guest，android 先构建 mobile guest，
                              ios 静态链接独立 mobile app，构建无签名 iPhone ARM64 UIKit/Metal App。
  ops android serve           仅监听 127.0.0.1:8000，供 USB adb reverse 的 Android mobile bundle 使用
  ops android deploy [--serial SERIAL]
                              调 Windows adb.exe 建立 reverse、安装并启动 ARM64 debug APK
  ops ios deploy --device UDID
                              使用 Xcode 配置的 Apple Development 签名，安装并启动 iPhone App
  ops verify [bundle [desktop|mobile]|gpu] [--build] [--port N]
                              验证（默认 bundle）：bundle = archive/ABI/guest 首帧校验
                              （--build 先生成 bundle）；gpu = 原生 JS WebGPU 环境自检（不经 wgpu，离屏三角形
                              回读上报，判定环境 vs wgpu 层问题）
  ops serve [port]            开发静态服务器（默认 8000）
  ops help                    显示本帮助`;

interface CliDeps {
  workspace: WorkspacePaths;
  reporter: Reporter;
}

/** 组装基础设施（依赖倒置根）：所有用例只依赖端口接口。 */
function bootstrap(root: string): CliDeps {
  const workspace = resolveWorkspace(root);
  const reporter = new TerminalReporter();
  return { workspace, reporter };
}

async function main(): Promise<number> {
  // 仓库根 = ops/src/interface/ 的上三级（兼容从任意 cwd 调用）。
  const root = fileURLToPath(new URL('../../../', import.meta.url)).replace(/\/$/, '');
  const { workspace, reporter } = bootstrap(root);
  const { positionals, values } = parseArgs({
    args: process.argv.slice(2),
    allowPositionals: true,
    options: {
      help: { type: 'boolean', short: 'h', default: false },
      release: { type: 'boolean', default: false },
      build: { type: 'boolean', default: false },
      port: { type: 'string', default: '' },
      serial: { type: 'string', default: '' },
      device: { type: 'string', default: '' },
      // 保留解析仅为对旧命令给出迁移提示，Android 构建不再接受可变网络 URL。
      'bundle-index': { type: 'string', default: '' },
    },
  });

  if (values.help) {
    console.log(USAGE);
    return 0;
  }

  const [command, targetArg, variantArg] = positionals;
  let target = targetArg;
  switch (command) {
    case 'check': {
      const cargo = new CargoPort(new NodeProcessPort(), workspace);
      const result = await runCheck({ cargo, reporter });
      return result.passed ? 0 : 1;
    }
    case 'build': {
      if (target === undefined || target === 'all') {
        reporter.fail('ops build 需要显式产品目标（core | webview | android | ios | win32 | win32-editor | speed-gear | macos）。');
        reporter.info('浏览器产品使用 ops build webview；交付 guest 可单独使用 ops build bundle [desktop|mobile]。');
        return 1;
      }
      const targets = [target];
      if (variantArg && target !== 'bundle') {
        reporter.fail(`构建目标 ${target ?? 'all'} 不接受额外参数: ${variantArg}`);
        return 1;
      }
      const processPort = new NodeProcessPort();
      const fs = new NodeFsPort();
      for (const t of targets) {
        if (t === 'core') {
          const cargo = new CargoPort(processPort, workspace);
          const result = await runBuildCore({ cargo, reporter, workspace });
          if (!result.ok) return 1;
        } else if (t === 'webview') {
          const cargo = new CargoPort(new NodeProcessPort(), workspace);
          const bundle = await runBuildBundle(
            { cargo, process: processPort, fs, reporter, workspace },
            'desktop',
          );
          if (!bundle.ok) return 1;
          const result = await runBuildWebview(
            { cargo, process: processPort, fs, reporter, workspace },
          );
          if (!result.ok) return 1;
          const frontend = await runBuildFrontend({ process: processPort, reporter, workspace });
          if (!frontend.ok) return 1;
        } else if (t === 'frontend') {
          const result = await runBuildFrontend({ process: processPort, reporter, workspace });
          if (!result.ok) return 1;
        } else if (t === 'bundle') {
          const channel = resolveBundleChannel(variantArg, reporter);
          if (!channel) return 1;
          const cargo = new CargoPort(processPort, workspace);
          const result = await runBuildBundle({ cargo, process: processPort, fs, reporter, workspace }, channel);
          if (!result.ok) return 1;
        } else if (t === 'android') {
          if (values['bundle-index']) {
            reporter.fail('Android 构建固定使用 http://127.0.0.1:8000/tela-mobile/latest.json；请移除 --bundle-index。');
            return 1;
          }
          if (values.serial) {
            reporter.fail('--serial 只适用于 ops android deploy。');
            return 1;
          }
          const cargo = new CargoPort(processPort, workspace);
          const result = await runBuildAndroid(
            { cargo, process: processPort, fs, reporter, workspace },
          );
          if (!result.ok) return 1;
        } else if (t === 'ios') {
          if (values['bundle-index']) {
            reporter.fail('iOS 静态链接 mobile app；不接受 --bundle-index。');
            return 1;
          }
          if (values.serial || values.device) {
            reporter.fail('--serial 与 --device 只适用于各自的 deploy 子命令。');
            return 1;
          }
          if (process.platform !== 'darwin' || process.arch !== 'arm64') {
            reporter.fail('ios 目标必须在 Apple Silicon macOS 上构建（需要完整 Xcode 的 iPhoneOS SDK）。');
            reporter.info('当前机器可继续验证 Rust 逻辑；在 Mac 上运行 nix develop .#ios --command ops build ios。');
            return 1;
          }
          const cargo = new CargoPort(processPort, workspace);
          const result = await runBuildIos(
            { cargo, process: processPort, fs, reporter, workspace },
            values.release ? 'release' : 'dev',
          );
          if (!result.ok) return 1;
        } else if (t === 'win32-editor') {
          const cargo = new CargoPort(processPort, workspace);
          const result = await runBuildWin32Editor(
            { cargo, fs, reporter, workspace },
            values.release ? 'release' : 'dev',
          );
          if (!result.ok) return 1;
        } else if (t === 'speed-gear') {
          const cargo = new CargoPort(processPort, workspace);
          const result = await runBuildSpeedGear(
            { cargo, fs, reporter, workspace },
            values.release ? 'release' : 'dev',
          );
          if (!result.ok) return 1;
        } else if (t === 'win32') {
          const cargo = new CargoPort(processPort, workspace);
          const bundle = await runBuildBundle(
            { cargo, process: processPort, fs, reporter, workspace },
            'desktop',
          );
          if (!bundle.ok) return 1;
          const result = await runBuildWin32(
            { cargo, fs, reporter, workspace },
            values.release ? 'release' : 'dev',
          );
          if (!result.ok) return 1;
        } else if (t === 'macos') {
          if (process.platform !== 'darwin' || process.arch !== 'arm64') {
            reporter.fail('macos 目标必须在 Apple Silicon macOS 上构建（需要本机 Apple SDK）。');
            reporter.info('当前机器仍可执行 ops build bundle，并在 Mac 上运行 ops build macos。');
            return 1;
          }
          const cargo = new CargoPort(processPort, workspace);
          const bundle = await runBuildBundle(
            { cargo, process: processPort, fs, reporter, workspace },
            'desktop',
          );
          if (!bundle.ok) return 1;
          const result = await runBuildMacos(
            { cargo, fs, reporter, workspace },
            values.release ? 'release' : 'dev',
          );
          if (!result.ok) return 1;
        } else {
          reporter.fail(`未知构建目标: ${t}（core | webview | frontend | bundle | android | ios | win32 | win32-editor | speed-gear | macos）`);
          return 1;
        }
      }
      return 0;
    }
    case 'android': {
      if (variantArg) {
        reporter.fail(`Android 子命令 ${target ?? '(空)'} 不接受额外参数: ${variantArg}`);
        return 1;
      }
      if (values['bundle-index']) {
        reporter.fail('Android 真机开发固定使用 ADB reverse localhost；不接受 --bundle-index。');
        return 1;
      }
      if (target === 'serve') {
        if (values.serial) {
          reporter.fail('--serial 只适用于 ops android deploy。');
          return 1;
        }
        const result = await runAndroidServe(
          { fs: new NodeFsPort(), server: new HttpServerPort(), reporter, workspace },
        );
        return result ? serveUntilStopped(result) : 1;
      }
      if (target === 'deploy') {
        const result = await runDeployAndroid(
          {
            adb: new WindowsAdbPort(new NodeProcessPort(), new NodeFsPort()),
            fs: new NodeFsPort(),
            reporter,
            workspace,
          },
          values.serial ? { serial: values.serial } : {},
        );
        return result ? 0 : 1;
      }
      reporter.fail(`未知 Android 子命令: ${target ?? '(空)'}（serve | deploy）`);
      return 1;
    }
    case 'ios': {
      if (variantArg) {
        reporter.fail(`iOS 子命令 ${target ?? '(空)'} 不接受额外参数: ${variantArg}`);
        return 1;
      }
      if (values['bundle-index'] || values.serial || values.release) {
        reporter.fail('ops ios deploy 不接受 --bundle-index、--serial 或 --release。');
        return 1;
      }
      if (target !== 'deploy') {
        reporter.fail(`未知 iOS 子命令: ${target ?? '(空)'}（deploy）`);
        return 1;
      }
      if (!values.device) {
        reporter.fail('iOS 部署需要 --device <UDID>。');
        return 1;
      }
      if (process.platform !== 'darwin' || process.arch !== 'arm64') {
        reporter.fail('iOS 真机部署必须在 Apple Silicon macOS 上执行。');
        return 1;
      }
      const result = await runDeployIos(
        { process: new NodeProcessPort(), fs: new NodeFsPort(), reporter, workspace },
        { deviceId: values.device },
      );
      return result ? 0 : 1;
    }
    case 'verify': {
      if (!target) target = 'bundle';
      if (target === 'gpu') {
        const port = values.port ? Number(values.port) : 8200;
        const telemetry = new TelemetryStore();
        const ok = await runVerifyGpu(
          { process: new NodeProcessPort(), server: new HttpServerPort(), telemetry, reporter, workspace },
          { preferredPort: port, timeoutMs: 45_000 },
        );
        return ok ? 0 : 1;
      }
      if (target === 'bundle') {
        const channel = resolveBundleChannel(variantArg, reporter);
        if (!channel) return 1;
        const process = new NodeProcessPort();
        const fs = new NodeFsPort();
        if (values.build) {
          reporter.info('--build 已指定，先构建应用 bundle…');
          const cargo = new CargoPort(process, workspace);
          const build = await runBuildBundle(
            { cargo, process, fs, reporter, workspace },
            channel,
          );
          if (!build.ok) return 1;
        }
        const vresult = await runVerifyBundle({ fs, process, reporter, workspace }, channel);
        return vresult.ok ? 0 : 1;
      }
      reporter.fail(`未知验证目标: ${target ?? '(空)'}（bundle | gpu）`);
      return 1;
    }
    case 'serve': {
      const port = target ? Number(target) : 8000;
      if (!Number.isInteger(port) || port <= 0 || port > 65535) {
        reporter.fail(`非法端口: ${target}`);
        return 1;
      }
      const result = await runServe(
        { fs: new NodeFsPort(), server: new HttpServerPort(), reporter, workspace },
        port,
      );
      return result ? serveUntilStopped(result) : 1;
    }
    case 'help':
    case undefined: {
      console.log(USAGE);
      return 0;
    }
    default:
      console.log(USAGE);
      reporter.fail(`未知命令: ${command}${suggestCommand(command) ? `——是否想输入 ${suggestCommand(command)}？` : ''}`);
      return 1;
  }
}

/** serve 与 android serve 都是常驻进程，统一处理终止信号以关闭底层 HTTP socket。 */
function serveUntilStopped(result: ServeResult): number {
  let shuttingDown = false;
  const shutdown = async (): Promise<void> => {
    if (shuttingDown) return;
    shuttingDown = true;
    await result.close();
    process.exit(0);
  };
  process.on('SIGINT', () => void shutdown());
  process.on('SIGTERM', () => void shutdown());
  return 0;
}

function resolveBundleChannel(value: string | undefined, reporter: Reporter): BundleChannel | undefined {
  if (value === undefined || value === 'desktop') return 'desktop';
  if (value === 'mobile') return 'mobile';
  reporter.fail(`未知 bundle channel: ${value}（desktop | mobile）`);
  return undefined;
}

/** 未知命令的模糊提示（编辑距离 ≤ 2 的已知命令，防笔误如 obs → ops）。 */
function suggestCommand(input: string): string | undefined {
  const known = ['check', 'build', 'verify', 'serve', 'android', 'ios', 'help'];
  const distance = (a: string, b: string): number => {
    const m = a.length;
    const n = b.length;
    const dp: number[][] = Array.from({ length: m + 1 }, () => Array(n + 1).fill(0));
    for (let i = 0; i <= m; i++) dp[i]![0] = i;
    for (let j = 0; j <= n; j++) dp[0]![j] = j;
    for (let i = 1; i <= m; i++) {
      for (let j = 1; j <= n; j++) {
        dp[i]![j] = Math.min(
          dp[i - 1]![j]! + 1,
          dp[i]![j - 1]! + 1,
          dp[i - 1]![j - 1]! + (a[i - 1] === b[j - 1] ? 0 : 1),
        );
      }
    }
    return dp[m]![n]!;
  };
  let best: string | undefined;
  let bestDist = 2;
  for (const cmd of known) {
    const d = distance(input, cmd);
    if (d <= bestDist) {
      bestDist = d;
      best = cmd;
    }
  }
  return best;
}

main().then(
  (code) => {
    process.exitCode = code;
  },
  (err: unknown) => {
    console.error('tela-ops 异常:', err);
    process.exitCode = 1;
  },
);
