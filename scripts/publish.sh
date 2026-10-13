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

command -v docker >/dev/null 2>&1 || { echo "!! 找不到 docker" >&2; exit 1; }
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
