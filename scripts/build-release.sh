#!/usr/bin/env bash
#
# Description:  Builds the release binary, statically linked where the platform allows it, and
#               reports what came out. Extra arguments are passed through to cargo.
# Author:       Alex Wu
# Dependencies: cargo
# Usage:        scripts/build-release.sh [extra cargo args]
#               scripts/build-release.sh --features unstable-dynamic

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

host="$(rustc -vV | awk '/^host:/ {print $2}')"
ext=""

# crt-static cannot live in .cargo/config.toml: without an explicit --target, cargo applies
# rustflags to host artifacts as well, and clap_derive is a proc macro, which cannot be linked
# statically. Passing --target splits host from target so only the binary gets the flag.
case "$host" in
    *-linux-gnu | *-linux-musl)
        echo "==> building for $host, statically linked"
        static=1
        ;;
    *-windows-msvc)
        # the MSVC CRT is dynamic by default, so this drops the vcredist runtime dependency
        echo "==> building for $host, statically linked against the MSVC CRT"
        static=1
        ext=".exe"
        ;;
    *-windows-gnu*)
        echo "==> building for $host with the default linkage"
        static=0
        ext=".exe"
        ;;
    *-apple-darwin)
        # Apple ships no static libSystem and does not support statically linked binaries, so
        # there is no equivalent of the Linux win to be had here
        echo "==> building for $host, dynamically linked (macOS has no static libc)"
        static=0
        ;;
    *)
        echo "==> building for $host with the default linkage"
        static=0
        ;;
esac

if [ "$static" = 1 ]; then
    RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static" \
        cargo build --release -p termidex --target "$host" "$@"
    binary="target/$host/release/termidex$ext"
else
    cargo build --release -p termidex "$@"
    binary="target/release/termidex$ext"
fi

echo
echo "built $binary"
if command -v stat >/dev/null; then
    echo "  size: $(wc -c < "$binary") bytes"
fi

# ldd on Linux, otool on macOS, and nothing worth printing elsewhere
if command -v ldd >/dev/null; then
    echo "  linkage: $(ldd "$binary" 2>&1 | head -1 | sed 's/^[[:space:]]*//')"
elif command -v otool >/dev/null; then
    echo "  linkage: $(otool -L "$binary" 2>/dev/null | sed -n '2p' | sed 's/^[[:space:]]*//')"
fi

if COMPLETE=bash "$binary" >/dev/null 2>&1; then
    echo "  completion: built in, register it with: source <(COMPLETE=bash $binary)"
else
    echo "  completion: not built in, add --features unstable-dynamic for it"
fi
