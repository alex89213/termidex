#!/usr/bin/env bash
#
# Description:  Runs every gate the project holds itself to and fails if any of them does.
#               Exists because printing "ok" after a command whose status was never checked is how
#               a clippy failure got committed once.
# Author:       Alex Wu
# Dependencies: cargo, rustfmt, clippy
# Usage:        scripts/check.sh

set -uo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

fail=0
run() {
    local label="$1"; shift
    printf '%-46s ' "$label"
    if out=$("$@" 2>&1); then
        echo "ok"
    else
        echo "FAILED"
        printf '%s\n' "$out" | tail -20 | sed 's/^/    /'
        fail=1
    fi
}

run "cargo fmt --all --check"      cargo fmt --all --check
run "cargo clippy -D warnings"     cargo clippy --workspace --all-targets --all-features -- -D warnings
run "cargo test default features"  cargo test --workspace
run "cargo test --all-features"    cargo test --workspace --all-features
run "cargo build --release offline" cargo build --release --offline

echo
if [ "$fail" -ne 0 ]; then
    echo "something above failed" >&2
    exit 1
fi
echo "all gates pass"
