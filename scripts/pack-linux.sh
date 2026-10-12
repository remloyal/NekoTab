#!/usr/bin/env bash
# 在本机（Linux / WSL）原生打包，产物落到 dist/ 下。
# Docker 只负责把产物打进镜像（见 Dockerfile.prebuilt），不在容器里编译。
#
# 用法：
#   bash scripts/pack-linux.sh                              # 只打本机架构 -> dist/linux/
#   bash scripts/pack-linux.sh aarch64-unknown-linux-gnu     # 交叉打 arm64 -> dist/linux-<triple>/
#   bash scripts/pack-linux.sh aarch64-unknown-linux-gnu armv7-unknown-linux-gnueabihf
#
# 关于多架构：
#   wasm 客户端与架构无关，只构建一次；随架构变化的只有服务端二进制。
#   dx 自带的 --target 不能用于此处：它会把目标架构也套到必须编成 wasm 的客户端上，
#   getrandom 的 wasm_js 后端在非 wasm 目标下不存在，直接编译失败（--client-target
#   也救不回来）。所以这里客户端交给 dx，服务端按目标架构用 cargo 单独构建。
#
# 前置条件：
#   * rustc >= 1.94（sqlx 0.9 的 MSRV）
#   * dioxus-cli 0.7.10：cargo install dioxus-cli --version 0.7.10 --locked
#   * 构建依赖：build-essential pkg-config libssl-dev make cmake g++
#   * 交叉构建还需：对应架构的交叉 gcc（如 gcc-aarch64-linux-gnu）+ rustup target add <triple>；
#     交叉 gcc 名不是 <arch>-linux-gnu-gcc 时，用环境变量 CROSS_CC 指定
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

command -v dx >/dev/null 2>&1 || {
    echo "!! 找不到 dx：cargo install dioxus-cli --version 0.7.10 --locked" >&2
    exit 1
}
command -v rustc >/dev/null 2>&1 || { echo "!! 找不到 rustc" >&2; exit 1; }

# 编译缓存默认放 ext4（避免 /mnt 的 9p 文件系统拖慢编译），可用环境变量覆盖。
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/mtab-lite-target}"

HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
TARGETS=("$@")
if [ "${#TARGETS[@]}" -eq 0 ]; then
    TARGETS=("$HOST_TRIPLE")
fi

echo ">> 构建客户端（wasm，与架构无关）"
dx build --release --platform web

DX_OUT="$CARGO_TARGET_DIR/dx/mtab-lite/release/web"
if [ ! -d "$DX_OUT/public" ]; then
    echo "!! 缺少客户端产物：$DX_OUT/public" >&2
    exit 1
fi

out_dir_for() {   # 宿主用 dist/linux，其它架构用 dist/linux-<triple>
    if [ "$1" = "$HOST_TRIPLE" ]; then echo "dist/linux"; else echo "dist/linux-$1"; fi
}

stage() {         # $1=服务端二进制路径  $2=输出目录
    local dest="$REPO_ROOT/$2"
    rm -rf "$dest"
    mkdir -p "$dest"
    cp -r "$DX_OUT/public" "$dest/public"
    if [ -f "$DX_OUT/.manifest.json" ]; then cp "$DX_OUT/.manifest.json" "$dest/"; fi
    cp "$1" "$dest/server"     # 运行镜像按 /app/server 启动
    printf '   -> %-30s %s\n' "$2" "$(file -b "$dest/server" | cut -d, -f1-2)"
}

for t in "${TARGETS[@]}"; do
    dest="$(out_dir_for "$t")"
    if [ "$t" = "$HOST_TRIPLE" ]; then
        echo ">> 宿主架构 $t"
        stage "$DX_OUT/server" "$dest"
    else
        echo ">> 交叉构建服务端 $t"
        arch="${t%%-*}"
        cc_bin="${CROSS_CC:-${arch}-linux-gnu-gcc}"
        key="$(printf '%s' "$t" | tr 'a-z-' 'A-Z_')"
        export "CARGO_TARGET_${key}_LINKER=$cc_bin"
        export "CC_${key}=$cc_bin"
        cargo build --release --target "$t" --no-default-features --features server
        stage "$CARGO_TARGET_DIR/$t/release/mtab-lite" "$dest"
    fi
done

echo
echo ">> 产物目录："
for t in "${TARGETS[@]}"; do echo "   $(out_dir_for "$t")"; done
echo ">> 打包示例（只 COPY，不编译）："
echo "   docker build -f Dockerfile.prebuilt -t mtab-lite:prebuilt dist/linux"
