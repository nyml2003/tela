#!/usr/bin/env bash
# 依赖方向检查（基线编码规则，见 010-落地路线 M0）。
#
# 方向：宿主 → 适配 → 核心 → 契约。规则：
#   - tela-contract 零依赖；
#   - tela-core 只依赖 tela-contract；
#   - tela-render-* 只依赖 tela-contract + 字形/数学/编码库（007-7.1），禁止反向依赖 tela-core；
#   - dev-dependencies 仅限测试专用集成（core → raster）。
#
# 用法：scripts/check-architecture.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

ZERO_DEP_CRATES=(tela-contract)

# 允许的依赖：<crate> 只允许依赖 <空格分隔的包名>
ALLOWED_DEPS=(
  "tela-core|tela-contract"
  "tela-render-raster|tela-contract ab_glyph ab_glyph_rasterizer png font8x8"
  "tela-render-canvas|tela-contract"
)
# dev-dependencies 白名单：core 的 dev 依赖仅限测试专用后端（集成测试，不进入运行时依赖）
ALLOWED_DEV_DEPS=(
  "tela-core|tela-render-raster"
)

# 提取 Cargo.toml 指定节（[dependencies] / [dev-dependencies]）的依赖包名
crate_deps() {
  local file="$1"
  local section="$2"
  if [ ! -f "$file" ]; then
    echo "FAIL: 缺少 $file" >&2
    exit 1
  fi
  sed -n "/^\[$section\]$/,/^\[/p" "$file" \
    | sed '1d' \
    | grep -E '^[a-zA-Z0-9_-]+\s*=' \
    | sed -E 's/^([a-zA-Z0-9_-]+)\s*=.*/\1/' || true
}

fail=0

# 1. 零依赖 crate（含 dev-dependencies）
for crate in "${ZERO_DEP_CRATES[@]}"; do
  deps="$(crate_deps "$ROOT/crates/$crate/Cargo.toml" dependencies)"
  deps="$deps $(crate_deps "$ROOT/crates/$crate/Cargo.toml" dev-dependencies)"
  if [ -n "${deps//[[:space:]]/}" ]; then
    echo "FAIL: $crate 必须零依赖，实际依赖: $(echo "$deps" | tr '\n' ' ')"
    fail=1
  fi
done

# 2. 核心 crate 只依赖允许列表（[dependencies] 与 [dev-dependencies] 分别校验）
check_section() {
  local section="$1"; shift
  local entry_allowed=("$@")
  for entry in "${entry_allowed[@]}"; do
    local crate="${entry%%|*}"
    local allowed="${entry##*|}"
    for dep in $(crate_deps "$ROOT/crates/$crate/Cargo.toml" "$section"); do
      if ! grep -qw "$dep" <<<"$allowed"; then
        echo "FAIL: $crate [$section] 依赖了未允许的 crate: $dep（允许: $allowed）"
        fail=1
      fi
    done
  done
}
check_section dependencies "${ALLOWED_DEPS[@]}"
check_section dev-dependencies "${ALLOWED_DEV_DEPS[@]}"

# 3. render 后端禁止反向依赖 tela-core（含 dev-dependencies）
for file in "$ROOT"/crates/tela-render-*/Cargo.toml; do
  [ -f "$file" ] || continue
  crate="$(basename "$(dirname "$file")")"
  deps="$(crate_deps "$file" dependencies) $(crate_deps "$file" dev-dependencies)"
  if grep -qw tela-core <<<"$deps"; then
    echo "FAIL: $crate 禁止反向依赖 tela-core"
    fail=1
  fi
done

if [ "$fail" -ne 0 ]; then
  echo "依赖方向检查未通过"
  exit 1
fi
echo "OK: 依赖方向检查通过"
