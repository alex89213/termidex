//! Turns the raw PokeAPI records into termidex's flat species and entry tables.
//!
//! A displayable "entry" is one (variety, form) pair. Every rule in here mirrors PokeAPI rather
//! than guessing: types come from the form when the form has its own, stats and measurements come
//! from the variety, and sprite files are only ever named by the JSON.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::api::{
    english_genus, english_name, latest_english_flavor, url_id, Dataset, FormJson, PokemonJson,
    SpeciesJson,
};
use crate::normalize;

/// Selector prefixes a user may type that mean the same thing as a shorter one.
/// Applied to the normalized input at lookup time, longest source first.
pub const SELECTOR_ALIASES: [(&str, &str); 5] = [
    ("gigantamax", "gmax"),
    ("alolan", "alola"),
    ("galarian", "galar"),
    ("hisuian", "hisui"),
    ("paldean", "paldea"),
];

/// Stat slugs in the order termidex displays them.
const STAT_ORDER: [&str; 6] = [
    "hp",
    "attack",
    "defense",
    "special-attack",
    "special-defense",
    "speed",
];

/// Names that would shadow a subcommand if a species ever used them.
const RESERVED_NAMES: [&str; 3] = ["random", "list", "help"];

pub const F_HAS_FEMALE: u8 = 1 << 0;
pub const F_NO_SHINY: u8 = 1 << 1;
pub const F_BATTLE_ONLY: u8 = 1 << 2;
pub const F_DEFAULT_VARIETY: u8 = 1 << 3;
pub const F_DEFAULT_FORM: u8 = 1 << 4;
pub const F_NO_SHINY_FEMALE: u8 = 1 << 5;

/// One displayable (variety, form) pair.
#[derive(Debug)]
pub struct Entry {
    pub species_idx: u32,
    pub display_name: String,
    /// Empty for the species' default entry, otherwise the slug with the species prefix removed.
    pub selector: String,
    pub types: [u8; 2],
    pub stats: [u8; 6],
    pub height: u16,
    pub weight: u16,
    pub flags: u8,
    /// The generation that introduced this entry, which is not always its species'.
    /// Mega Beedrill is Gen 6 on a Gen 1 Pokémon, Mega Raichu is Gen 9 on one.
    pub generation: u8,
    /// 0 when the entry could not be placed, otherwise the `REGIONS` index plus one.
    pub region: u8,
    /// Repo-relative sprite paths for slots `[default, female, shiny, shiny_female]`.
    pub sprite_paths: [String; 4],
    /// Full variety or form slug, kept for the "did you mean --form" error path.
    pub full_slug: String,
}

impl Entry {
    /// Reports whether this is its species' default variety and default form.
    pub fn is_default_entry(&self) -> bool {
        self.flags & (F_DEFAULT_VARIETY | F_DEFAULT_FORM) == (F_DEFAULT_VARIETY | F_DEFAULT_FORM)
    }
}

/// One Pokémon species with the range of entries that belong to it.
#[derive(Debug)]
pub struct Species {
    pub slug: String,
    pub name: String,
    pub genus: String,
    pub flavor: Option<String>,
    pub dex: u16,
    pub generation: u8,
    pub gender_rate: i8,
    pub default_entry: u32,
    pub entry_start: u32,
    pub entry_end: u32,
    /// Normalized selector to entry id, sorted by key so the runtime can binary search.
    pub selectors: Vec<(String, u32)>,
}

/// Everything the emitter needs, plus the notes that go into `coverage.txt`.
#[derive(Debug)]
pub struct Model {
    pub species: Vec<Species>,
    pub entries: Vec<Entry>,
    pub types: Vec<String>,
    pub generations: Vec<u8>,
    /// Slug plus reason, for entries dropped because no sprite exists.
    pub excluded: Vec<(String, String)>,
    pub no_shiny: Vec<String>,
    pub no_shiny_female: Vec<String>,
    pub no_flavor: Vec<String>,
    /// Region names in PokeAPI id order, keeping only those an entry actually belongs to.
    pub regions: Vec<String>,
    /// Entry slug, region, and the games it came from, for review.
    pub regional: Vec<(String, String, String)>,
    /// Entries whose games map to no region at all.
    pub region_unknown: Vec<String>,
    /// Entries whose own generation differs from their species'.
    pub generation_differs: Vec<(String, u8, u8)>,
    pub unprefixed: Vec<String>,
    pub odd_keys: Vec<String>,
    /// Species where `has_gender_differences` disagrees with the sprites actually present.
    pub gender_mismatch: Vec<String>,
    /// Entries PokeAPI gives a female sprite that turns out to look the same as the default.
    pub female_same_as_default: Vec<String>,
    /// Entries whose female is its own entry, so their borrowed female sprite is dropped.
    pub female_is_own_entry: Vec<String>,
}

