//! Serde types for the three PokeAPI endpoints the generator reads, plus the loader that walks
//! the vendored `api-data` checkout.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The prefixes a sprite URL can carry before the path inside the sprites repo.
const SPRITE_URL_PREFIXES: [&str; 2] = [
    "https://raw.githubusercontent.com/PokeAPI/sprites/master/",
    "https://raw.githubusercontent.com/PokeAPI/sprites/main/",
];

#[derive(Deserialize, Debug, Clone)]
pub struct NamedRef {
    pub name: String,
    pub url: String,
}

#[derive(Deserialize, Debug)]
pub struct LocalizedName {
    pub name: String,
    pub language: NamedRef,
}

#[derive(Deserialize, Debug)]
pub struct Genus {
    pub genus: String,
    pub language: NamedRef,
}

#[derive(Deserialize, Debug)]
pub struct FlavorText {
    pub flavor_text: String,
    pub language: NamedRef,
    pub version: NamedRef,
}

#[derive(Deserialize, Debug)]
pub struct Variety {
    pub is_default: bool,
    pub pokemon: NamedRef,
}

#[derive(Deserialize, Debug)]
pub struct SpeciesJson {
    pub id: u32,
    pub name: String,
    pub generation: NamedRef,
    pub gender_rate: i8,
    pub has_gender_differences: bool,
    pub varieties: Vec<Variety>,
    pub names: Vec<LocalizedName>,
    pub genera: Vec<Genus>,
    pub flavor_text_entries: Vec<FlavorText>,
}

#[derive(Deserialize, Debug)]
pub struct TypeSlot {
    pub slot: u8,
    #[serde(rename = "type")]
    pub type_ref: NamedRef,
}

#[derive(Deserialize, Debug)]
pub struct StatSlot {
    pub base_stat: u32,
    pub stat: NamedRef,
}

/// The four front-facing sprite slots termidex uses. Everything else in the `sprites` object
/// (back sprites, `versions`, `other`) is deliberately ignored.
#[derive(Deserialize, Debug, Default, Clone)]
pub struct FrontSprites {
    pub front_default: Option<String>,
    pub front_shiny: Option<String>,
    pub front_female: Option<String>,
    pub front_shiny_female: Option<String>,
}

impl FrontSprites {
    /// Rewrites each slot from a raw.githubusercontent URL to a path inside the sprites clone.
    ///
    /// # Returns
    /// the same four slots, holding repo-relative paths like `sprites/pokemon/25.png`
    pub fn to_repo_paths(&self) -> [Option<String>; 4] {
        [
            strip_sprite_prefix(self.front_default.as_deref()),
            strip_sprite_prefix(self.front_shiny.as_deref()),
            strip_sprite_prefix(self.front_female.as_deref()),
            strip_sprite_prefix(self.front_shiny_female.as_deref()),
        ]
    }
}

/// Turns one sprite URL into a repo-relative path, or `None` if the slot is null.
fn strip_sprite_prefix(url: Option<&str>) -> Option<String> {
    let url = url?;
    for prefix in SPRITE_URL_PREFIXES {
        if let Some(rest) = url.strip_prefix(prefix) {
            return Some(rest.to_string());
        }
    }
    // an unexpected host means the generator would guess at a filename, which is exactly what
    // the data contract forbids
    panic!("sprite url does not point at the PokeAPI sprites repo: {url}");
}

#[derive(Deserialize, Debug)]
pub struct PokemonJson {
    pub id: u32,
    pub name: String,
    pub is_default: bool,
    pub height: u32,
    pub weight: u32,
    pub forms: Vec<NamedRef>,
    pub stats: Vec<StatSlot>,
    pub types: Vec<TypeSlot>,
    pub sprites: FrontSprites,
}

#[derive(Deserialize, Debug)]
pub struct FormJson {
    pub id: u32,
    pub name: String,
    /// The games this form was introduced in, which is how its generation and region are derived.
    pub version_group: NamedRef,
    pub form_order: u32,
    pub is_default: bool,
    pub is_battle_only: bool,
    #[serde(default)]
    pub types: Vec<TypeSlot>,
    pub names: Vec<LocalizedName>,
    pub sprites: FrontSprites,
}

/// A set of games, which is what dates a form and places it in a region.
#[derive(Deserialize, Debug)]
pub struct VersionGroupJson {
    pub generation: NamedRef,
    /// The dexes these games ship. Preferred over `regions` for working out the region, because
    /// `gold-silver` lists both Kanto and Johto while its only dex is `original-johto`, and
    /// `mega-dimension` lists no region at all while its `hyperspace` dex is Kalos.
    pub pokedexes: Vec<NamedRef>,
}

/// A regional Pokédex, which is the link from a set of games to a region.
#[derive(Deserialize, Debug)]
pub struct PokedexJson {
    /// `None` for the national dex, which spans every region.
    pub region: Option<NamedRef>,
}

/// A region, used only for its id so the stored order matches PokeAPI's.
#[derive(Deserialize, Debug)]
pub struct RegionJson {
    pub id: u32,
    pub name: String,
}

/// Every record the generator needs, keyed by PokeAPI id.
pub struct Dataset {
    pub species: BTreeMap<u32, SpeciesJson>,
    pub pokemon: BTreeMap<u32, PokemonJson>,
    pub forms: BTreeMap<u32, FormJson>,
    pub version_groups: BTreeMap<u32, VersionGroupJson>,
    pub pokedexes: BTreeMap<u32, PokedexJson>,
    pub regions: BTreeMap<u32, RegionJson>,
    pub sprites_root: PathBuf,
}

