//! Turning what the user typed into an entry id.
//!
//! Only species names resolve as names: a species slug or its English name, in any punctuation.
//! Variety and form slugs deliberately do not, because `charizard-mega-x` and
//! `charizard --form mega-x` would otherwise be two ways to say the same thing. Typing one gets a
//! pointer to the right command instead.

use crate::data::{self, ALT_SLUGS, NAMES, SELECTOR_ALIASES};
use crate::normalize::normalize;

/// How many "did you mean" suggestions an unknown name gets.
const MAX_SUGGESTIONS: usize = 3;

/// Below this Jaro-Winkler score a suggestion is more confusing than helpful.
const SUGGESTION_FLOOR: f64 = 0.7;

/// What a typed name turned out to be.
pub enum Resolved {
    /// A species, giving its default entry id.
    Entry(u32),
    /// A variety or form slug, which needs `--form` instead.
    AltSlug {
        species: u32,
        /// Empty when the slug names the species' default entry.
        selector: &'static str,
    },
    /// Nothing matched, with the closest species slugs.
    Unknown(Vec<&'static str>),
}

/// Resolves a typed Pokémon name.
///
/// # Arguments
/// * `input` - whatever the user typed, punctuation and case included
///
/// # Returns
/// the matching entry, a pointer to the right `--form` command, or suggestions
pub fn resolve_name(input: &str) -> Resolved {
    let key = normalize(input);
    if let Some(id) = NAMES.get(key.as_str()) {
        return Resolved::Entry(*id);
    }
    if let Some((species, selector)) = ALT_SLUGS.get(key.as_str()) {
        return Resolved::AltSlug {
            species: *species,
            selector,
        };
    }
    Resolved::Unknown(suggest(&key))
}

/// Finds the species slugs closest to a normalized key.
///
/// # Arguments
/// * `key` - the normalized, unmatched input
///
/// # Returns
/// up to three species slugs, best first
fn suggest(key: &str) -> Vec<&'static str> {
    if key.is_empty() {
        return Vec::new();
    }
    // score every lookup key, then collapse to species so "mrmime" and "mr-mime" do not both show
    let mut scored: Vec<(f64, &'static str)> = Vec::new();
    for (candidate, entry_id) in NAMES.entries() {
        let score = strsim::jaro_winkler(key, candidate);
        if score < SUGGESTION_FLOOR {
            continue;
        }
        let slug = data::entry(*entry_id).species().slug;
        match scored.iter_mut().find(|(_, s)| *s == slug) {
            Some(best) if best.0 < score => best.0 = score,
            Some(_) => {}
            None => scored.push((score, slug)),
        }
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(b.1)));
    scored.truncate(MAX_SUGGESTIONS);
    scored.into_iter().map(|(_, slug)| slug).collect()
}

/// Resolves a `--form` selector within one species.
///
/// Accepts a variety selector and a form selector alike, because which one a given alternate is
/// depends on PokeAPI's own modelling and the user has no reason to care.
///
/// # Arguments
/// * `species_idx` - the species to look inside
/// * `input` - the selector as typed
///
/// # Returns
/// the entry id, or `None` when the species has no such selector
pub fn resolve_selector(species_idx: u32, input: &str) -> Option<u32> {
    let key = apply_aliases(&normalize(input));
    let selectors = data::species(species_idx).selectors();
    selectors
        .binary_search_by(|(k, _)| (*k).cmp(key.as_str()))
        .ok()
        .map(|i| selectors[i].1)
}

/// Finds the separate entry PokeAPI keeps for this entry's female, when the species splits
/// gender into its own forms, like Pyroar, Meowstic or Indeedee.
///
/// Those species name their default entry "Male X", and the female can have its own stats. The
/// generator drops the male entry's own female sprite, so `--form female` is the one way to ask for
/// it by name, and a female roll in `random` shows this entry instead.
///
/// # Arguments
/// * `entry_id` - the entry being shown
///
/// # Returns
/// the female entry's id, or `None` when the species keeps both genders on one entry
pub fn female_counterpart(entry_id: u32) -> Option<u32> {
    let entry = data::entry(entry_id);
    // the default entry is the male, and a male alternate like male-mega pairs with female-mega
    let key = if entry.is_default() {
        "female".to_string()
    } else {
        format!("female{}", normalize(entry.selector).strip_prefix("male")?)
    };
    resolve_selector(entry.species, &key).filter(|id| *id != entry_id)
}

/// Reports whether this entry is itself the female of a species split by gender.
pub fn is_female_counterpart(entry_id: u32) -> bool {
    !data::entry(entry_id).is_default()
        && normalize(data::entry(entry_id).selector).starts_with("female")
}

/// Rewrites the spellings people reach for into the ones PokeAPI uses.
///
/// `alolan-raichu` and `gigantamax` are what a player would say, while the data says `alola` and
/// `gmax`. The rewrite is a prefix swap so compound selectors like `galarian-standard` work too.
///
/// # Arguments
/// * `key` - an already normalized selector
///
/// # Returns
/// the key with any alias prefix swapped for its canonical form
fn apply_aliases(key: &str) -> String {
    for (from, to) in SELECTOR_ALIASES {
        if let Some(rest) = key.strip_prefix(from) {
            return format!("{to}{rest}");
        }
    }
    key.to_string()
}

/// Lists the selectors a species accepts, for the error message and the completion script.
///
/// # Arguments
/// * `species_idx` - the species to list
///
/// # Returns
/// the raw (not normalized) selectors in game order
pub fn available_selectors(species_idx: u32) -> Vec<&'static str> {
    let species = data::species(species_idx);
    species
        .entry_ids()
        .map(data::entry)
        .filter(|e| !e.is_default())
        .map(|e| e.selector)
        .collect()
}
