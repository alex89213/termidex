# termidex

Print Pokémon sprites and Pokédex details in your terminal.

The included data covers 1,025 species and 1,530 entries, including alternate varieties and
forms with front sprites in PokéAPI. Sprites and data are built into the executable.
Running termidex needs no network connection or external data files.

```sh
termidex pikachu
termidex random --name
termidex charizard --form mega-x --name --dex --stats
```

## Install

Requires Rust 1.85 or newer and a terminal that supports ANSI colours and Unicode block characters.
No Nerd Font is needed.

```sh
git clone https://github.com/alex89213/termidex.git
cd termidex
cargo install --path crates/termidex --locked
```

Run the Cargo command from the repository root. No PokéAPI download or data generation is needed.
The source submodules can stay uninitialized for normal builds and installs.
Cargo may download dependencies on the first build. Once the dependencies for your target and
features are cached, add `--offline` to build without a network connection.

### Build a release executable

```sh
cargo build --release -p termidex --locked
```

This produces `target/release/termidex` (`termidex.exe` on Windows) using the platform's default
linking settings. To build with static linking where supported:

```sh
scripts/build-release.sh --locked
```

The script builds for the current Rust host target and prints the executable's path. It builds
termidex only, not the data generator. Extra arguments are passed to Cargo.

| Rust host target | Linking | Output directory |
|---|---|---|
| Linux GNU or musl | static C runtime | `target/<host-target>/release/` |
| Windows MSVC | static MSVC C runtime | `target/<host-target>/release/` |
| macOS | dynamic | `target/release/` |
| Windows GNU and other targets | platform default | `target/release/` |

For example, on x86-64 Linux with the GNU toolchain:

```sh
./target/x86_64-unknown-linux-gnu/release/termidex pikachu
```

The scripts need Bash. On Windows, use Git Bash with a Windows Rust toolchain, or build directly
from PowerShell. A build under WSL produces a Linux executable.

For static MSVC runtime linking in PowerShell:

```powershell
$env:RUSTFLAGS="-C target-feature=+crt-static"
cargo build --release -p termidex --locked --target x86_64-pc-windows-msvc
```

## Usage

Supply a species name, `random`, or `list`. Use `termidex --help`, `termidex random --help`, or
`termidex list --help` for the available options.

### Names and alternate forms

Names ignore case, spaces, hyphens, dots, and apostrophes. The `é` in a name can be written as `e`.
These all select the same species:

```sh
termidex "Mr. Mime"
termidex mr-mime
termidex mrmime
termidex MR.MIME
```

`nidoran-f` and `nidoran♀` select Nidoran♀; `nidoran-m` and `nidoran♂` select Nidoran♂.

A species name selects its default entry. Use `--form` for an alternate:

```sh
termidex charizard --form mega-x
termidex raichu --form alolan
termidex deoxys --form attack
termidex arceus --form bug
termidex charizard --form gigantamax
```

`--form` accepts both PokéAPI varieties, which have their own Pokémon records, and cosmetic forms.
It also accepts these aliases: `alolan` → `alola`, `galarian` → `galar`, `hisuian` → `hisui`,
`paldean` → `paldea`, and `gigantamax` → `gmax`.

List the available alternates with `--list`:

```text
$ termidex charizard --list
mega-x (Mega Charizard X)
mega-y (Mega Charizard Y)
gmax (Gigantamax Charizard)
```

Alternate slugs such as `deoxys-attack` are not accepted as species names. The error message gives
the matching command, in this case `termidex deoxys --form attack`.

### Display flags

Without display flags, termidex prints only the sprite.

| Flag | Effect |
|---|---|
| `--name` | print the display name, with an added ` ♀` for a distinct female sprite and ` ★` for a shiny sprite |
| `--dex` | print genus, types, height, and weight in metric and imperial units |
| `--entry` | print the English Pokédex text, wrapped at 20–80 columns based on terminal width |
| `--stats` | print base-stat bars |
| `--female` | select the female sprite when a separate one is available |

Output always follows this order: sprite, name, dex details, Pokédex text, then stat bars.
`--list` cannot be combined with `--form` or display flags.

For `random`, put display flags after the subcommand: `termidex random --name`.
Global `--seed` and `--shiny-rate` options work before or after a subcommand.

### Female sprites

```sh
termidex pikachu --female --name
termidex nidoqueen --female --name
```

Pikachu's female sprite has a different tail, so its name gets an added ` ♀`.
Female-only species such as Nidoqueen never get an added marker. The symbol in the official name
Nidoran♀ stays part of that name.

The marker compares the ordinary sprites for ordinary output and the shiny sprites for shiny
output. The included data has 96 entries with separate, distinct female sprites. When no separate
female sprite is available, `--female` shows the default sprite and prints the reason to stderr.

