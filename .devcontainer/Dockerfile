# Dev container для разработки cosmos-checker.
# ВНИМАНИЕ: итоговая Windows-сборка (.exe/.msi) выполняется НЕ в этом контейнере,
# а на Windows-хосте через `cargo tauri build`. Этот образ — для unit/integration
# тестов, линтеров, frontend-разработки и Linux-preview сборок.

FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

ARG DEBIAN_FRONTEND=noninteractive

# Системные зависимости для Tauri (Linux preview) + protoc для tonic gRPC codegen.
RUN apt-get update && apt-get install -y --no-install-recommends \
        libwebkit2gtk-4.1-dev \
        libgtk-3-dev \
        libayatana-appindicator3-dev \
        librsvg2-dev \
        libssl-dev \
        pkg-config \
        build-essential \
        curl \
        file \
        wget \
        protobuf-compiler \
        libprotobuf-dev \
    && rm -rf /var/lib/apt/lists/*

# rustfmt и clippy
RUN rustup component add rustfmt clippy

# pnpm ставится через feature node + ручной install-g (postCreateCommand),
# cargo tauri-cli — там же.

USER vscode
WORKDIR /workspaces/cosmos-checker
