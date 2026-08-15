// 基础设施层：cargo 命令封装（fmt/clippy/test/build/metadata）。
import type { ProcessPort, ProcessResult } from '../domain/ports.ts';
import type { GateResult } from '../domain/gates.ts';
import type { BuildProfile, WorkspacePaths } from '../domain/workspace.ts';
import type { CrateInfo } from '../domain/architecture.ts';

/** cargo 相关命令的领域服务：参数拼装与结果规范化都在这里。 */
export class CargoPort {
  private readonly process: ProcessPort;
  private readonly workspace: WorkspacePaths;

  constructor(process: ProcessPort, workspace: WorkspacePaths) {
    this.process = process;
    this.workspace = workspace;
  }

  async fmtCheck(): Promise<GateResult> {
    return this.gate('fmt', ['fmt', '--check']);
  }

  async clippy(): Promise<GateResult> {
    return this.gate('clippy', ['clippy', '--all-targets', '--', '-D', 'warnings']);
  }

  async test(): Promise<GateResult> {
    return this.gate('test', ['test']);
  }

  /** 构建演示 wasm（wasm32-unknown-unknown，见 009 多环境集成）。 */
  async buildWasm(
    crate: string,
    profile: BuildProfile,
    features: readonly string[] = [],
  ): Promise<GateResult> {
    const args = ['build', '--target', 'wasm32-unknown-unknown', '-p', crate];
    if (profile === 'release') args.push('--release');
    for (const f of features) args.push('--features', f);
    return this.gate('build', args);
  }

  /** 构建 Win32 GNU 开发壳（交叉 target 由项目 flake 与 .cargo/config.toml 提供）。 */
  async buildWin32(crate: string, profile: BuildProfile): Promise<GateResult> {
    const args = ['build', '--target', 'x86_64-pc-windows-gnu', '-p', crate];
    if (profile === 'release') args.push('--release');
    return this.gateWithCommand('cargo-win32', 'build', args);
  }

  /** 构建 Apple Silicon macOS 开发壳；只能在本机 Apple SDK 环境中链接。 */
  async buildMacos(crate: string, profile: BuildProfile): Promise<GateResult> {
    const args = ['build', '--target', 'aarch64-apple-darwin', '-p', crate];
    if (profile === 'release') args.push('--release');
    return this.gate('build', args);
  }

  /** 读取 workspace 各 crate 的声明依赖（--no-deps：只取成员声明，不解析外部树）。 */
  async metadata(): Promise<CrateInfo[]> {
    const res = await this.process.run(
      'cargo',
      ['metadata', '--no-deps', '--format-version', '1'],
      { cwd: this.workspace.root },
    );
    if (res.code !== 0) {
      throw new Error(`cargo metadata 失败: ${res.stderr.trim() || res.stdout.trim()}`);
    }
    const parsed: { packages: { name: string; dependencies: { name: string; kind: string | null }[] }[] } =
      JSON.parse(res.stdout);
    return parsed.packages.map((p) => ({
      name: p.name,
      deps: p.dependencies.map((d) => ({
        name: d.name,
        kind: d.kind === 'dev' ? 'dev' : d.kind === 'build' ? 'build' : 'normal',
      })),
    }));
  }

  private async gate(id: GateResult['id'], args: string[]): Promise<GateResult> {
    return this.gateWithCommand('cargo', id, args);
  }

  private async gateWithCommand(command: string, id: GateResult['id'], args: string[]): Promise<GateResult> {
    const t0 = performance.now();
    const res: ProcessResult = await this.process.run(command, args, {
      cwd: this.workspace.root,
    });
    const durationMs = performance.now() - t0;
    const passed = res.code === 0;
    const detail = passed ? undefined : `${res.stderr.trim() || res.stdout.trim()}`.slice(0, 2000);
    const result: GateResult = { id, passed, durationMs };
    if (detail !== undefined) result.detail = detail;
    return result;
  }
}
