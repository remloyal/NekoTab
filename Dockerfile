# 多阶段构建。已在 WSL(Ubuntu 24.04) + Docker 实测通过：
#   docker build -t mtab-lite .
#   docker run -d --name mtab-lite -p 8080:8080 -v mtab-data:/data mtab-lite
# 产物目录 target/dx/mtab-lite/release/web/ 内含服务端二进制 server 与 public/。

# ---------- 构建阶段 ----------
# 基础镜像版本由实际依赖决定，不能用 Cargo.toml 里声明的 rust-version 1.85：
#   * sqlx 0.9.0 要求 rustc >= 1.94（客户端 wasm 不含 sqlx，能过；服务端构建
#     会因 cargo 的 MSRV 校验直接失败）
#   * dioxus-cli 0.7.10 的依赖树要求 rustc >= 1.88
# 取两者较高值 1.94。用 -bookworm 变体是为了与运行阶段 debian:bookworm-slim
# 的 glibc 保持一致。
FROM rust:1.94-slim-bookworm AS builder

# libssl-dev 是 dioxus-cli 依赖链里 openssl-sys 的硬需求，缺了会在
# `cargo install dioxus-cli` 处报 "Could not find openssl via pkg-config"。
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        ca-certificates \
        curl \
        libssl-dev \
        make \
        cmake \
        g++ \
    && rm -rf /var/lib/apt/lists/*

# dx 构建 web 客户端需要 wasm 目标
RUN rustup target add wasm32-unknown-unknown

# CLI 版本必须与 Cargo.toml 里的 dioxus 依赖一致
RUN cargo install dioxus-cli --version 0.7.10 --locked

WORKDIR /app
COPY . .

RUN dx build --release --platform web

# ---------- 运行阶段 ----------
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 实测：服务端启动时会读取可执行文件同级的 public/ 目录（index.html / wasm /
# assets）。dx 会指好该目录，但裸跑二进制必须自带它，因此这里拷贝整个 web/
# 产物目录，而不是只拷可执行文件。
COPY --from=builder /app/target/dx/mtab-lite/release/web/ /app/

# ICON_DIR 与数据一起放挂载卷：图标是落盘缓存的字节，容器重建后不必重新代取。
ENV DATABASE_URL="sqlite:///data/mtab.db?mode=rwc" \
    ICON_DIR="/data/icon" \
    IP=0.0.0.0 \
    PORT=8080

VOLUME /data
EXPOSE 8080

# dx build --platform web 产出的服务端二进制名为 `server`（不是包名 mtab-lite），
# 实测位于 web/ 目录根部，与 public/ 同级。
CMD ["/app/server"]
