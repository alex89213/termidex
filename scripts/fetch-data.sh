#!/usr/bin/env bash
#
# Description:  Checks out the pinned PokeAPI submodules with shallow sparse clones.
#               Only the endpoints and sprite directories termidex-gen reads are checked out.
# Author:       Alex Wu
# Dependencies: git 2.25+ (needs sparse-checkout)
# Usage:        scripts/fetch-data.sh

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
vendor="$repo_root/vendor"
mkdir -p "$vendor"

# Configure sparse checkout before the first checkout so unused sprite trees aren't downloaded.
# Existing clones are reused, including older clones with a .git directory rather than a file.
prepare_submodule() {
    local path="$1" dest="$repo_root/$1" mode pin stage recorded_path url
    if ! read -r mode pin stage recorded_path < <(git ls-files --stage -- "$path") \
        || [ "$mode" != 160000 ] || [ "$stage" != 0 ] || [ "$recorded_path" != "$path" ]; then
        echo "error: $path has no recorded submodule commit" >&2
        exit 1
    fi
    if [ -e "$dest/.git" ]; then
        if [ -n "$(git -C "$dest" status --porcelain)" ]; then
            echo "error: $path has local changes; save them before fetching data" >&2
            exit 1
        fi
    else
        url="$(git config --get "submodule.$path.url")"
        echo "==> cloning $path"
        git clone --depth 1 --filter=blob:none --no-checkout "$url" "$dest"
    fi
    if ! git -C "$dest" cat-file -e "$pin^{commit}" 2>/dev/null; then
        echo "==> fetching pinned commit for $path"
        git -C "$dest" fetch --depth 1 origin "$pin"
    fi
}

checkout_submodule() {
    local path="$1" pin
    pin="$(git rev-parse ":$path")"
    echo "==> checking out $path at $pin"
    git -C "$repo_root/$path" -c advice.detachedHead=false checkout --detach "$pin"
    git submodule absorbgitdirs -- "$path"
}

paths=(vendor/api-data vendor/sprites)
git submodule init -- "${paths[@]}"

# Check both existing worktrees before changing either checkout.
for path in "${paths[@]}"; do
    if [ -e "$repo_root/$path/.git" ] && [ -n "$(git -C "$repo_root/$path" status --porcelain)" ]; then
        echo "error: $path has local changes; save them before fetching data" >&2
        exit 1
    fi
done

prepare_submodule vendor/api-data
echo "==> selecting api-data endpoints"
git -C "$vendor/api-data" sparse-checkout set \
    data/api/v2/pokemon \
    data/api/v2/pokemon-species \
    data/api/v2/pokemon-form \
    data/api/v2/version-group \
    data/api/v2/pokedex \
    data/api/v2/region
checkout_submodule vendor/api-data

prepare_submodule vendor/sprites
echo "==> selecting sprite directories"
# --no-cone so the patterns can pick single files out of a directory without pulling
# the versions/ and other/ trees that live alongside them
git -C "$vendor/sprites" sparse-checkout set --no-cone \
    '/sprites/pokemon/*.png' \
    '/sprites/pokemon/shiny/*.png' \
    '/sprites/pokemon/female/*.png' \
    '/sprites/pokemon/shiny/female/*.png'
checkout_submodule vendor/sprites

echo
echo "==> spot checks"
fail=0
for f in \
    "$vendor/sprites/sprites/pokemon/6.png" \
    "$vendor/sprites/sprites/pokemon/shiny/6.png" \
    "$vendor/sprites/sprites/pokemon/female/25.png" \
    "$vendor/sprites/sprites/pokemon/10034.png" \
    "$vendor/api-data/data/api/v2/pokemon/25/index.json" \
    "$vendor/api-data/data/api/v2/pokemon-species/25/index.json" \
    "$vendor/api-data/data/api/v2/pokemon-form/25/index.json" \
    "$vendor/api-data/data/api/v2/version-group/17/index.json" \
    "$vendor/api-data/data/api/v2/pokedex/16/index.json" \
    "$vendor/api-data/data/api/v2/region/7/index.json"
do
    if [ -s "$f" ]; then
        echo "  ok      $f"
    else
        echo "  MISSING $f"
        fail=1
    fi
done

echo
echo "  default sprites: $(find "$vendor/sprites/sprites/pokemon" -maxdepth 1 -name '*.png' | wc -l)"
echo "  shiny sprites:   $(find "$vendor/sprites/sprites/pokemon/shiny" -maxdepth 1 -name '*.png' 2>/dev/null | wc -l)"
echo "  female sprites:  $(find "$vendor/sprites/sprites/pokemon/female" -maxdepth 1 -name '*.png' 2>/dev/null | wc -l)"
echo "  species records: $(find "$vendor/api-data/data/api/v2/pokemon-species" -name index.json | wc -l)"
echo "  pokemon records: $(find "$vendor/api-data/data/api/v2/pokemon" -maxdepth 2 -name index.json | wc -l)"
echo "  form records:    $(find "$vendor/api-data/data/api/v2/pokemon-form" -name index.json | wc -l)"
echo "  version groups:  $(find "$vendor/api-data/data/api/v2/version-group" -name index.json | wc -l)"
echo "  pokedexes:       $(find "$vendor/api-data/data/api/v2/pokedex" -name index.json | wc -l)"
echo "  regions:         $(find "$vendor/api-data/data/api/v2/region" -name index.json | wc -l)"

if [ "$fail" -ne 0 ]; then
    echo
    echo "some expected files are missing, the sparse patterns need adjusting" >&2
    exit 1
fi

echo
echo "done. next: cargo run -p termidex-gen --release --locked"
