#!/usr/bin/env node
// tela-ops CLI 入口（接口层）：解析参数 → 组装依赖（依赖倒置）→ 分发到应用层用例。
// 运行时零第三方依赖：Node 24 原生执行 TS（type stripping，erasableSyntaxOnly）。
// 用法：
//   ops check                    四道验证门（fmt/clippy/test/arch）
//   ops build [demo|frontend|bundle|win32|macos|all] [--release] [--gpu]  构建发布物到 dist/
//   ops verify demo [--build]    冒烟测试（可先自动构建）
//   ops serve [port]             开发静态服务器（默认 8000）
import { parseArgs } from 'node:util';
import { fileURLToPath } from 'node:url';
import type { WorkspacePaths } from '../domain/workspace.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import type { Reporter } from '../domain/ports.ts';
import { TerminalReporter } from '../infrastructure/reporter.ts';
import { NodeProcessPort } from '../infrastructure/process.ts';
import { NodeFsPort } from '../infrastructure/fs.ts';
import { NodeWasmSmokePort } from '../infrastructure/wasm-smoke.ts';
import { CargoPort } from '../infrastructure/cargo.ts';
import { HttpServerPort } from '../infrastructure/server.ts';
import { runCheck } from '../application/check.ts';
import { runBuildDemo } from '../application/build-demo.ts';
import { runBuildFrontend } from '../application/build-frontend.ts';
import { runBuildBundle } from '../application/build-bundle.ts';
import { runBuildWin32 } from '../application/build-win32.ts';
import { runBuildMacos } from '../application/build-macos.ts';
import { runVerifyDemo } from '../application/verify-demo.ts';
import { runServe } from '../application/serve.ts';
import { runVerifyGpu } from '../application/verify-gpu.ts';
import { TelemetryStore } from '../infrastructure/telemetry-store.ts';

const USAGE = `tela-ops — tela 开发运维工作流（DDD 分层，运行时零依赖）

用法:
  ops check                   四道验证门（fmt / clippy / test / arch）
  ops build [demo|frontend|bundle|win32|macos|all] [--release] [--gpu]
                              构建发布物到 dist/；all 会先重建目录（--gpu：WebGPU 后端
                              + wasm-bindgen glue，强制 release）
  ops verify [demo|gpu] [--build] [--port N]
                              验证（默认 demo）：demo = 冒烟测试（--build 先构建）；
                              gpu = 原生 JS WebGPU 环境自检（不经 wgpu，离屏三角形
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
      gpu: { type: 'boolean', default: false },
      port: { type: 'string', default: '' },
    },
  });

  if (values.help) {
    console.log(USAGE);
    return 0;
  }

  const [command, targetArg] = positionals;
  let target = targetArg;
  switch (command) {
    case 'check': {
      const cargo = new CargoPort(new NodeProcessPort(), workspace);
      const result = await runCheck({ cargo, reporter });
      return result.passed ? 0 : 1;
    }
    case 'build': {
      const targets = target === 'all' ? ['demo', 'frontend', 'bundle'] : [target ?? 'demo'];
      const processPort = new NodeProcessPort();
      const fs = new NodeFsPort();
      if (target === 'all') {
        reporter.section('准备发布目录（dist/）');
        await fs.resetDir(workspace.distDir);
        reporter.ok(`已重建 ${workspace.distDir}`);
      }
      for (const t of targets) {
        if (t === 'demo') {
          // `all --gpu` 必须留下可直接切换的 CPU 与 GPU 产物；只构建
          // `demo --gpu` 时仍保持原有的 GPU-only 行为。
          const modes = target === 'all' && values.gpu ? [false, true] : [values.gpu];
          for (const gpu of modes) {
            const cargo = new CargoPort(new NodeProcessPort(), workspace);
            const result = await runBuildDemo(
              { cargo, process: processPort, fs, reporter, workspace },
              {
                profile: values.release ? 'release' : 'dev',
                gpu,
              },
            );
            if (!result.ok) return 1;
          }
        } else if (t === 'frontend') {
          const result = await runBuildFrontend({ process: processPort, reporter, workspace });
          if (!result.ok) return 1;
        } else if (t === 'bundle') {
          const cargo = new CargoPort(processPort, workspace);
          const result = await runBuildBundle({ cargo, process: processPort, fs, reporter, workspace });
          if (!result.ok) return 1;
        } else if (t === 'win32') {
          const cargo = new CargoPort(processPort, workspace);
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
          const result = await runBuildMacos(
            { cargo, fs, reporter, workspace },
            values.release ? 'release' : 'dev',
          );
          if (!result.ok) return 1;
        } else {
          reporter.fail(`未知构建目标: ${t}（demo | frontend | bundle | win32 | macos | all）`);
          return 1;
        }
      }
      return 0;
    }
    case 'verify': {
      if (!target) target = 'demo';  // 默认 demo（收敛后 verify 不带 target = 冒烟）
      if (target === 'gpu') {
        const port = values.port ? Number(values.port) : 8200;
        const telemetry = new TelemetryStore();
        const ok = await runVerifyGpu(
          { process: new NodeProcessPort(), server: new HttpServerPort(), telemetry, reporter, workspace },
          { preferredPort: port, timeoutMs: 45_000 },
        );
        return ok ? 0 : 1;
      }
      if (target === 'demo') {
        const process = new NodeProcessPort();
        const fs = new NodeFsPort();
        if (values.build) {
          reporter.info('--build 已指定，先构建 CPU wasm…');
          const cargo = new CargoPort(process, workspace);
          const build = await runBuildDemo(
            { cargo, process, fs, reporter, workspace },
            { profile: values.release ? 'release' : 'dev', gpu: false },
          );
          if (!build.ok) return 1;
        }
        const vresult = await runVerifyDemo(
          { fs, smoke: new NodeWasmSmokePort(), reporter, workspace },
        );
        return vresult.ok ? 0 : 1;
      }
      reporter.fail(`未知验证目标: ${target ?? '(空)'}（demo | gpu）`);
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
      if (!result) return 1;
      // 常驻：SIGINT/SIGTERM 优雅关闭。
      let shuttingDown = false;
      const shutdown = async (): Promise<void> => {
        if (shuttingDown) return;
        shuttingDown = true;
        await result.close();
        process.exit(0);
      };
      process.on('SIGINT', () => void shutdown());
      process.on('SIGTERM', () => void shutdown());
      return 0; // 常驻进程，实际由信号退出
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

/** 未知命令的模糊提示（编辑距离 ≤ 2 的已知命令，防笔误如 obs → ops）。 */
function suggestCommand(input: string): string | undefined {
  const known = ['check', 'build', 'verify', 'serve', 'help'];
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
