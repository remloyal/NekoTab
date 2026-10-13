#!/usr/bin/env bash
# 构建并推送多架构镜像到镜像仓库（默认 Docker Hub）。
# 流程：本机原生打包产物 -> 各架构分别 build & push -> 合成多架构 manifest。
#
# 用法：
#   bash scripts/publish.sh                     # 版本取 Cargo.toml，推 amd64+arm64，并更新 latest
#   bash scripts/publish.sh 0.2.0               # 指定版本标签
#   IMAGE=someone/nekotab bash scripts/publish.sh
#   ARCHES="arm64" bash scripts/publish.sh 0.2.0        # 只推单架构（此时 $IMAGE:0.2.0 直接指向它）
#   SKIP_BUILD=1 bash scripts/publish.sh                # 不重新打包，直接用已有 dist/<arch>
#   SKIP_LATEST=1 bash scripts/publish.sh 0.2.0-rc1     # 预发布：不碰 latest
#   DRY_RUN=1 bash scripts/publish.sh                   # 只打印将要执行的命令，不做任何改动
#
# 环境变量：
#   IMAGE         镜像名，默认 remloyal001/nekotab
#   ARCHES        目标架构，默认 "amd64 arm64"（可选 amd64 arm64 arm 386 riscv64 loong64）
#   SKIP_BUILD    非空则跳过本机打包（要求 dist/<arch> 已存在）
#   SKIP_LATEST   非空则不更新 latest
#   DRY_RUN       非空则只打印命令
#   DOCKER_USER / DOCKER_TOKEN   两者都设则先自动 docker login（适合 CI）
#   NEKOTAB_WSL_DISTRO           Windows 下指定用哪个 WSL 发行版（默认用默认发行版）
#   NEKOTAB_NO_WSL               非空则不在 Windows 下自动改用 WSL
#
# Windows 用户注意：产物的服务端二进制必须是 Linux 的，而 docker 通常也只装在
# WSL 里。所以在 Git Bash/MSYS 下运行本脚本时，它会自动把自己丢进 WSL 重跑一遍
# （详见下方注释），直接 `scripts/publish.sh` 即可。
#
# 前置：
#   * 已 docker login（或设 DOCKER_USER/DOCKER_TOKEN）
#   * buildx 可用（Ubuntu：apt install docker-buildx）
#   * 打包依赖与交叉工具链见 scripts/pack-linux.sh
#
# 说明：交叉架构（如 arm64）在本机是靠 QEMU 模拟构建的，较慢；
#       在目标架构机器上跑同样命令则是原生构建，更快。
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# ---- Windows(Git Bash/MSYS) 下自动改到 WSL 里执行 ----
#
# 发布链路的产物必须是 Linux 的：pack-linux.sh 用本机工具链编译服务端二进制，
# 而 Windows 上编出的是 server.exe（PE 文件），塞进 Linux 容器跑不起来。
# 另外 docker 往往只装在 WSL 里（没装 Docker Desktop 时 Git Bash 完全看不到），
# 于是脚本会在 `command -v docker` 那一步直接失败。
#
# 所以：检测到 MSYS/MinGW 且 wsl.exe 可用时，把脚本自己丢进 WSL 重跑一遍。
#   * 仓库路径交给 WSL 侧的 wslpath 转换（比手工拼 /mnt 前缀可靠）
#   * 参数按原样转过去（版本号等）
#   * 凭据沿用 WSL 里已有的 docker login
#   * NEKOTAB_IN_WSL 是防重入标记：WSL 里也能看到 wsl.exe（interop），
#     不标记就会无限套娃
# 想自己指定发行版：NEKOTAB_WSL_DISTRO=Ubuntu-24.04
# 想关掉这个行为（例如非 WSL 的其它 Linux 环境）：NEKOTAB_NO_WSL=1
case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
        if [ -z "${NEKOTAB_IN_WSL:-}" ] && [ -z "${NEKOTAB_NO_WSL:-}" ] \
            && command -v wsl.exe >/dev/null 2>&1; then
            if command -v cygpath >/dev/null 2>&1; then
                win_root="$(cygpath -w "$REPO_ROOT")"
            else
                win_root="$REPO_ROOT"
            fi

            inner="cd \"\$(wslpath -a '$win_root')\" && NEKOTAB_IN_WSL=1 bash scripts/publish.sh"
            for arg in "$@"; do
                inner="$inner $(printf '%q' "$arg")"
            done

            wsl_args=(wsl.exe)
            if [ -n "${NEKOTAB_WSL_DISTRO:-}" ]; then
                wsl_args+=(-d "$NEKOTAB_WSL_DISTRO")
            fi

            echo ">> 本机（$(uname -s)）不适合构建 Linux 产物，改用 WSL 里的 docker 重新执行"
            echo "   （关掉这个行为：NEKOTAB_NO_WSL=1）"
            exec "${wsl_args[@]}" -- bash -lc "$inner"
        fi
        ;;
esac

export DOCKER_CLI_EXPERIMENTAL=enabled   # docker manifest 子命令

