#!/usr/bin/env bash
# 在本机（Linux / WSL）原生打包，产物落到 dist/<架构>/ 下。
# Docker 只负责把产物打进镜像（见 Dockerfile.prebuilt），不在容器里编译。
#
# 用法：
#   bash scripts/pack-linux.sh                              # 本机架构 -> dist/amd64/
#   bash scripts/pack-linux.sh aarch64-unknown-linux-gnu     # 交叉 arm64 -> dist/arm64/
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

# rustup 安装时用了 --no-modify-path，普通 shell 里 ~/.cargo/bin 不在 PATH 上，这里补上，
# 保证脚本可以直接跑（也能让调用它的 publish.sh 继承到）。
if [ -x "$HOME/.cargo/bin/dx" ] && ! command -v dx >/dev/null 2>&1; then
    PATH="$HOME/.cargo/bin:$PATH"
    export PATH
fi

command -v dx >/dev/null 2>&1 || {
    echo "!! 找不到 dx：cargo install dioxus-cli --version 0.7.10 --locked" >&2
    exit 1
}
command -v rustc >/dev/null 2>&1 || { echo "!! 找不到 rustc" >&2; exit 1; }

# 编译缓存默认放 ext4（避免 /mnt 的 9p 文件系统拖慢编译），可用环境变量覆盖。
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/nekotab-target}"

HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
TARGETS=("$@")
if [ "${#TARGETS[@]}" -eq 0 ]; then
    TARGETS=("$HOST_TRIPLE")
fi

# target triple -> Docker 架构名（同时用作产物目录名）
docker_arch() {
    case "$1" in
        x86_64-*)      echo amd64 ;;
        aarch64-*)     echo arm64 ;;
        armv7-*|arm-*) echo arm ;;
        i686-*)        echo 386 ;;
        riscv64*)      echo riscv64 ;;
        loongarch64-*) echo loong64 ;;
        *)             echo "${1%%-*}" ;;
    esac
}

DX_OUT="$CARGO_TARGET_DIR/dx/nekotab/release/web"

# 先清掉上一次的客户端产物再构建。
# dx 往 public/assets/ 里写的是带内容哈希的文件名，且**只增不减**：换一次代码就
# 多一份 wasm/js，旧的一直留着。而下面 stage() 是整目录拷贝，于是这些陈旧产物
# 会一起进镜像——实测一次发布里躺着 4 份 wasm 共 6.3 MB，其中 3 份是历史版本，
# 而且部署后仍可被 URL 直接请求到（等于对外提供旧版客户端）。
# public/ 是 dx 的生成目录（不是仓库里的源目录，本项目自己的资源在 assets/），
# 删掉后每次构建都会完整重建。
rm -rf "$DX_OUT"

echo ">> 构建客户端（wasm，与架构无关）"
dx build --release --platform web

if [ ! -d "$DX_OUT/public" ]; then
    echo "!! 缺少客户端产物：$DX_OUT/public" >&2
    exit 1
fi

# 自检：产物里只应有本次构建引用的那一份 wasm，多出来就是上面的清理没生效
echo "   -> public 资源合计 $(du -sh "$DX_OUT/public" | cut -f1)，wasm $(ls -1 "$DX_OUT/public/assets/"*.wasm 2>/dev/null | wc -l) 份"

stage() {         # $1=服务端二进制路径  $2=输出目录
    local dest="$REPO_ROOT/$2"
    rm -rf "$dest"
    mkdir -p "$dest"
    cp -r "$DX_OUT/public" "$dest/public"
    if [ -f "$DX_OUT/.manifest.json" ]; then cp "$DX_OUT/.manifest.json" "$dest/"; fi
    cp "$1" "$dest/server"     # 运行镜像按 /app/server 启动
    printf '   -> %-10s %s\n' "$2" "$(file -b "$dest/server" | cut -d, -f1-2)"
}

for t in "${TARGETS[@]}"; do
    dest="dist/$(docker_arch "$t")"
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
        stage "$CARGO_TARGET_DIR/$t/release/nekotab" "$dest"
    fi
done

echo
echo ">> 产物目录："
for t in "${TARGETS[@]}"; do echo "   dist/$(docker_arch "$t")"; done
echo ">> 打成镜像（只 COPY，不编译）："
echo "   docker build -f Dockerfile.prebuilt -t nekotab:local dist/amd64"
echo ">> 或直接构建并推送多架构镜像："
echo "   bash scripts/publish.sh"