Some species have separate male and female entries: Frillish, Jellicent, Pyroar, Meowstic,
Indeedee, Basculegion, and Oinkologne. Use `--form female` to select their female entry and its stats:

```sh
termidex pyroar --form female --name
termidex meowstic --form female-mega --name
```

In name mode, `--female` does not switch to a separate entry. For example, `termidex pyroar --female`
keeps Male Pyroar and prints a note pointing to `--form female`.

### Random selection

```sh
termidex random
termidex random --gen 1-7
termidex random --gen 1 2 5
termidex random --include-varieties
termidex random --include-forms
termidex random --battle-forms
termidex random --region alola
```

The default pool starts with one entry per species. These flags add alternates:

| Flag | Adds | Entries in the included data |
|---|---|---:|
| `--include-varieties` | alternate Pokémon records, such as regional variants and Deoxys formes | 163 |
| `--include-forms` | cosmetic forms, such as Unown letters and Arceus types | 186 |
| `--battle-forms` | entries marked as battle-only in PokéAPI, such as Mega, Primal, and Gigantamax forms | 156 |

These three groups do not overlap. Battle forms are excluded unless `--battle-forms` is supplied,
including when `--region` is used. Named commands can select them at any time.

`--gen` and `--region` use the games that introduced each entry, as recorded in the included data:

| Entry | Generation | Region |
|---|---:|---|
| Charizard | 1 | kanto |
| Mega Charizard X | 6 | kalos |
| Gigantamax Charizard | 8 | galar |
| Alolan Raichu | 7 | alola |
| Mega Raichu X | 9 | kalos |
| Primal Kyogre | 6 | hoenn |

For example, `random --gen 1` excludes Mega Charizard X. To include it in a Generation 6 pool,
use `random --gen 6 --battle-forms`.

A region filter includes matching default entries and alternates without needing an include flag.
It cannot be combined with `--include-varieties` or `--include-forms`. Generation and region filters
can be combined:

```sh
termidex random --gen 6 --region hoenn --battle-forms
```

Available regions are `kanto`, `johto`, `hoenn`, `sinnoh`, `unova`, `kalos`, `alola`, `galar`,
`hisui`, and `paldea`. Available generations are 1 through 9.

Random mode uses the species' female ratio to select a female appearance where available.
`--female` forces that appearance when possible. For species with separate gender entries, a
female roll selects the female entry if it is not already eligible for a direct draw from the pool.
If both entries are in the pool, they are selected directly without an extra female substitution.

### List species

```sh
termidex list
termidex list --gen 1-3
termidex list --region galar
```

`list` prints one species per line, sorted by National Dex number, followed by its alternate selectors:

```text
0006 charizard  mega-x, mega-y, gmax
0019 rattata  alola
```

`list --gen` filters by the species' generation. `random --gen` filters by the selected entry's
generation. Thus `list --gen 1` includes Charizard and lists its later forms, while `random --gen 1`
only draws entries introduced in Generation 1.

`list --region` includes a species if any of its entries belongs to that region. It lists only the
alternates from that region:

```text
$ termidex list --region kanto | grep 0026
0026 raichu
$ termidex list --region kalos | grep 0026
0026 raichu  mega-x, mega-y
$ termidex list --region alola | grep 0026
0026 raichu  alola
```

`list` includes battle-form selectors without needing `--battle-forms`; that flag belongs to
`random`. Global `--seed` and `--shiny-rate` options have no effect on `list`.

### Shiny sprites and seeds

```sh
termidex pikachu --shiny-rate 1        # always request a shiny sprite
termidex pikachu --shiny-rate 0        # never request a shiny sprite
termidex pikachu                      # default: 1 in 4096
termidex random --seed 12345
```

`--shiny-rate N` gives a one-in-N shiny chance. If the selected gender has no shiny sprite,
termidex shows its ordinary sprite, prints a note to stderr, and omits the star. Mega Venusaur's
female sprite has this limitation in the included data.

With the same program version, options, and terminal settings, `--seed` gives the same output.
Random mode draws the entry, gender, and shiny roll in that order. Name mode draws only the shiny
roll. Updating the data may change seeded results.

### Show a Pokémon when the shell starts

For Fish, add this to `~/.config/fish/config.fish`:

```fish
if status is-interactive
    termidex random --name
end
```

For Bash, add this to `~/.bashrc`:

```bash
[[ $- == *i* ]] && termidex random --name
```

## Colours

Sprites use half-block characters to show two pixels per terminal cell. They are cropped and drawn
at their native size, without scaling.

Truecolour is used when `COLORTERM` is `truecolor` or `24bit`, or when `WT_SESSION` is set
(as it is in Windows Terminal). Otherwise, colours use the nearest xterm-256 palette entry.

