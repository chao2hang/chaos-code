# Clean-Linux build/test image for the gates documented in CONTRIBUTING.md.
#
# It exists because a developer machine accumulates things a fresh clone does
# not have: a warm target/ directory, a globally installed protoc, a Rust
# toolchain installed by hand, a system ripgrep, an OS trust store that already
# contains the right roots. `scripts/verify-in-docker.sh` runs the same command
# sequence as the `rust` job in `.github/workflows/ci.yml` inside this image, so
# a documented step that only works by accident on one machine fails here.
#
# The toolchain is installed through rustup from `rust-toolchain.toml` rather
# than taken from a `rust:` image tag. Two reasons: the base image is rebuilt
# rarely while the pinned channel is bumped often, and requiring a `rust:<pin>`
# tag to exist makes the image unbuildable on the day of a bump. Here the pin is
# the only source of truth, and the resulting `rustc -V` is asserted against it
# so a silent fallback to a different compiler cannot pass.
#
# Base image name is an argument because this repo's developers pull through
# registry mirrors with different prefixes.
ARG BASE_IMAGE=docker.io/library/debian:bookworm-slim
FROM ${BASE_IMAGE}

ENV DEBIAN_FRONTEND=noninteractive

# libssl-dev/pkg-config/build-essential mirror the `Install Linux deps` CI step.
# ca-certificates matters more than it looks: the provider adapter validates TLS
# against the OS store plus the Mozilla bundle, so an image without roots fails
# every https endpoint while a developer box passes. cmake and ninja-build are
# needed by crate build scripts that a prebuilt `rust:` image already carries.
# python3 and git are consumed by the guard scripts and by tests that shell out.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        pkg-config \
        libssl-dev \
        ca-certificates \
        ripgrep \
        python3 \
        git \
        curl \
        wget \
        cmake \
        ninja-build \
        unzip \
        xz-utils \
    && rm -rf /var/lib/apt/lists/*

# `bin/protoc` is a dotslash launcher, not a binary; apt's protobuf-compiler is
# too old for proto3 optional. Mirrors the `Install dotslash` CI step.
RUN curl -fsSL \
        https://github.com/facebook/dotslash/releases/download/v0.5.3/dotslash-linux-musl.x86_64.v0.5.3.tar.gz \
        -o /tmp/dotslash.tar.gz \
    && tar -xzf /tmp/dotslash.tar.gz -C /tmp \
    && install -m 0755 /tmp/dotslash /usr/local/bin/dotslash \
    && rm -f /tmp/dotslash /tmp/dotslash.tar.gz

ENV CARGO_HOME=/usr/local/cargo
ENV RUSTUP_HOME=/usr/local/rustup
ENV PATH="${CARGO_HOME}/bin:${PATH}"

# The pinned channel is resolved exactly the way a checkout resolves it.
#
# Installing with `--profile minimal` was the first attempt here and it was
# wrong: `rust-toolchain.toml` asks for `profile = "default"` plus five cross
# targets, so rustup considered the installed toolchain incomplete and re-synced
# it from the network on *every* cargo invocation -- roughly two minutes per
# gate step. Resolving the manifest once, inside a directory that carries a copy
# of it, makes the checkout-time resolution a no-op.
COPY rust-toolchain.toml /tmp/rust-toolchain.toml
RUN set -eu; \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup.sh; \
    sh /tmp/rustup.sh -y --no-modify-path --default-toolchain none; \
    rm -f /tmp/rustup.sh; \
    channel="$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' \
        /tmp/rust-toolchain.toml)"; \
    test -n "$channel" || { echo "no channel pinned in rust-toolchain.toml" >&2; exit 1; }; \
    mkdir -p /opt/toolchain-pin; \
    cp /tmp/rust-toolchain.toml /opt/toolchain-pin/rust-toolchain.toml; \
    printf '[package]\nname = "toolchain-pin"\nversion = "0.0.0"\nedition = "2024"\n' \
        > /opt/toolchain-pin/Cargo.toml; \
    # Running cargo here is what performs the sync; a bare `rustup toolchain
    # install` would not read the profile or target list from the manifest.
    cd /opt/toolchain-pin; \
    cargo -V; \
    rustc -V; \
    # Keep an explicit default so the image is also usable outside a checkout.
    rustup default "$channel"; \
    rustfmt --version >/dev/null; \
    cargo clippy --version >/dev/null; \
    # Without this check a rustup that failed to switch would keep the previous
    # compiler and every result below would describe a different build.
    test "$(rustc -V | cut -d' ' -f2)" = "$channel" \
        || { echo "toolchain drift: rustc -V is not $channel" >&2; exit 1; }

# Test-thread stack for the large xai-grok-shell actor tests, same as CI.
ENV RUST_MIN_STACK=16777216

WORKDIR /src
CMD ["bash"]
