//! Dynamic shell completion, behind the `unstable-dynamic` feature.
//!
//! Species names come straight out of the lookup table. `--form` selectors are trickier: clap's
//! completer gets the word being completed but not the rest of the command line, so the Pokémon
//! name is recovered from the raw process arguments. During a completion request the shell invokes
//! the binary as `termidex -- termidex <typed words...>`, so everything after the `--` is the line
//! the user is in the middle of typing.

use std::ffi::OsString;

use clap_complete::engine::{ArgValueCandidates, CompletionCandidate};
use clap_complete::CompleteEnv;

use crate::cli::Cli;
use crate::data;
use crate::lookup;

/// Long flags that swallow the next word, so it is a value rather than the Pokémon name.
const VALUE_FLAGS: [&str; 5] = ["--form", "--shiny-rate", "--seed", "--gen", "--region"];

/// The variable a shell sets to ask for completion rather than a sprite.
const COMPLETE_VAR: &str = "COMPLETE";

/// Handles a completion request and exits, or returns so the normal run can continue.
///
/// Must be called before anything is written to stdout.
///
/// The variable is checked here rather than left to `CompleteEnv`, which reads the current
/// directory before it looks at the environment. Skipping that keeps an ordinary run from doing
/// any work for a feature it is not using. The saving is below what this can measure, since
/// `getcwd` costs well under a microsecond, so this is tidiness rather than an optimization.
pub fn install() {
    if std::env::var_os(COMPLETE_VAR).is_none_or(|v| v.is_empty()) {
        return;
    }
    CompleteEnv::with_factory(<Cli as clap::CommandFactory>::command).complete();
}

/// Every species name, for completing the positional argument.
pub fn species_names() -> ArgValueCandidates {
    ArgValueCandidates::new(|| {
        (0..data::SPECIES_COUNT as u32)
            .map(|i| CompletionCandidate::new(data::species(i).slug))
            .collect()
    })
}

/// The `--form` selectors of whichever Pokémon is already on the command line.
///
/// Falls back to no candidates when the name has not been typed yet or does not resolve, which
/// leaves the shell to do nothing rather than offer something wrong.
pub fn form_selectors() -> ArgValueCandidates {
    ArgValueCandidates::new(|| {
        let Some(name) = name_from_completion_args() else {
            return Vec::new();
        };
        let lookup::Resolved::Entry(entry_id) = lookup::resolve_name(&name) else {
            return Vec::new();
        };
        let species = data::entry(entry_id).species;
        lookup::available_selectors(species)
            .into_iter()
            .map(CompletionCandidate::new)
            .collect()
    })
}

/// Digs the already-typed Pokémon name out of the raw completion arguments.
///
/// # Returns
/// the name, or `None` when the line has no name yet or names a subcommand instead
fn name_from_completion_args() -> Option<String> {
    let args: Vec<OsString> = std::env::args_os().collect();
    // everything after the escape marker is the line being completed, starting with the program
    let start = args.iter().position(|a| a == "--")? + 2;
    let words = args.get(start..)?;

    let mut skip_next = false;
    for word in words {
        let word = word.to_str()?;
        if skip_next {
            skip_next = false;
            continue;
        }
        if let Some(long) = word.strip_prefix("--") {
            // --form=x carries its value inline, --form x takes the next word
            if !long.contains('=') && VALUE_FLAGS.contains(&word) {
                skip_next = true;
            }
            continue;
        }
        if word.starts_with('-') {
            continue;
        }
        if word == "random" || word == "list" {
            return None;
        }
        return Some(word.to_string());
    }
    None
}
