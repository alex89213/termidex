#!/usr/bin/env bash
#
# Description:  Type-checks termidex for Linux, Windows and macOS, with and without the completion
#               feature. Catches breakage in cfg-gated code that a normal build never compiles,
#               such as the #[cfg(windows)] ANSI setup.
# Author:       Alex Wu
# Dependencies: cargo, rustup (to install the std for each target)
# Usage:        scripts/check-targets.sh

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

targets=(
    x86_64-unknown-linux-gnu
    x86_64-pc-windows-msvc
    aarch64-apple-darwin
)

missing=()
for t in "${targets[@]}"; do
    rustup target list --installed 2>/dev/null | grep -qx "$t" || missing+=("$t")
done
if [ ${#missing[@]} -gt 0 ]; then
    echo "==> installing the std for: ${missing[*]}"
    rustup target add "${missing[@]}"
fi

fail=0
for t in "${targets[@]}"; do
    for features in "" "unstable-dynamic"; do
        label="$t${features:+ +$features}"
        printf '%-52s ' "$label"
        args=(check -p termidex --target "$t")
        [ -n "$features" ] && args+=(--features "$features")
        if cargo "${args[@]}" >/dev/null 2>&1; then
            echo "ok"
        else
            echo "FAILED"
            fail=1
        fi
    done
done

echo
if [ "$fail" -ne 0 ]; then
    echo "some targets do not compile, rerun one of them without the output redirect to see why" >&2
    exit 1
fi
echo "all targets type-check"
