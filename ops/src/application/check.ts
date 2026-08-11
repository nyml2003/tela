// 应用层：check 用例——四道验证门（fmt/clippy/test/arch），替代 flake 里的 check 脚本。
import type { Reporter } from '../domain/ports.ts';
import type { GateResult } from '../domain/gates.ts';
import { CHECK_GATES } from '../domain/gates.ts';
import { checkArchitecture } from '../domain/architecture.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

export interface CheckResult {
  passed: boolean;
  gates: GateResult[];
}

export interface CheckDeps {
  cargo: CargoPort;
  reporter: Reporter;
}

/** 顺序执行四道门；arch 门用 cargo metadata（真实依赖树）而非 TOML 正则。 */
export async function runCheck(deps: CheckDeps): Promise<CheckResult> {
  const { cargo, reporter } = deps;
  const gates: GateResult[] = [];

  reporter.section('验证门（check）');
  gates.push(await cargo.fmtCheck());
  gates.push(await cargo.clippy());
  gates.push(await cargo.test());

  // arch 门：metadata → 纯规则校验（domain/architecture.ts）。
  const t0 = performance.now();
  let archPassed = true;
  let archDetail: string | undefined;
  try {
    const crates = await cargo.metadata();
    const violations = checkArchitecture(crates);
    archPassed = violations.length === 0;
    if (!archPassed) {
      archDetail = violations.map((v) => `${v.crate}: ${v.message}`).join('\n');
    }
  } catch (err) {
    archPassed = false;
    archDetail = String(err);
  }
  const archResult: GateResult = {
    id: 'arch',
    passed: archPassed,
    durationMs: performance.now() - t0,
  };
  if (archDetail !== undefined) archResult.detail = archDetail;
  gates.push(archResult);

  for (const g of gates) {
    const label = CHECK_GATES.find((s) => s.id === g.id)?.label ?? g.id;
    const time = `(${g.durationMs.toFixed(0)}ms)`;
    if (g.passed) {
      reporter.ok(`${label} ${time}`);
    } else {
      reporter.fail(`${label} ${time}`);
      if (g.detail) reporter.info(g.detail);
    }
  }

  const passed = gates.every((g) => g.passed);
  reporter.section(passed ? 'check 全部通过' : `check 失败：${gates.filter((g) => !g.passed).length} 道门未通过`);
  return { passed, gates };
}
