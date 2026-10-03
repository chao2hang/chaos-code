# Cross image for docs/verification/model-path-windows-probe.rs: the pinned
# toolchain, a mingw C toolchain for the windows-gnu target, and wine to run the
# result. Build context is the directory holding this file.
FROM docker.m.daocloud.io/library/rust:1.94.0-bookworm

# gcc-mingw-w64-x86-64 is the linker rustc wants for x86_64-pc-windows-gnu, and
# wine64 is the runner. Both are --no-install-recommends on purpose: the
# recommended set adds ~370 MB of audio and video codecs that a std-only binary
# never loads.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      gcc-mingw-w64-x86-64 wine64 \
 && rm -rf /var/lib/apt/lists/*

RUN rustup target add x86_64-pc-windows-gnu

# Debian's wine64 installs no `wine` wrapper on PATH; the entry point is
# /usr/lib/wine/wine64, which is what the recorded runs invoke. Printed because a
# build step that probed for `wine` on PATH died on exit 127.
RUN (dpkg -L wine64 | grep -E "bin|wine64$") || true

ENV WINEDEBUG=-all
ENV WINEPREFIX=/tmp/wineprefix