/// Builds the whole model from the vendored records.
///
/// # Arguments
/// * `data` - the loaded PokeAPI records
///
/// # Returns
/// the model, or an error message for anything the data contract forbids
pub fn build(data: &Dataset) -> Result<Model, String> {
    let mut type_ids: BTreeMap<String, u8> = BTreeMap::new();
    let mut m = Model {
        species: Vec::new(),
        entries: Vec::new(),
        types: Vec::new(),
        generations: Vec::new(),
        excluded: Vec::new(),
        no_shiny: Vec::new(),
        no_shiny_female: Vec::new(),
        no_flavor: Vec::new(),
        regions: Vec::new(),
        regional: Vec::new(),
        region_unknown: Vec::new(),
        generation_differs: Vec::new(),
        unprefixed: Vec::new(),
        odd_keys: Vec::new(),
        gender_mismatch: Vec::new(),
        female_same_as_default: Vec::new(),
        female_is_own_entry: Vec::new(),
    };

    // collect the full type vocabulary first so ids do not depend on iteration order
    for pokemon in data.pokemon.values() {
        for t in &pokemon.types {
            type_ids.insert(t.type_ref.name.clone(), 0);
        }
    }
    for form in data.forms.values() {
        for t in &form.types {
            type_ids.insert(t.type_ref.name.clone(), 0);
        }
    }
    for (i, (_, id)) in type_ids.iter_mut().enumerate() {
        if i == u8::MAX as usize {
            return Err("more than 255 types (255 is the empty type sentinel)".to_string());
        }
        *id = u8::try_from(i).map_err(|_| "more than 255 types".to_string())?;
    }
    m.types = type_ids.keys().cloned().collect();

    // Region names in PokeAPI id order, so a stored index means the same thing across
    // regenerations. Only regions some form actually belongs to are kept, so `--region` never
    // offers a value that can only ever match nothing. Orre has no Pokémon of its own, for one.
    let used: HashSet<&str> = data
        .forms
        .values()
        .filter_map(|f| data.form_region(f))
        .collect();
    m.regions = data
        .regions
        .values()
        .filter(|r| used.contains(r.name.as_str()))
        .map(|r| (r.id, r.name.clone()))
        .collect::<BTreeMap<u32, String>>()
        .into_values()
        .collect();

    let mut generations: HashSet<u8> = HashSet::new();

    for species in data.species.values() {
        let species_idx = u32::try_from(m.species.len()).unwrap();
        let entry_start = u32::try_from(m.entries.len()).unwrap();
        let generation = parse_generation(&species.generation.name)
            .ok_or_else(|| format!("{}: cannot parse {}", species.name, species.generation.name))?;
        generations.insert(generation);
        if !(-1..=8).contains(&species.gender_rate) {
            return Err(format!(
                "{}: invalid gender rate {}",
                species.name, species.gender_rate
            ));
        }
        if species.varieties.iter().filter(|v| v.is_default).count() != 1 {
            return Err(format!(
                "{}: expected exactly one default variety",
                species.name
            ));
        }

        // the default variety first, so the species' default entry lands at entry_start
        let mut varieties: Vec<&crate::api::Variety> = species.varieties.iter().collect();
        varieties.sort_by_key(|v| !v.is_default);

        // normalized selector to entry id, filled as entries are pushed
        let mut selectors: HashMap<String, u32> = HashMap::new();
        let mut aliases: Vec<(String, u32)> = Vec::new();

        for variety_ref in varieties {
            let pokemon_id = url_id(&variety_ref.pokemon.url)
                .ok_or_else(|| format!("{}: bad pokemon url", species.name))?;
            let pokemon = data
                .pokemon
                .get(&pokemon_id)
                .ok_or_else(|| format!("{}: missing pokemon {pokemon_id}", species.name))?;
            // the species' variety list and the pokemon record must agree about which is default
            if pokemon.is_default != variety_ref.is_default || pokemon.id != pokemon_id {
                return Err(format!(
                    "{}: pokemon {pokemon_id} disagrees with its species entry \
                     (id {}, is_default {} vs {})",
                    species.name, pokemon.id, pokemon.is_default, variety_ref.is_default
                ));
            }

            let mut forms: Vec<&FormJson> = Vec::new();
            for form_ref in &pokemon.forms {
                let form_id = url_id(&form_ref.url)
                    .ok_or_else(|| format!("{}: bad form url", pokemon.name))?;
                let form = data
                    .forms
                    .get(&form_id)
                    .ok_or_else(|| format!("{}: missing form {form_id}", pokemon.name))?;
                if form.id != form_id {
                    return Err(format!(
                        "{}: form {form_id} disagrees with its record id {}",
                        pokemon.name, form.id
                    ));
                }
                forms.push(form);
            }
            let default_forms = forms.iter().filter(|f| f.is_default).count();
            if default_forms > 1 {
                return Err(format!(
                    "{}: expected exactly one default form",
                    pokemon.name
                ));
            }
            // the variety's default form first, then the rest in the game's own form order
            forms.sort_by_key(|f| (!f.is_default, f.form_order, f.id));

            let variety_start = m.entries.len();
            for form in forms {
                let Some(entry) = build_entry(
                    &mut m,
                    data,
                    species,
                    species_idx,
                    generation,
                    pokemon,
                    variety_ref.is_default,
                    form,
                    &type_ids,
                )?
                else {
                    continue;
                };

                let entry_id = u32::try_from(m.entries.len()).unwrap();
                let is_default_entry = variety_ref.is_default && form.is_default;

                if is_default_entry {
                    // the default entry has no selector, but its variety and form slugs are still
                    // worth accepting (deoxys-normal, arceus-normal) so --form normal works
                    for slug in [pokemon.name.as_str(), form.name.as_str()] {
                        if let Some(sel) = strip_species_prefix(&species.name, slug) {
                            aliases.push((normalize(&sel), entry_id));
                        }
                    }
                } else {
                    let key = normalize(&entry.selector);
                    if key.is_empty() {
                        return Err(format!(
                            "{}: entry {} has an empty selector",
                            species.name, entry.full_slug
                        ));
                    }
                    if let Some(prev) = selectors.insert(key.clone(), entry_id) {
                        return Err(format!(
                            "{}: selector '{}' is ambiguous, claimed by both {} and {}",
                            species.name, key, m.entries[prev as usize].full_slug, entry.full_slug
                        ));
                    }
                }

                generations.insert(entry.generation);
                m.entries.push(entry);
            }
            // Some sprite-less ride varieties have no default form in PokéAPI. They are
            // excluded above; a variety that can be displayed must have a default.
            if m.entries.len() > variety_start && default_forms != 1 {
                return Err(format!(
                    "{}: expected exactly one default form",
                    pokemon.name
                ));
            }
        }

        let entry_end = u32::try_from(m.entries.len()).unwrap();
        if entry_start == entry_end {
            // every entry was dropped for want of a sprite, so the species is not displayable
            m.excluded
                .push((species.name.clone(), "no displayable entry".to_string()));
            continue;
        }
        if !m.entries[entry_start as usize].is_default_entry() {
            return Err(format!(
                "{}: default entry has no sprite; refusing to substitute an alternate",
                species.name
            ));
        }

        // aliases only fill gaps, a real selector always wins
        for (key, id) in aliases {
            if !key.is_empty() {
                selectors.entry(key).or_insert(id);
            }
        }
        for key in selectors.keys() {
            for (from, _) in SELECTOR_ALIASES {
                if key.starts_with(from) {
                    return Err(format!(
                        "{}: selector '{key}' collides with the '{from}' alias prefix",
                        species.name
                    ));
                }
            }
        }

        // PokeAPI's has_gender_differences flag should line up with the female sprites on disk
        let any_female = m.entries[entry_start as usize..entry_end as usize]
            .iter()
            .any(|e| e.flags & F_HAS_FEMALE != 0);
        if any_female != species.has_gender_differences {
            m.gender_mismatch.push(format!(
                "{}: has_gender_differences is {} but female sprites present is {any_female}",
                species.name, species.has_gender_differences
            ));
        }

        let flavor = latest_english_flavor(&species.flavor_text_entries);
        if flavor.is_none() {
            m.no_flavor.push(species.name.clone());
        }
        let mut selectors: Vec<(String, u32)> = selectors.into_iter().collect();
        selectors.sort();

        m.species.push(Species {
            slug: species.name.clone(),
            name: english_name(&species.names)
                .unwrap_or(&species.name)
                .to_string(),
            genus: english_genus(&species.genera).unwrap_or("").to_string(),
            flavor,
            dex: u16::try_from(species.id)
                .map_err(|_| format!("{}: dex overflow", species.name))?,
            generation,
            gender_rate: species.gender_rate,
            default_entry: entry_start,
            entry_start,
            entry_end,
            selectors,
        });
    }

    m.generations = {
        let mut g: Vec<u8> = generations.into_iter().collect();
        g.sort_unstable();
        g
    };

    check_reserved(&m)?;
    collect_odd_keys(&mut m);
    Ok(m)
}

