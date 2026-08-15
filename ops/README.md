# tela-ops — tela 开发运维工作流

> 把 tela 的日常开发操作（构建 / 验证 / 服务）收敛为一个 DDD 分层的 CLI。
> 运行时**零第三方依赖**（Node 内置 API + 本项目代码），工具链用最现代的 TS7。

## 为什么存在

旧开发方式是散装脚本：`scripts/check-architecture.sh`（bash 正则解析 Cargo.toml）、
`scripts/serve-demo.mjs`、手工三步构建（`cargo build --target wasm32-unknown-unknown`
→ `cp` → 外部冒烟脚本）。痛点：

- 构建 wasm 要手工三步，容易忘步骤；
- 验证门没有统一入口（fmt/clippy/test/arch 散在 flake 脚本里）；
- bash 解析 TOML 脆弱，改了依赖就崩。

## 安装与入口

`ops` 命令有三层入口（优先级从高到低）：

1. **用户级安装**（推荐，任何 shell 可用）：把包装脚本放到 PATH 里的 `~/.local/bin`：

   ```bash
   cat > ~/.local/bin/ops <<'EOF'
   #!/usr/bin/env bash
   exec node "/绝对路径/tela/ops/src/interface/cli.ts" "$@"
   EOF
   chmod +x ~/.local/bin/ops
   ```

2. **nix dev shell 自动**：flake `shellHook` 在未检测到 `ops` 命令时注册
   `alias ops="node <仓库根>/ops/src/interface/cli.ts"`（git 定位，任意目录进入都稳）。

3. **临时调用**：`node ops/src/interface/cli.ts <命令>`（仓库根下，任意 cwd）。

`ops --help` / `ops help` 显示全部命令。

## 四条命令

| 命令 | 做什么 | 对应旧方式 |
|---|---|---|
| `ops check` | 四道验证门：fmt / clippy / test / **依赖方向检查**（TS 版，cargo metadata 真实依赖树，替代 bash 正则） | flake `check` + check-architecture.sh |
| `ops build [demo\|frontend\|bundle\|win32\|macos\|all] [--release] [--gpu]` | 构建发布物到 `dist/`；`bundle` 固定生成经 Wasmtime 首帧/viewport 校验的 release WASM 包，`win32` 交叉构建开发壳，`macos` 在 Apple Silicon Mac 构建最小 `Tela.app`，`all` 先重建目录；`--gpu` 走 WebGPU 后端（webgpu feature + wasm-bindgen glue，强制 release，产出 `tela_demo_gpu.js`/`_bg.wasm`） | 手工三步 |
| `ops verify demo [--build]` | 由 `ops` 内置的 Node wasm 适配器验证 `dist/tela_demo.wasm`（--build 真的先构建） | 外部 smoke 脚本 |
| `ops serve [port]` | 开发静态服务器（默认 8000，端口占用自动递增，MIME/防穿越同旧脚本） | serve-demo.mjs |

`ops build all [--gpu]` 是默认发布组合：它先重建 `dist/`，再生成 CPU wasm、可选 GPU glue、浏览器
bundle 和 `tela-dev` 原生开发包。`ops build win32` 与 `ops build macos` 都保持显式：前者需要 GNU
Windows 交叉 target，后者需要 Apple Silicon macOS 的本机 SDK；它们不应让日常浏览器构建被平台工具链
阻塞。`macos` 只发布 AppKit/Metal 壳和 Info.plist，不把远程 WASM bundle 嵌入 App。单目标构建只更新
自己拥有的工件，不清除其他输出。

## DDD 分层

```
src/
├── interface/      接口层：cli.ts（参数解析 + 依赖组装 + 分发，Node util.parseArgs）
├── application/    应用层：用例（check/build-demo/verify-demo/serve/stress），只依赖端口
├── domain/         领域层：纯模型 + 端口（无 I/O，全部可单测）
│   ├── workspace.ts    路径模型（纯函数派生）
│   ├── gates.ts        验证门模型
│   ├── artifact.ts     构建工件模型
│   ├── architecture.ts 依赖方向规则（把 bash 脚本模型化为可测纯函数）
│   └── ports.ts        端口：Process/Fs/Server/Reporter/Wasm
└── infrastructure/ 基础设施层：Node 适配器（子进程/fs/http/wasm 加载/终端报告）
```

依赖方向：interface → application → domain ← infrastructure（端口实现注入应用层，
依赖倒置；domain 零 I/O 可离线单测）。

## 技术栈（为什么这样选）

- **Node 24 原生执行 TS**：`node src/interface/cli.ts` 直接跑，type stripping
  （要求 `erasableSyntaxOnly` 语法：无 enum / 参数属性 / namespace）；
- **TS7（native）typecheck**：`tsc --noEmit`（`typescript-native` 7.0.2）。
  typescript-eslint 8.66 尚不支持 TS7，按官方 side-by-side 方案与 TS 5.9.3 并存
  （pnpm 严格隔离：`.bin/tsc` → TS7，typescript-eslint peer → TS5.9）；
- **运行时零第三方依赖**：CLI 只用 Node 内置（child_process / http / fs / util）；
- 测试：`node --test`（内置 runner，直接跑 `.test.ts`）。

## 开发流程

```bash
pnpm install          # 安装 devDependencies（typescript/eslint/@types/node）
pnpm check            # typecheck (TS7) + lint + test（node:test）
pnpm dev -- stress    # 直接跑命令
```

## 与 tela 的边界

- ops 只碰**宿主侧开发流程**，不依赖 tela 的 Rust crate，也不被它们依赖；
- `ops check` 的依赖方向检查与旧 `scripts/check-architecture.sh` 规则等价
  （零依赖 crate / 白名单 / render 禁止反向依赖 core），数据源从 TOML 正则改为
  `cargo metadata --no-deps`（真实声明依赖，含 build/dev）；
