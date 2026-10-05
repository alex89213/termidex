//! Builds termidex's committed data tables from the vendored PokeAPI checkouts.
//!
//! Run it after `scripts/fetch-data.sh`:
//!
//! ```sh
//! cargo run -p termidex-gen --release
//! ```
//!
//! Everything it can check, it checks, and it fails loudly rather than guessing. Anything a human
//! should look over lands in `crates/termidex/generated/coverage.txt`.

mod api;
mod emit;
mod model;
mod sprite;

// the runtime's own normalizer, compiled straight into the generator so the two can never
// disagree about what a lookup key looks like
#[path = "../../termidex/src/normalize.rs"]
mod normalize;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("termidex-gen: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let root = repo_root();
    let vendor = root.join("vendor");
    let out_dir = root.join("crates/termidex/generated");

    let started = Instant::now();
    eprintln!("reading {}", vendor.display());
    let data = api::Dataset::load(&vendor)?;
    eprintln!(
        "  {} species, {} pokemon, {} forms",
        data.species.len(),
        data.pokemon.len(),
        data.forms.len()
    );

    let model = model::build(&data)?;
    eprintln!(
        "  {} species kept, {} entries, {} types",
        model.species.len(),
        model.entries.len(),
        model.types.len()
    );

    let mut model = model;
    model::drop_females_with_own_entry(&mut model);

    eprintln!("encoding sprites");
    let mut blob = sprite::SpriteBlob::new();
    let mut dedupe: HashMap<Vec<u8>, u32> = HashMap::new();
    let mut slots = Vec::with_capacity(model.entries.len() * 4);
    for entry in &model.entries {
        for path in &entry.sprite_paths {
            slots.push(blob.add(&data.sprites_root, path, &mut dedupe)?);
        }
    }
    eprintln!(
        "  {} unique sprites, {} KiB, largest palette {}",
        blob.directory.len(),
        blob.bytes.len() / 1024,
        blob.max_palette
    );

    model::drop_indistinguishable_females(&mut model, &slots);
    eprintln!(
        "  {} entries keep a distinct female sprite, {} had one that looks the same",
        model
            .entries
            .iter()
            .filter(|e| e.flags & model::F_HAS_FEMALE != 0)
            .count(),
        model.female_same_as_default.len()
    );

    emit::write_data(&out_dir, &model, &blob, &slots)?;
    emit::write_coverage(&out_dir.join("coverage.txt"), &model, &blob)?;
    eprintln!(
        "wrote {} in {:.1}s",
        out_dir.display(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

/// Finds the workspace root, whether the generator is run from the root or from its own crate.
fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at crates/termidex-gen, so climb two levels
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("crate lives two levels under the workspace root")
}
