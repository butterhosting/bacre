default:
    @just --list

dev:
    #!/usr/bin/env bash
    set -euo pipefail
    # Ctrl-C ends both
    trap 'kill 0' EXIT
    cargo build
    (cd website && bun install && bun run dev) &
    cargo watch --quiet --watch src --watch Cargo.toml --watch build.rs --exec "run -- dev/config.yaml" &
    wait

build: build-website
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p dist
    for pair in x86_64-unknown-linux-musl:amd64 aarch64-unknown-linux-musl:arm64; do
        target="${pair%%:*}"
        cargo build --release --target "$target"
        cp "target/$target/release/bacre" "dist/bacre-linux-${pair##*:}"
    done
    ls -lh dist

build-native: build-website
    cargo build --release

build-website:
    cd website && bun install --frozen-lockfile && bun run build

check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test
    cd website && bun install && bun run lint
