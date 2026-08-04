#!/usr/bin/env bash
# 依赖方向检查（基线编码规则，见 010-落地路线 M0）。
#
# 方向：宿主 → 适配 → 核心 → 契约。规则：
#   - tela-contract 零依赖；
#   - tela-core 只依赖 tela-contract；
#   - tela-render-* 只依赖 tela-contract，禁止反向依赖 tela-core。
#
# 用法：scripts/check-architecture.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

ZERO_DEP_CRATES=(tela-contract)

# 允许的依赖：<crate> 只允许依赖 <空格分隔的包名>
ALLOWED_DEPS=(
  "tela-core|tela-contract"
  "tela-render-raster|tela-contract"
)

# 提取 Cargo.toml 的 [dependencies] 节中的依赖包名（不含 dev/build 依赖）
crate_deps() {
  local file="$1"
  if [ ! -f "$file" ]; then
    echo "FAIL: 缺少 $file" >&2
    exit 1
  fi
  sed -n '/^\[dependencies\]$/,/^\[/p' "$file" \
    | sed '1d' \
    | grep -E '^[a-zA-Z0-9_-]+\s*=' \
    | sed -E 's/^([a-zA-Z0-9_-]+)\s*=.*/\1/' || true
}

fail=0

# 1. 零依赖 crate
for crate in "${ZERO_DEP_CRATES[@]}"; do
  deps="$(crate_deps "$ROOT/crates/$crate/Cargo.toml")"
  if [ -n "$deps" ]; then
    echo "FAIL: $crate 必须零依赖，实际依赖: $(echo "$deps" | tr '\n' ' ')"
    fail=1
  fi
done

# 2. 核心 crate 只依赖允许列表
for entry in "${ALLOWED_DEPS[@]}"; do
  crate="${entry%%|*}"
  allowed="${entry##*|}"
  for dep in $(crate_deps "$ROOT/crates/$crate/Cargo.toml"); do
    if ! grep -qw "$dep" <<<"$allowed"; then
      echo "FAIL: $crate 依赖了未允许的 crate: $dep（允许: $allowed）"
      fail=1
    fi
  done
done

# 3. render 后端禁止反向依赖 tela-core
for file in "$ROOT"/crates/tela-render-*/Cargo.toml; do
  [ -f "$file" ] || continue
  crate="$(basename "$(dirname "$file")")"
  if crate_deps "$file" | grep -qw tela-core; then
    echo "FAIL: $crate 禁止反向依赖 tela-core"
    fail=1
  fi
done

if [ "$fail" -ne 0 ]; then
  echo "依赖方向检查未通过"
  exit 1
fi
echo "OK: 依赖方向检查通过"