# rustup 安装时用了 --no-modify-path，普通 shell 里 ~/.cargo/bin 不在 PATH 上，这里补上。
if [ -x "$HOME/.cargo/bin/dx" ] && ! command -v dx >/dev/null 2>&1; then
    PATH="$HOME/.cargo/bin:$PATH"
    export PATH
fi

IMAGE="${IMAGE:-remloyal001/nekotab}"
ARCHES="${ARCHES:-amd64 arm64}"

if [ "$#" -ge 1 ] && [ -n "${1:-}" ]; then
    VERSION="$1"
else
    # 取 [package] 段的 version（Cargo.toml 里第一个 ^version 行）
    VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
fi
if [ -z "$VERSION" ]; then
    echo "!! 无法从 Cargo.toml 读取版本，请显式传入：bash scripts/publish.sh 0.1.0" >&2
    exit 1
fi

triple_for() {
    case "$1" in
        amd64)   echo x86_64-unknown-linux-gnu ;;
        arm64)   echo aarch64-unknown-linux-gnu ;;
        arm)     echo armv7-unknown-linux-gnueabihf ;;
        386)     echo i686-unknown-linux-gnu ;;
        riscv64) echo riscv64gc-unknown-linux-gnu ;;
        loong64) echo loongarch64-unknown-linux-gnu ;;
        *) echo "!! 未知架构：$1（可用 amd64 arm64 arm 386 riscv64 loong64）" >&2; exit 1 ;;
    esac
}

platform_for() {
    case "$1" in
        arm) echo linux/arm/v7 ;;
        *)   echo "linux/$1" ;;
    esac
}

run() {
    if [ -n "${DRY_RUN:-}" ]; then
        printf '+ %s\n' "$*"
    else
        "$@"
    fi
}

command -v docker >/dev/null 2>&1 || {
    echo "!! 找不到 docker" >&2
    echo "   Windows 上 docker 一般装在 WSL 里：在 WSL 内执行本脚本，或去掉 NEKOTAB_NO_WSL 让它自动切换。" >&2
    exit 1
}
if ! docker buildx version >/dev/null 2>&1; then
    echo "!! 需要 docker buildx（Ubuntu：apt install docker-buildx）" >&2
    exit 1
fi

if [ -n "${DOCKER_USER:-}" ] && [ -n "${DOCKER_TOKEN:-}" ]; then
    if [ -n "${DRY_RUN:-}" ]; then
        echo "+ docker login -u $DOCKER_USER --password-stdin   # 口令来自 DOCKER_TOKEN"
    else
        printf '%s' "$DOCKER_TOKEN" | docker login -u "$DOCKER_USER" --password-stdin
    fi
fi

# shellcheck disable=SC2086
ARCH_LIST="$(echo $ARCHES)"
ARCH_COUNT=0
TRIPLES=()
for a in $ARCH_LIST; do
    TRIPLES+=("$(triple_for "$a")")
    ARCH_COUNT=$((ARCH_COUNT + 1))
done
if [ "$ARCH_COUNT" -eq 0 ]; then
    echo "!! ARCHES 为空" >&2
    exit 1
fi

if [ -z "${SKIP_BUILD:-}" ]; then
    echo ">> 本机打包：$ARCH_LIST"
    run bash scripts/pack-linux.sh "${TRIPLES[@]}"
fi

echo ">> 构建并推送：$IMAGE:$VERSION"
for a in $ARCH_LIST; do
    ctx="dist/$a"
    if [ ! -d "$ctx" ]; then
        if [ -n "${DRY_RUN:-}" ]; then
            echo "   (dry-run：$ctx 尚不存在，实跑时会由 pack-linux.sh 生成)"
        else
            echo "!! 缺少产物目录 $ctx —— 先跑 bash scripts/pack-linux.sh，或别设 SKIP_BUILD" >&2
            exit 1
        fi
    fi
    args=(--platform "$(platform_for "$a")" --provenance=false -f Dockerfile.prebuilt)
    args+=(-t "$IMAGE:$VERSION-$a")
    if [ "$ARCH_COUNT" -eq 1 ]; then
        args+=(-t "$IMAGE:$VERSION")
        if [ -z "${SKIP_LATEST:-}" ]; then
            args+=(-t "$IMAGE:latest")
        fi
    fi
    run docker buildx build "${args[@]}" --push "$ctx"
done

if [ "$ARCH_COUNT" -gt 1 ]; then
    echo ">> 合成多架构 manifest"
    for t in "$VERSION" latest; do
        if [ "$t" = "latest" ] && [ -n "${SKIP_LATEST:-}" ]; then
            continue
        fi
        run docker manifest rm "$IMAGE:$t" 2>/dev/null || true
        amend=()
        for a in $ARCH_LIST; do
            amend+=(--amend "$IMAGE:$VERSION-$a")
        done
        run docker manifest create "$IMAGE:$t" "${amend[@]}"
        run docker manifest push "$IMAGE:$t"
    done
fi

echo
echo ">> 完成："
echo "   docker pull $IMAGE:$VERSION"
if [ "$ARCH_COUNT" -gt 1 ] && [ -z "${SKIP_LATEST:-}" ]; then
    echo "   docker pull $IMAGE:latest"
fi