A nonempty `NO_COLOR` disables colour in text, including types and stat bars. Sprites remain coloured.

Stat bars use the same scale for every stat, with a full bar representing 255:

| Base stat | Colour |
|---|---|
| 0–29 | red |
| 30–59 | orange |
| 60–89 | yellow |
| 90–119 | green |
| 120–149 | dark green |
| 150–255 | turquoise |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | lookup, data, or output error |
| 2 | usage error |

Notes and errors go to stderr. Normal output goes to stdout. A closed output pipe is handled quietly.

## Shell completion

Completion is optional because it uses Clap's unstable completion engine. Install it with:

```sh
cargo install --path crates/termidex --locked --features unstable-dynamic
```

Or build a release executable with completion and static linking where supported:

```sh
scripts/build-release.sh --locked --features unstable-dynamic
```

The following commands use the installed executable. Add the line for your shell to its startup file:

```bash
source <(COMPLETE=bash termidex)      # ~/.bashrc
```

```zsh
source <(COMPLETE=zsh termidex)       # ~/.zshrc
```

```fish
COMPLETE=fish termidex | source      # ~/.config/fish/config.fish
```

Completion includes species names, flags, subcommands, and the `--form` selectors for a species
already on the command line.

## Data and limitations

Pokédex text and genus belong to the species, so every form of a species shares them.
For example, Alolan Raichu uses Raichu's Pokédex text.

The included data leaves out 49 entries that have no front sprite: Sandy and Trash Mothim,
Scatterbug and Spewpa pattern forms, Mega Zygarde, and Koraidon and Miraidon ride forms.
See [coverage.txt](crates/termidex/generated/coverage.txt) for the full list.

Counts, available forms, and generation/region assignments describe the data shipped with this
version. They may change when the PokéAPI sources are updated.

### Regenerate the data

The PokéAPI sources are pinned Git submodules under `vendor/`. To reproduce the included data:

```sh
scripts/fetch-data.sh
cargo run -p termidex-gen --release --locked
```

The fetch script initializes [PokeAPI/api-data](https://github.com/PokeAPI/api-data) and
[PokeAPI/sprites](https://github.com/PokeAPI/sprites) at the commits recorded by termidex.
It does not advance them to the latest upstream versions. It makes shallow clones with Git's blob
filter and configures sparse checkouts before the first checkout. It selects six JSON endpoints
(`pokemon`, `pokemon-species`, `pokemon-form`, `version-group`, `pokedex`, and `region`) and the
ordinary, shiny, female, and shiny-female front sprite directories. Existing clones are reused;
local changes stop the script rather than being discarded.

You can also use `git submodule update --init -- vendor/api-data vendor/sprites` to check out the
pinned sources, but a fresh checkout through that command downloads their full working trees.
Use the fetch script to keep the downloads smaller. No submodules are needed to run termidex.

The generator writes these files under `crates/termidex/generated/`:

| File | Contents |
|---|---|
| `data.rs` | Pokémon tables and name lookup maps |
| `sprites.bin` | compressed sprite data, with duplicate images stored once |
| `coverage.txt` | counts, excluded entries, missing sprites, and metadata to review |

The current sprite blob is about 3.5 MB. The generator checks names, default entries, referenced
records, stats, type slots, measurements, and sprite paths before writing the tables and blob.
Review the generated diff and coverage report before committing an update.

### Update the source revisions

To use the latest upstream data, update the submodules explicitly, then regenerate:

```sh
git submodule update --init --remote --checkout -- vendor/api-data vendor/sprites
cargo run -p termidex-gen --release --locked
git diff --submodule
git add vendor/api-data vendor/sprites crates/termidex/generated
git commit -m "Update PokéAPI data"
```

Initialize the sources with `scripts/fetch-data.sh` first to keep sparse checkouts. Review the new
source commits and generated files before committing them together. Running the fetch script
before staging an update returns the checkouts to the previously recorded commits.

## Development checks

Run from a checkout:

```sh
scripts/check.sh
scripts/check-targets.sh
```

`check.sh` checks formatting, runs Clippy with warnings treated as errors, runs workspace tests with
default and all features, and builds a native release offline.

`check-targets.sh` type-checks termidex for x86-64 Linux GNU, x86-64 Windows MSVC, and Apple Silicon
macOS, with and without completion. It installs missing Rust target libraries. These checks do not
link executables or run tests on the other platforms.

## Credits and license

Sprites and game data come from [PokéAPI](https://pokeapi.co), through
[PokeAPI/api-data](https://github.com/PokeAPI/api-data) and
[PokeAPI/sprites](https://github.com/PokeAPI/sprites).

Pokémon names and images belong to Nintendo, Game Freak, and Creatures Inc.
This is an unofficial fan project and is not affiliated with or endorsed by them.

termidex's code is [MIT licensed](LICENSE).