impl Dataset {
    /// Works out which region a form belongs to, by way of the games that introduced it.
    ///
    /// The chain is form -> version group -> its first dex that has a region -> that region.
    ///
    /// # Arguments
    /// * `form` - the form to place
    ///
    /// # Returns
    /// the region name, or `None` when no dex of those games maps to a region
    pub fn form_region(&self, form: &FormJson) -> Option<&str> {
        let vg = self.version_group(form)?;
        vg.pokedexes.iter().find_map(|dex_ref| {
            let id = url_id(&dex_ref.url)?;
            self.pokedexes
                .get(&id)?
                .region
                .as_ref()
                .map(|r| r.name.as_str())
        })
    }

    /// Borrows the version group a form was introduced in.
    pub fn version_group(&self, form: &FormJson) -> Option<&VersionGroupJson> {
        self.version_groups.get(&url_id(&form.version_group.url)?)
    }
}

impl Dataset {
    /// Reads every `pokemon-species`, `pokemon` and `pokemon-form` record out of `vendor/`.
    ///
    /// # Arguments
    /// * `vendor` - the `vendor/` directory holding the `api-data` and `sprites` clones
    ///
    /// # Returns
    /// the loaded dataset, or an error describing which file could not be read
    pub fn load(vendor: &Path) -> Result<Dataset, String> {
        let api = vendor.join("api-data/data/api/v2");
        if !api.is_dir() {
            return Err(format!(
                "{} is missing, run scripts/fetch-data.sh first",
                api.display()
            ));
        }
        let sprites_root = vendor.join("sprites");
        if !sprites_root.join("sprites/pokemon").is_dir() {
            return Err(format!(
                "{} is missing, run scripts/fetch-data.sh first",
                sprites_root.display()
            ));
        }

        Ok(Dataset {
            species: load_endpoint(&api.join("pokemon-species"))?,
            pokemon: load_endpoint(&api.join("pokemon"))?,
            forms: load_endpoint(&api.join("pokemon-form"))?,
            version_groups: load_endpoint(&api.join("version-group"))?,
            pokedexes: load_endpoint(&api.join("pokedex"))?,
            regions: load_endpoint(&api.join("region"))?,
            sprites_root,
        })
    }
}

/// Reads every `<id>/index.json` under one endpoint directory.
///
/// Directory entries whose name is not a number are skipped, which drops the endpoint's own
/// `index.json` list and the `encounters` subtree.
fn load_endpoint<T: serde::de::DeserializeOwned>(dir: &Path) -> Result<BTreeMap<u32, T>, String> {
    let mut out = BTreeMap::new();
    let listing = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in listing {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        let path = entry.path().join("index.json");
        let text = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let value: T = serde_json::from_str(&text)
            .map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
        out.insert(id, value);
    }
    if out.is_empty() {
        return Err(format!("no records found under {}", dir.display()));
    }
    Ok(out)
}

/// Pulls the trailing numeric id out of a PokeAPI resource url.
///
/// Works for both the live absolute urls and the relative ones the static mirror uses, so
/// `/api/v2/pokemon/25/` gives `25`.
///
/// # Arguments
/// * `url` - a PokeAPI resource url
///
/// # Returns
/// the id, or `None` if the last path segment is not a number
pub fn url_id(url: &str) -> Option<u32> {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .and_then(|s| s.parse().ok())
}

/// Picks the English entry out of a localized name list.
pub fn english_name(names: &[LocalizedName]) -> Option<&str> {
    names
        .iter()
        .find(|n| n.language.name == "en")
        .map(|n| n.name.as_str())
}

/// Picks the English entry out of a genus list.
pub fn english_genus(genera: &[Genus]) -> Option<&str> {
    genera
        .iter()
        .find(|g| g.language.name == "en")
        .map(|g| g.genus.as_str())
}

/// Finds the newest English flavor text and cleans it up for one-line display.
///
/// "Newest" means the highest version id, parsed from the version url. The games break flavor
/// text across fixed-width lines with real newlines and form feeds, and hyphenate across them
/// with a soft hyphen, so all of that has to be undone.
///
/// # Arguments
/// * `entries` - the species' whole `flavor_text_entries` list
///
/// # Returns
/// the cleaned text, or `None` if the species has no English entry
pub fn latest_english_flavor(entries: &[FlavorText]) -> Option<String> {
    let best = entries
        .iter()
        .filter(|e| e.language.name == "en")
        .max_by_key(|e| url_id(&e.version.url).unwrap_or(0))?;

    // a soft hyphen right before a line break is a word split, joining the halves drops both
    let mut text = String::with_capacity(best.flavor_text.len());
    let mut chars = best.flavor_text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\n' | '\u{c}' | '\r' | '\t' => text.push(' '),
            // The source also splits words across form feeds and CRLF, not just newlines.
            '\u{ad}' => {
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
            }
            c => text.push(c),
        }
    }
    Some(text.split_whitespace().collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_hyphens_join_words_across_game_line_and_page_breaks() {
        for line_break in ["\n", "\r\n", "\u{c}"] {
            let entries = vec![FlavorText {
                flavor_text: format!("A poi\u{ad}{line_break}sonous Pokémon.\nIt sleeps.\u{ad}"),
                language: NamedRef {
                    name: "en".to_string(),
                    url: String::new(),
                },
                version: NamedRef {
                    name: "red".to_string(),
                    url: "/api/v2/version/1/".to_string(),
                },
            }];
            assert_eq!(
                latest_english_flavor(&entries).as_deref(),
                Some("A poisonous Pokémon. It sleeps.")
            );
        }
    }
}
