#!/usr/bin/env bash
#
# Description:  Fetches the PokeAPI sprite and JSON repos into vendor/ with shallow sparse clones.
#               Only the endpoints and sprite directories termidex-gen actually reads are pulled.
# Author:       Alex Wu
# Dependencies: git 2.25+ (needs sparse-checkout)
# Usage:        scripts/fetch-data.sh

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
vendor="$repo_root/vendor"
mkdir -p "$vendor"

# a fresh sparse clone, or a re-sync if the clone is already there
clone_sparse() {
    local url="$1" dest="$2"
    if [ -d "$dest/.git" ]; then
        echo "==> updating $dest"
        git -C "$dest" pull --ff-only
    else
        echo "==> cloning $url into $dest"
        git clone --depth 1 --filter=blob:none --sparse "$url" "$dest"
    fi
}

clone_sparse https://github.com/PokeAPI/api-data "$vendor/api-data"
echo "==> selecting api-data endpoints"
git -C "$vendor/api-data" sparse-checkout set \
    data/api/v2/pokemon \
    data/api/v2/pokemon-species \
    data/api/v2/pokemon-form \
    data/api/v2/version-group \
    data/api/v2/pokedex \
    data/api/v2/region

clone_sparse https://github.com/PokeAPI/sprites "$vendor/sprites"
echo "==> selecting sprite directories"
# --no-cone so the patterns can pick single files out of a directory without pulling
# the versions/ and other/ trees that live alongside them
git -C "$vendor/sprites" sparse-checkout set --no-cone \
    '/sprites/pokemon/*.png' \
    '/sprites/pokemon/shiny/*.png' \
    '/sprites/pokemon/female/*.png' \
    '/sprites/pokemon/shiny/female/*.png'

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
echo "done. next: cargo run -p termidex-gen --release"