/// Builds one entry, or returns `None` when it has to be excluded for want of a sprite.
#[allow(clippy::too_many_arguments)]
fn build_entry(
    m: &mut Model,
    data: &Dataset,
    species: &SpeciesJson,
    species_idx: u32,
    species_generation: u8,
    pokemon: &PokemonJson,
    variety_is_default: bool,
    form: &FormJson,
    type_ids: &BTreeMap<String, u8>,
) -> Result<Option<Entry>, String> {
    // the slug that names this entry: a non-default form names itself, otherwise the variety does
    let full_slug = if form.is_default {
        pokemon.name.clone()
    } else {
        form.name.clone()
    };
    let is_default_entry = variety_is_default && form.is_default;

    let selector = if is_default_entry {
        String::new()
    } else {
        match strip_species_prefix(&species.name, &full_slug) {
            Some(s) => s,
            None => {
                // the slug does not start with the species slug, so the whole slug is the
                // selector, worth a human look
                m.unprefixed
                    .push(format!("{} (species {})", full_slug, species.name));
                full_slug.clone()
            }
        }
    };

    let Some(sprites) = resolve_sprites(m, pokemon, form, &full_slug) else {
        return Ok(None);
    };

    let mut flags = 0u8;
    if sprites.has_female {
        flags |= F_HAS_FEMALE;
    }
    if sprites.no_shiny {
        flags |= F_NO_SHINY;
    }
    if sprites.no_shiny_female {
        flags |= F_NO_SHINY_FEMALE;
    }
    if form.is_battle_only {
        flags |= F_BATTLE_ONLY;
    }
    if variety_is_default {
        flags |= F_DEFAULT_VARIETY;
    }
    if form.is_default {
        flags |= F_DEFAULT_FORM;
    }

    // an entry's own generation and region come from the games that introduced its form, which
    // is often not where its species came from
    let version_group = data.version_group(form).ok_or_else(|| {
        format!(
            "{full_slug}: missing version group {}",
            form.version_group.url
        )
    })?;
    let generation = parse_generation(&version_group.generation.name).ok_or_else(|| {
        format!(
            "{full_slug}: cannot parse {}",
            version_group.generation.name
        )
    })?;
    if generation != species_generation {
        m.generation_differs
            .push((full_slug.clone(), species_generation, generation));
    }

    let region = match data.form_region(form) {
        Some(name) => {
            let index = m
                .regions
                .iter()
                .position(|r| r == name)
                .expect("every region name is registered before entries are built");
            m.regional.push((
                full_slug.clone(),
                name.to_string(),
                form.version_group.name.clone(),
            ));
            u8::try_from(index).unwrap() + 1
        }
        None => {
            m.region_unknown
                .push(format!("{} ({})", full_slug, form.version_group.name));
            0
        }
    };

    let height = u16::try_from(pokemon.height)
        .map_err(|_| format!("{}: height {} overflows u16", pokemon.name, pokemon.height))?;
    let weight = u16::try_from(pokemon.weight)
        .map_err(|_| format!("{}: weight {} overflows u16", pokemon.name, pokemon.weight))?;
    Ok(Some(Entry {
        species_idx,
        display_name: display_name(species, form, &selector, is_default_entry),
        types: resolve_types(pokemon, form, type_ids)?,
        stats: resolve_stats(pokemon)?,
        height,
        weight,
        flags,
        generation,
        region,
        sprite_paths: sprites.paths,
        selector,
        full_slug,
    }))
}

