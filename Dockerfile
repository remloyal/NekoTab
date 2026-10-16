# 多阶段构建。已在 WSL(Ubuntu 24.04) + Docker 实测通过：
#   docker build -t nekotab .
#   docker run -d --name nekotab -p 8080:8080 -v mtab-data:/data nekotab
# 产物目录 target/dx/nekotab/release/web/ 内含服务端二进制 server 与 public/。

# ---------- 构建阶段 ----------
# 基础镜像版本由实际依赖决定，不能用 Cargo.toml 里声明的 rust-version 1.85：
#   * sqlx 0.9.0 要求 rustc >= 1.94（客户端 wasm 不含 sqlx，能过；服务端构建
#     会因 cargo 的 MSRV 校验直接失败）
#   * dioxus-cli 0.7.10 的依赖树要求 rustc >= 1.88
# 取两者较高值 1.94。用 -bookworm 变体是为了与运行阶段 debian:bookworm-slim
# 的 glibc 保持一致。
FROM rust:1.94-slim-bookworm AS builder

# dx 下载的工具（wasm-opt / wasm-bindgen…）都放在 $DX_HOME/tools 下。
# 显式钉住这个路径，下面才能把 wasm-opt 预先放进去，而不依赖 dx 的默认值。
ENV DX_HOME=/opt/dx

# libssl-dev 是 dioxus-cli 依赖链里 openssl-sys 的硬需求，缺了会在
# `cargo install dioxus-cli` 处报 "Could not find openssl via pkg-config"。
# binaryen 见下面那条：用来让 wasm-opt 走发行版，而不是去 GitHub 下。
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        ca-certificates \
        curl \
        libssl-dev \
        make \
        cmake \
        g++ \
        binaryen \
    && rm -rf /var/lib/apt/lists/*

# 把发行版的 wasm-opt 放进 dx 的工具缓存目录。
#
# 为什么需要：dx 收尾时用 wasm-opt 压 wasm，而它**默认去 GitHub Releases 下 binaryen**。
# 在连不上 GitHub 的网络里（内网 CI、国内直连），整个构建会倒在最后一步——前面客户端与
# 服务端其实都已编译成功，报错却是 "Failed to create wasm-opt instance: error sending
# request for url (https://github.com/WebAssembly/binaryen/releases/…)"，很难归因。
#
# dx 的查找顺序是"先看自己的缓存目录，命中就不下载"（dioxus-cli 的
# wasm_opt::get_binary_path），所以落到那个目录即可。**不用 NO_DOWNLOADS=1**：那是全局
# 开关，会连带让 wasm-bindgen 也改去 PATH 找，反而更麻烦。
#
# 目录名里的 129 是 dx 内部的 BINARYEN_VERSION 常量。**dx 升级后这里要跟着改**——改错了
# 不会报错，只是悄悄退回"去 GitHub 下"，也就是回到连不上时的那个失败。
# 发行版自带的是 108（比 dx 想下的 127 旧）：实测 dx 传的那组参数它全都接受，
# 优化效果可能略差一点，但换来的是这个镜像在任何网络里都能构建。
RUN mkdir -p "$DX_HOME/tools/binaryen-129/bin" \
    && ln -s /usr/bin/wasm-opt "$DX_HOME/tools/binaryen-129/bin/wasm-opt"

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
COPY --from=builder /app/target/dx/nekotab/release/web/ /app/

# ICON_DIR 与数据一起放挂载卷：图标是落盘缓存的字节，容器重建后不必重新代取。
# WALLPAPER_DIR 更得放卷里——那是用户上传的数据，丢了没法重新生成。
ENV DATABASE_URL="sqlite:///data/mtab.db?mode=rwc" \
    ICON_DIR="/data/icon" \
    WALLPAPER_DIR="/data/wallpaper" \
    IP=0.0.0.0 \
    PORT=8080

VOLUME /data
EXPOSE 8080

# dx build --platform web 产出的服务端二进制名为 `server`（不是包名 nekotab），
# 实测位于 web/ 目录根部，与 public/ 同级。
CMD ["/app/server"]
