# AGENTS.md — tela 项目环境与 Nix 加速知识库

## Nix 下载慢的根因与修复（本项目实测，2026-08）

### 镜像优先级坑（最重要）
- Nix 按 `substituters` 配置顺序**固定**使用源：第一个能返回 narinfo 的源会被一直用，**不会自动选最快**。
- 国内直连镜像必须排在 `cache.nixos.org` **前面**，官方源放最后兜底。
- 当前生效顺序：`mirrors.ustc.edu.cn/nix-channels/store` → `mirror.sjtu.edu.cn/nix-channels/store` → `cache.nixos.org`。
- 实测延迟：cache.nixos.org ~216ms，USTC ~52ms，SJTU ~49ms，TUNA ~107ms。

### narinfo 探测法（判断镜像真可用，不是看 nix-cache-info）
- 只看 `nix-cache-info` 200 是**假象**（部分镜像 nar 实际 404）。
- **必须用纯 hash 形式**：`https://<mirror>/nix-channels/store/<hash>.narinfo`（`<hash>-<name>.narinfo`
  全名形式实测四源全 404，纯 hash 全 200——2026-08 实测确认，脚本已按此实现）。
- 注意：本机 flake 锁定的 nixpkgs rev（`531670d8…`）**不是官方 channel rev**（`02e08985a27c…`），
  其二进制在官方 cache 与镜像上全部 404（实测 0/23）→ 依赖全走本地编译。
  要命中官方预编译缓存，需把 rev 对齐官方 channel（`nix flake update nixpkgs` 后核对 `.git-revision`）。
- 一键检测脚本：`nix-mirror-check`（见下）。

### flake registry 与 tarball 下载（2026-08 实测）
- 用户级 registry 已改为 TUNA：`nix registry list` 应显示
  `user flake:nixpkgs https://mirrors.tuna.tsinghua.edu.cn/nix-channels/nixpkgs-unstable/nixexprs.tar.xz`
  （默认 global 是 channels.nixos.org，直连慢且易截断 "Truncated tar archive"）。
- **nix 下载 tarball 走 FlClash 代理会截断**（curl 走同一代理却成功）→ 必须 `no_proxy` 排除镜像域名
  （HM `home.sessionVariables.NO_PROXY` 已含 `mirrors.tuna.tsinghua.edu.cn,mirrors.ustc.edu.cn,mirror.sjtu.edu.cn,channels.nixos.org`；
  daemon 侧 `/etc/systemd/system/nix-daemon.service.d/override.conf` 需同步加）。
- 实测：no_proxy 直连 TUNA 40MB tarball 4.5MB/s 无截断；改后 `nix build nixpkgs#nodejs`（35.4MB 闭包）总耗时 1.45s。
- channel tarball 顶层带版本目录，但 nix 2.34 实测**可**直接作 flake 输入（自动下钻单顶层目录），
  早期 path: 输入测到 "flake.nix does not exist" 是 path 输入不下钻所致，tarball URL 输入正常。

### 配置层级与生效（踩过的坑）
- 配置优先级：命令行 > `~/.config/nix/nix.conf`（Home Manager 生成）> `/etc/nix/nix.conf`。
- **用户级配置覆盖系统级**：只改 `/etc/nix/nix.conf` 不会生效，必须同步改 HM 源头
  `~/.config/home-manager/home.nix` 的 `nix.settings.substituters`，再 `home-manager switch`。
- 改 `/etc/nix/nix.conf` 后必须重启 daemon：`sudo systemctl restart nix-daemon`（Determinate Nix systemd）。
- 本机为 Determinate Nix 2.34.7，daemon drop-in 在 `/etc/systemd/system/nix-daemon.service.d/override.conf`。

### flake 输入（本项目实测）
- **channel tarball 不能直接作为 flake 输入**：`channels.nixos.org` 与 TUNA/USTC 的 `nixexprs.tar.xz`
  顶层都带版本目录（如 `nixpkgs-26.11pre…/`），nix 2.34 实测报 `flake.nix does not exist`。
- 本项目保持 `inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05"`（tarball 已缓存，日常不进网络）。
- `nix build nixpkgs#<pkg>` 走 flake registry 默认源 = `channels.nixos.org/nixpkgs-unstable/nixexprs.tar.xz`，
  该源直连慢且易截断（实测 "Truncated tar archive"）→ 非必要少用裸 `nixpkgs#` 引用。
- system 必须为 `x86_64-linux`（WSL2 本机），`.envrc` 用 `use flake`，进入目录自动加载。

### 代理（WSL2 + FlClash）
- 本机为 **mirrored 网络模式**（Windows 侧 `.wslconfig` 配置）：WSL 内 `127.0.0.1` 直通 Windows，
  FlClash 需开 Allow LAN，混合端口 **7897**（非默认 7890）。
- 判定方法：WSL 内 `ss -tlnp | grep 789x` 无本机进程但 `curl 127.0.0.1:7897` 通 → mirrored。
- 代理变量由 Home Manager `home.sessionVariables` 固化（base.nix），daemon 侧在 override.conf。
- **镜像域名必须进 `no_proxy` 直连**：nix 经 FlClash 代理下载 tarball 会截断（curl 同代理却成功），
  直连国内镜像 4.5MB/s 无截断（HM 已配，daemon override.conf 需同步）。
- 若改用默认 NAT 模式：宿主 IP = `ip route show default | awk '{print $3}'`（形如 10.x.x.1），端口 7890。
- `NO_PROXY` 保留 `10.*,192.168.*,172.*,localhost` 等网段；国内镜像走代理实测 50ms 级，无需特判。

### Rust 加速
- `~/.cargo/config.toml` 已配置 rsproxy sparse 镜像（`sparse+https://rsproxy.cn/index/` +
  `git-fetch-with-cli = true`）。项目级 `.cargo/config.toml` 只含 wasm/Win32 链接器 flag，勿混。
- rustup 慢时临时用：`export RUSTUP_DIST_SERVER=https://rsproxy.cn/dist RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup`
  （若 `rustup default stable` 报 "no release found"，重试官方源即可）。
- **绝不要手改 `~/.rustup/settings.toml`**（rustup 自管；改坏后删除该文件让 rustup 重建）。

## 防复发机制
- `~/.local/bin/nix-mirror-check`：真实 narinfo 检测 ustc/sjtu/tuna/official 四源；
  `--apply` 用 `NIX_SUDO_PASS` + `printf|sudo -S` 写 `/etc/nix/nix.conf` 并重启 daemon
  （配置内容先写临时文件再 `sudo cp`，避免 sudo -S 与 heredoc 抢 stdin）。
- 检测样本：`~/.local/share/nix-mirror-check/samples.txt`，每行一个 `<hash>` 或 `<hash>-<name>`
  （从真实下载日志 `nix build <pkg> -v 2>&1 | grep "copying path from"` 提取）。
- 改任何 nix 配置后验证：`nix config show | grep -A5 substituters`。