/// Sprite slots after every fallback has been applied at generation time.
struct ResolvedSprites {
    /// `[default, female, shiny, shiny_female]`, all four always present.
    paths: [String; 4],
    has_female: bool,
    no_shiny: bool,
    no_shiny_female: bool,
}

/// Fills all four sprite slots so the runtime never has to branch on a missing file.
fn resolve_sprites(
    m: &mut Model,
    pokemon: &PokemonJson,
    form: &FormJson,
    full_slug: &str,
) -> Option<ResolvedSprites> {
    // A variety's default form carries the variety's sprites when it has none of its own. The
    // fallback is all or nothing: mixing levels would pair Red Florges' own sprite with the
    // variety's plain Florges as its "female", which is a different picture of the same Pokémon.
    let form_paths = form.sprites.to_repo_paths();
    let paths = if form_paths[0].is_none() && form.is_default {
        pokemon.sprites.to_repo_paths()
    } else {
        form_paths
    };
    let pick = |i: usize| -> Option<String> { paths[i].clone() };

    let Some(default) = pick(0) else {
        m.excluded
            .push((full_slug.to_string(), "no front_default sprite".to_string()));
        return None;
    };

    let shiny = pick(1);
    let no_shiny = shiny.is_none();
    if no_shiny {
        m.no_shiny.push(full_slug.to_string());
    }
    let shiny = shiny.unwrap_or_else(|| default.clone());

    let female = pick(2);
    let has_female = female.is_some();
    let no_shiny_female = if has_female {
        pick(3).is_none()
    } else {
        no_shiny
    };
    let (female, shiny_female) = match female {
        // female exists, so its shiny counterpart falls back to the plain female sprite
        Some(f) => {
            let sf = pick(3).unwrap_or_else(|| {
                m.no_shiny_female.push(full_slug.to_string());
                f.clone()
            });
            (f, sf)
        }
        // no female sprite at all, point both female slots at the male ones
        None => (default.clone(), shiny.clone()),
    };

    Some(ResolvedSprites {
        paths: [default, female, shiny, shiny_female],
        has_female,
        no_shiny,
        no_shiny_female,
    })
}

/// Picks the entry's types: the form's own when it has any, otherwise the variety's.
fn resolve_types(
    pokemon: &PokemonJson,
    form: &FormJson,
    type_ids: &BTreeMap<String, u8>,
) -> Result<[u8; 2], String> {
    let mut slots: Vec<(u8, u8)> = if form.types.is_empty() {
        pokemon
            .types
            .iter()
            .map(|t| (t.slot, type_ids[&t.type_ref.name]))
            .collect()
    } else {
        form.types
            .iter()
            .map(|t| (t.slot, type_ids[&t.type_ref.name]))
            .collect()
    };
    slots.sort_unstable();
    if slots.is_empty()
        || slots.len() > 2
        || slots
            .iter()
            .enumerate()
            .any(|(i, (slot, _))| *slot as usize != i + 1)
    {
        return Err(format!(
            "{}: expected one or two consecutive type slots",
            form.name
        ));
    }
    let mut out = [u8::MAX; 2];
    for (i, (_, id)) in slots.into_iter().enumerate() {
        out[i] = id;
    }
    Ok(out)
}

/// Reads the six base stats off the variety in termidex's display order.
fn resolve_stats(pokemon: &PokemonJson) -> Result<[u8; 6], String> {
    let mut out = [0u8; 6];
    let mut seen = [false; 6];
    for slot in &pokemon.stats {
        let i = STAT_ORDER
            .iter()
            .position(|s| *s == slot.stat.name)
            .ok_or_else(|| format!("{}: unknown stat {}", pokemon.name, slot.stat.name))?;
        if seen[i] {
            return Err(format!(
                "{}: duplicate stat {}",
                pokemon.name, slot.stat.name
            ));
        }
        seen[i] = true;
        out[i] = u8::try_from(slot.base_stat).map_err(|_| {
            format!(
                "{}: stat {} value {} overflows u8",
                pokemon.name, slot.stat.name, slot.base_stat
            )
        })?;
    }
    if seen.contains(&false) {
        return Err(format!("{}: expected all six base stats", pokemon.name));
    }
    Ok(out)
}

/// Works out what to print for `--name`.
fn display_name(
    species: &SpeciesJson,
    form: &FormJson,
    selector: &str,
    is_default_entry: bool,
) -> String {
    if let Some(name) = english_name(&form.names) {
        return name.to_string();
    }
    let species_name = english_name(&species.names).unwrap_or(&species.name);
    if is_default_entry {
        return species_name.to_string();
    }
    format!("{species_name} ({})", title_case(selector))
}

/// Removes the species slug from the front of a variety or form slug.
///
/// # Returns
/// the remainder, or `None` when the slug is the species slug itself or does not start with it
fn strip_species_prefix(species_slug: &str, slug: &str) -> Option<String> {
    let rest = slug.strip_prefix(species_slug)?.strip_prefix('-')?;
    (!rest.is_empty()).then(|| rest.to_string())
}

/// Turns a selector into something printable, so `mega-x` reads as `Mega X`.
pub fn title_case(s: &str) -> String {
    s.split('-')
        .map(|word| {
            let mut c = word.chars();
            match c.next() {
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Turns `generation-vii` into `7`.
fn parse_generation(name: &str) -> Option<u8> {
    let roman = name.strip_prefix("generation-")?;
    let mut total = 0u32;
    let mut prev = 0u32;
    for ch in roman.chars().rev() {
        let value = match ch {
            'i' => 1,
            'v' => 5,
            'x' => 10,
            'l' => 50,
            'c' => 100,
            _ => return None,
        };
        if value < prev {
            total -= value;
        } else {
            total += value;
            prev = value;
        }
    }
    u8::try_from(total).ok()
}

/// Makes sure no species could ever be shadowed by a subcommand.
fn check_reserved(m: &Model) -> Result<(), String> {
    for species in &m.species {
        for key in [normalize(&species.slug), normalize(&species.name)] {
            if RESERVED_NAMES.contains(&key.as_str()) {
                return Err(format!(
                    "species '{}' normalizes to the reserved subcommand name '{key}'",
                    species.slug
                ));
            }
        }
    }
    Ok(())
}

/// Records any lookup key that kept a character the normalizer has no explicit rule for.
fn collect_odd_keys(m: &mut Model) {
    let mut seen = HashSet::new();
    let mut report = |label: String, key: String| {
        if normalize::has_unmapped_char(&key) && seen.insert(key.clone()) {
            m.odd_keys.push(format!("{label} -> {key}"));
        }
    };
    let keys: Vec<(String, String)> = m
        .species
        .iter()
        .flat_map(|s| {
            [
                (s.slug.clone(), normalize(&s.slug)),
                (s.name.clone(), normalize(&s.name)),
            ]
        })
        .collect();
    for (label, key) in keys {
        report(label, key);
    }
}

/// Re-exported so the rest of the generator does not need to know where it lives.
pub use normalize::normalize;

/// Clears `F_HAS_FEMALE` on entries whose female sprite is indistinguishable from the default.
///
/// PokeAPI often fills `front_female` with the *same URL* as `front_default`, and occasionally with
/// a different file holding identical pixels. Nidoqueen is the obvious case: it is female-only, and
/// its female sprite is simply the default one, so marking output with ♀ claims a difference that is
/// not there. Comparing the encoded sprite indices catches both, because the blob deduplicates by
/// content hash, so identical pixels always share an index.
///
/// Runs after encoding, since that is when the indices exist.
///
/// # Arguments
/// * `m` - the model to correct in place
/// * `slots` - `entry_id * 4 + shiny * 2 + female` to sprite directory index
pub fn drop_indistinguishable_females(m: &mut Model, slots: &[u32]) {
    for (id, entry) in m.entries.iter_mut().enumerate() {
        if entry.flags & F_HAS_FEMALE == 0 {
            continue;
        }
        if slots[id * 4] == slots[id * 4 + 1] {
            entry.flags &= !F_HAS_FEMALE;
            m.female_same_as_default.push(entry.full_slug.clone());
        }
    }
}

/// Drops the female sprite from entries whose female PokeAPI keeps as a separate entry.
///
/// Frillish, Jellicent and Pyroar split gender into forms, and Meowstic, Indeedee, Basculegion and
/// Oinkologne into varieties. Their default entry is "Male Pyroar", yet its variety still carries a
/// `front_female`, which printed "Male Pyroar ♀" over the male's stats. The female is reachable as
/// `--form female`, so keeping a second copy on the male entry would only give two ways to ask for
/// the same thing. A male alternate like `male-mega` pairs with `female-mega` the same way.
///
/// Runs before encoding, so the dropped sprites never reach the blob.
///
/// # Arguments
/// * `m` - the model to correct in place
pub fn drop_females_with_own_entry(m: &mut Model) {
    for species in &m.species {
        for id in species.entry_start..species.entry_end {
            let entry = &mut m.entries[id as usize];
            if entry.flags & F_HAS_FEMALE == 0 {
                continue;
            }
            // the default entry is the male, and male-x pairs with female-x
            let female_key = if entry.is_default_entry() {
                "female".to_string()
            } else {
                match normalize(&entry.selector).strip_prefix("male") {
                    Some(rest) => format!("female{rest}"),
                    None => continue,
                }
            };
            let has_female_entry = species
                .selectors
                .iter()
                .any(|(key, other)| *key == female_key && *other != id);
            if !has_female_entry {
                continue;
            }
            entry.flags &= !F_HAS_FEMALE;
            entry.sprite_paths[1] = entry.sprite_paths[0].clone();
            entry.sprite_paths[3] = entry.sprite_paths[2].clone();
            entry.flags &= !F_NO_SHINY_FEMALE;
            if entry.flags & F_NO_SHINY != 0 {
                entry.flags |= F_NO_SHINY_FEMALE;
            }
            m.female_is_own_entry.push(entry.full_slug.clone());
        }
    }
    // a dropped female sprite can no longer be missing its shiny counterpart
    let dropped = &m.female_is_own_entry;
    m.no_shiny_female.retain(|slug| !dropped.contains(slug));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{
        FormJson, FrontSprites, Genus, LocalizedName, NamedRef, PokedexJson, PokemonJson,
        RegionJson, SpeciesJson, StatSlot, TypeSlot, Variety, VersionGroupJson,
    };
    use std::path::PathBuf;

    const SPRITE_BASE: &str = "https://raw.githubusercontent.com/PokeAPI/sprites/master/";

    fn named(name: &str, url: &str) -> NamedRef {
        NamedRef {
            name: name.to_string(),
            url: url.to_string(),
        }
    }

    fn english(name: &str) -> LocalizedName {
        LocalizedName {
            name: name.to_string(),
            language: named("en", "/api/v2/language/9/"),
        }
    }

    /// The one version group the synthetic forms use: gen 1, Kanto.
    fn version_group() -> NamedRef {
        named("red-green-japan", "/api/v2/version-group/1/")
    }

    /// A version group, its pokedex and its region, so region lookup resolves in tests.
    fn region_tables() -> (
        BTreeMap<u32, VersionGroupJson>,
        BTreeMap<u32, PokedexJson>,
        BTreeMap<u32, RegionJson>,
    ) {
        let vg = VersionGroupJson {
            generation: named("generation-i", "/api/v2/generation/1/"),
            pokedexes: vec![named("kanto", "/api/v2/pokedex/2/")],
        };
        let dex = PokedexJson {
            region: Some(named("kanto", "/api/v2/region/1/")),
        };
        let region = RegionJson {
            id: 1,
            name: "kanto".to_string(),
        };
        (
            [(1, vg)].into_iter().collect(),
            [(2, dex)].into_iter().collect(),
            [(1, region)].into_iter().collect(),
        )
    }

    fn sprites(id: u32) -> FrontSprites {
        FrontSprites {
            front_default: Some(format!("{SPRITE_BASE}sprites/pokemon/{id}.png")),
            front_shiny: Some(format!("{SPRITE_BASE}sprites/pokemon/shiny/{id}.png")),
            front_female: None,
            front_shiny_female: None,
        }
    }

    fn stats() -> Vec<StatSlot> {
        STAT_ORDER
            .iter()
            .enumerate()
            .map(|(i, name)| StatSlot {
                base_stat: 50,
                stat: named(name, &format!("/api/v2/stat/{}/", i + 1)),
            })
            .collect()
    }

    fn types() -> Vec<TypeSlot> {
        vec![TypeSlot {
            slot: 1,
            type_ref: named("normal", "/api/v2/type/1/"),
        }]
    }

    /// One species with one default variety and one default form, ready to be bent out of shape.
    fn one_species(slug: &str, display: &str) -> Dataset {
        let species = SpeciesJson {
            id: 1,
            name: slug.to_string(),
            generation: named("generation-i", "/api/v2/generation/1/"),
            gender_rate: 4,
            has_gender_differences: false,
            varieties: vec![Variety {
                is_default: true,
                pokemon: named(slug, "/api/v2/pokemon/1/"),
            }],
            names: vec![english(display)],
            genera: vec![Genus {
                genus: "Test Pokémon".to_string(),
                language: named("en", "/api/v2/language/9/"),
            }],
            flavor_text_entries: Vec::new(),
        };
        let pokemon = PokemonJson {
            id: 1,
            name: slug.to_string(),
            is_default: true,
            height: 10,
            weight: 100,
            forms: vec![named(slug, "/api/v2/pokemon-form/1/")],
            stats: stats(),
            types: types(),
            sprites: sprites(1),
        };
        let form = FormJson {
            id: 1,
            name: slug.to_string(),
            version_group: version_group(),
            form_order: 1,
            is_default: true,
            is_battle_only: false,
            types: Vec::new(),
            names: Vec::new(),
            sprites: FrontSprites::default(),
        };

        let (version_groups, pokedexes, regions) = region_tables();
        Dataset {
            species: [(1, species)].into_iter().collect(),
            pokemon: [(1, pokemon)].into_iter().collect(),
            forms: [(1, form)].into_iter().collect(),
            version_groups,
            pokedexes,
            regions,
            sprites_root: PathBuf::from("vendor/sprites"),
        }
    }

    /// Adds a second variety whose default form carries the given slug.
    fn add_variety(data: &mut Dataset, id: u32, slug: &str) {
        let species = data.species.get_mut(&1).unwrap();
        species.varieties.push(Variety {
            is_default: false,
            pokemon: named(slug, &format!("/api/v2/pokemon/{id}/")),
        });
        data.pokemon.insert(
            id,
            PokemonJson {
                id,
                name: slug.to_string(),
                is_default: false,
                height: 10,
                weight: 100,
                forms: vec![named(slug, &format!("/api/v2/pokemon-form/{id}/"))],
                stats: stats(),
                types: types(),
                sprites: sprites(id),
            },
        );
        data.forms.insert(
            id,
            FormJson {
                id,
                name: slug.to_string(),
                version_group: version_group(),
                form_order: 1,
                is_default: true,
                is_battle_only: false,
                types: Vec::new(),
                names: Vec::new(),
                sprites: FrontSprites::default(),
            },
        );
    }

    /// Adds an extra cosmetic form to the default variety.
    fn add_form(data: &mut Dataset, id: u32, slug: &str, order: u32) {
        data.pokemon
            .get_mut(&1)
            .unwrap()
            .forms
            .push(named(slug, &format!("/api/v2/pokemon-form/{id}/")));
        data.forms.insert(
            id,
            FormJson {
                id,
                name: slug.to_string(),
                version_group: version_group(),
                form_order: order,
                is_default: false,
                is_battle_only: false,
                types: Vec::new(),
                names: Vec::new(),
                sprites: sprites(id),
            },
        );
    }

    /// the happy path should build one species with one default entry and no selectors
    #[test]
    fn minimal_dataset_builds() {
        let model = build(&one_species("testmon", "Testmon")).expect("should build");
        assert_eq!(model.species.len(), 1);
        assert_eq!(model.entries.len(), 1);
        assert!(model.entries[0].is_default_entry());
        assert!(model.species[0].selectors.is_empty());
    }

    /// a variety and a form claiming the same selector should fail loudly and name both
    #[test]
    fn ambiguous_selector_fails() {
        let mut data = one_species("testmon", "Testmon");
        add_variety(&mut data, 2, "testmon-alt");
        add_form(&mut data, 3, "testmon-alt", 2);

        let error = build(&data).expect_err("an ambiguous selector should fail");
        assert!(error.contains("ambiguous"), "got: {error}");
        assert!(error.contains("testmon-alt"), "got: {error}");
    }

    /// a species named after a subcommand should be rejected, by slug or by English name
    #[test]
    fn reserved_names_are_rejected() {
        for (slug, name) in [("random", "Random"), ("list", "List"), ("help", "Help")] {
            let error =
                build(&one_species(slug, name)).expect_err("a reserved slug should be rejected");
            assert!(error.contains("reserved"), "got: {error}");
        }
        // the English name is a lookup key too, so it gets the same treatment
        let error = build(&one_species("testmon", "List")).expect_err("name should be rejected");
        assert!(error.contains("reserved"), "got: {error}");
    }

    /// a selector that normalizes down to nothing would be untypeable, so it must fail
    #[test]
    fn empty_selector_fails() {
        let mut data = one_species("testmon", "Testmon");
        // the slug leaves only punctuation once the species prefix comes off
        add_variety(&mut data, 2, "testmon--");
        let error = build(&data).expect_err("an empty selector should fail");
        assert!(error.contains("empty selector"), "got: {error}");
    }

    /// a selector that starts with an alias prefix would be shadowed by the rewrite
    #[test]
    fn selector_colliding_with_an_alias_fails() {
        let mut data = one_species("testmon", "Testmon");
        add_variety(&mut data, 2, "testmon-alolanguard");
        let error = build(&data).expect_err("an alias prefix collision should fail");
        assert!(error.contains("alias prefix"), "got: {error}");
    }

    /// the species list and the pokemon records must agree about which variety is default
    #[test]
    fn disagreeing_is_default_fails() {
        let mut data = one_species("testmon", "Testmon");
        data.pokemon.get_mut(&1).unwrap().is_default = false;
        let error = build(&data).expect_err("a mismatch should fail");
        assert!(error.contains("disagrees"), "got: {error}");
    }

    /// an entry with no front_default anywhere should be excluded and reported
    #[test]
    fn missing_sprite_excludes_the_entry() {
        let mut data = one_species("testmon", "Testmon");
        add_variety(&mut data, 2, "testmon-alt");
        data.pokemon.get_mut(&2).unwrap().sprites.front_default = None;

        let model = build(&data).expect("the species should still build");
        assert_eq!(
            model.entries.len(),
            1,
            "the sprite-less entry should be gone"
        );
        assert!(model.excluded.iter().any(|(slug, _)| slug == "testmon-alt"));
    }

    #[test]
    fn missing_default_sprite_never_promotes_an_alternate() {
        let mut data = one_species("testmon", "Testmon");
        add_variety(&mut data, 2, "testmon-alt");
        data.pokemon.get_mut(&1).unwrap().sprites.front_default = None;
        let error = build(&data).expect_err("cannot substitute an alternate for the default");
        assert!(error.contains("default entry has no sprite"), "{error}");
    }

    #[test]
    fn default_variety_and_form_must_be_unique() {
        let mut data = one_species("testmon", "Testmon");
        add_variety(&mut data, 2, "testmon-alt");
        data.species.get_mut(&1).unwrap().varieties[1].is_default = true;
        assert!(build(&data)
            .unwrap_err()
            .contains("exactly one default variety"));

        let mut data = one_species("testmon", "Testmon");
        add_form(&mut data, 2, "testmon-alt", 2);
        data.forms.get_mut(&2).unwrap().is_default = true;
        assert!(build(&data)
            .unwrap_err()
            .contains("exactly one default form"));
    }

    #[test]
    fn invalid_stats_are_rejected_instead_of_truncated_or_filled() {
        for mutation in 0..4 {
            let mut data = one_species("testmon", "Testmon");
            let stats = &mut data.pokemon.get_mut(&1).unwrap().stats;
            match mutation {
                0 => stats[0].base_stat = 256,
                1 => {
                    stats.pop();
                }
                2 => stats[1].stat.name = "hp".to_string(),
                _ => stats[0].stat.name = "unknown".to_string(),
            }
            assert!(build(&data).is_err(), "invalid stats mutation {mutation}");
        }
    }

    #[test]
    fn measurements_must_fit_without_clamping() {
        for height in [true, false] {
            let mut data = one_species("testmon", "Testmon");
            let pokemon = data.pokemon.get_mut(&1).unwrap();
            if height {
                pokemon.height = 65536;
            } else {
                pokemon.weight = 65536;
            }
            assert!(build(&data).unwrap_err().contains("overflows u16"));
        }
    }

    #[test]
    fn invalid_types_and_missing_version_groups_are_rejected() {
        let mut data = one_species("testmon", "Testmon");
        data.pokemon.get_mut(&1).unwrap().types[0].slot = 2;
        assert!(build(&data).unwrap_err().contains("type slots"));
        let mut data = one_species("testmon", "Testmon");
        data.version_groups.clear();
        assert!(build(&data).unwrap_err().contains("missing version group"));
    }

    #[test]
    fn entry_generations_are_available_to_filters() {
        let mut data = one_species("testmon", "Testmon");
        data.version_groups.get_mut(&1).unwrap().generation =
            named("generation-ii", "/api/v2/generation/2/");
        assert_eq!(build(&data).unwrap().generations, vec![1, 2]);
    }

    /// roman generation numbers should parse across the range the data uses
    #[test]
    fn generations_parse() {
        assert_eq!(parse_generation("generation-i"), Some(1));
        assert_eq!(parse_generation("generation-iv"), Some(4));
        assert_eq!(parse_generation("generation-vii"), Some(7));
        assert_eq!(parse_generation("generation-ix"), Some(9));
        assert_eq!(parse_generation("generation-x"), Some(10));
        assert_eq!(parse_generation("nonsense"), None);
    }

    /// the species prefix should only come off when it is really a prefix
    #[test]
    fn prefix_stripping() {
        assert_eq!(
            strip_species_prefix("charizard", "charizard-mega-x").as_deref(),
            Some("mega-x")
        );
        assert_eq!(
            strip_species_prefix("deoxys", "deoxys-normal").as_deref(),
            Some("normal")
        );
        // the slug is the species itself, so there is nothing left
        assert_eq!(strip_species_prefix("pikachu", "pikachu"), None);
        // a shared start is not a prefix without the separator
        assert_eq!(strip_species_prefix("pika", "pikachu"), None);
    }

    /// selectors should read as words when they get printed
    #[test]
    fn selectors_title_case() {
        assert_eq!(title_case("mega-x"), "Mega X");
        assert_eq!(title_case("paldea-combat-breed"), "Paldea Combat Breed");
        assert_eq!(title_case("gmax"), "Gmax");
    }
}
