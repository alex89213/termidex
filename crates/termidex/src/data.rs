//! The baked-in Pokémon tables.
//!
//! Everything here is `static`, so a lookup is an array index and startup does no work at all.
//! The tables themselves come from `generated/data.rs`, which `termidex-gen` writes from the
//! PokeAPI mirrors. The struct definitions live here rather than in the generated file so the
//! generated file stays pure data.

/// A packed 24-bit color.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// A Pokémon type, with the community-palette color it prints in.
pub struct TypeInfo {
    /// Display name, already title cased.
    pub name: &'static str,
    /// `None` for types with no agreed color, such as `unknown` and `stellar`.
    pub color: Option<Rgb>,
}

/// One Pokémon species, owning a contiguous run of entries.
pub struct Species {
    pub slug: &'static str,
    pub name: &'static str,
    pub genus: &'static str,
    /// Species-level Pokédex text, so every form of a species shares it.
    pub flavor: Option<&'static str>,
    /// National Dex number.
    pub dex: u16,
    pub generation: u8,
    /// PokeAPI's chance-of-female in eighths, or -1 for genderless.
    pub gender_rate: i8,
    pub default_entry: u32,
    pub entry_start: u32,
    pub entry_end: u32,
    pub selector_start: u32,
    pub selector_end: u32,
}

/// One displayable (variety, form) pair.
pub struct Entry {
    pub species: u32,
    pub display_name: &'static str,
    /// Empty for a species' default entry.
    pub selector: &'static str,
    /// Type ids, second slot is `u8::MAX` for single-type Pokémon.
    pub types: [u8; 2],
    /// HP, Attack, Defense, Sp. Atk, Sp. Def, Speed.
    pub stats: [u8; 6],
    /// Height in decimetres, straight from PokeAPI.
    pub height: u16,
    /// Weight in hectograms, straight from PokeAPI.
    pub weight: u16,
    pub flags: u8,
    /// The generation that introduced this entry, which is not always its species'.
    ///
    /// Mega Beedrill is Gen 6 on a Gen 1 Pokémon and Mega Raichu is Gen 9 on one, so filtering
    /// entries by their species' generation would mislabel them.
    pub generation: u8,
    /// The region whose games introduced this entry, as a `REGIONS` index plus one.
    ///
    /// Not the species' region: Charizard is Kanto but Mega Charizard X is Kalos, while Kyogre and
    /// Primal Kyogre are both Hoenn because ORAS is a Hoenn game. 0 only if PokeAPI maps the
    /// entry's games to no region, which currently never happens.
    pub region: u8,
}

pub const F_HAS_FEMALE: u8 = 1 << 0;
pub const F_NO_SHINY: u8 = 1 << 1;
pub const F_BATTLE_ONLY: u8 = 1 << 2;
pub const F_DEFAULT_VARIETY: u8 = 1 << 3;
pub const F_DEFAULT_FORM: u8 = 1 << 4;
pub const F_NO_SHINY_FEMALE: u8 = 1 << 5;

/// Marks a single-type Pokémon's empty second type slot.
pub const NO_TYPE: u8 = u8::MAX;

/// The encoded sprite blob. Slices of this are handed straight to the sprite decoder.
pub static SPRITES: &[u8] = include_bytes!("../generated/sprites.bin");

include!("../generated/data.rs");

impl Entry {
    /// Borrows the species this entry belongs to.
    pub fn species(&self) -> &'static Species {
        &SPECIES[self.species as usize]
    }

    /// Reports whether this is the species' default variety and default form.
    pub fn is_default(&self) -> bool {
        self.flags & (F_DEFAULT_VARIETY | F_DEFAULT_FORM) == (F_DEFAULT_VARIETY | F_DEFAULT_FORM)
    }

    /// Reports whether this entry is an alternate variety, so it has its own stats.
    ///
    /// This is what `random --include-varieties` lets into the pool.
    pub fn is_alt_variety(&self) -> bool {
        self.flags & F_DEFAULT_VARIETY == 0 && self.flags & F_DEFAULT_FORM != 0
    }

    /// Reports whether this entry is an alternate form, so it is cosmetic and shares its
    /// variety's stats.
    ///
    /// This is what `random --include-forms` lets into the pool.
    pub fn is_alt_form(&self) -> bool {
        self.flags & F_DEFAULT_FORM == 0
    }

    /// Reports whether a distinct female sprite exists.
    pub fn has_female(&self) -> bool {
        self.flags & F_HAS_FEMALE != 0
    }

    /// Reports whether the shiny slot is really just the default sprite.
    pub fn no_shiny(&self) -> bool {
        self.flags & F_NO_SHINY != 0
    }

    /// Reports whether the chosen gender lacks a shiny sprite.
    pub fn no_shiny_for(&self, female: bool) -> bool {
        if female {
            self.flags & F_NO_SHINY_FEMALE != 0
        } else {
            self.no_shiny()
        }
    }

    /// Reports whether this entry only ever appears mid-battle, like a Mega or Mimikyu-Busted.
    pub fn is_battle_only(&self) -> bool {
        self.flags & F_BATTLE_ONLY != 0
    }

    /// Sums the six base stats.
    pub fn stat_total(&self) -> u16 {
        self.stats.iter().map(|s| *s as u16).sum()
    }

    /// Yields this entry's types, one or two of them.
    pub fn type_infos(&self) -> impl Iterator<Item = &'static TypeInfo> + '_ {
        self.types
            .iter()
            .filter(|t| **t != NO_TYPE)
            .map(|t| &TYPES[*t as usize])
    }
}

impl Species {
    /// Borrows this species' slice of the flat selector table.
    pub fn selectors(&self) -> &'static [(&'static str, u32)] {
        &SELECTORS[self.selector_start as usize..self.selector_end as usize]
    }

    /// Yields every entry id belonging to this species, in game order.
    pub fn entry_ids(&self) -> std::ops::Range<u32> {
        self.entry_start..self.entry_end
    }
}

/// Borrows an entry by id.
pub fn entry(id: u32) -> &'static Entry {
    &ENTRIES[id as usize]
}

/// Borrows a species by index.
pub fn species(idx: u32) -> &'static Species {
    &SPECIES[idx as usize]
}

/// Returns the encoded sprite bytes for one entry and one appearance.
///
/// Every combination is populated at generation time, so there is no fallback to do here.
///
/// # Arguments
/// * `entry_id` - the entry to draw
/// * `shiny` - whether to use the shiny slot
/// * `female` - whether to use the female slot
///
/// # Returns
/// the slice of `SPRITES` holding that sprite
pub fn sprite_bytes(entry_id: u32, shiny: bool, female: bool) -> &'static [u8] {
    let slot = entry_id * 4 + (shiny as u32) * 2 + female as u32;
    let (offset, len) = SPRITE_DIR[SPRITE_SLOTS[slot as usize] as usize];
    &SPRITES[offset as usize..(offset + len) as usize]
}

/// Whether the selected appearance has a visibly different female sprite.
/// Single-gender and genderless species never receive an additional female marker.
pub fn female_sprite_differs(entry_id: u32, shiny: bool) -> bool {
    let rate = entry(entry_id).species().gender_rate;
    let slot = entry_id as usize * 4 + usize::from(shiny) * 2;
    (1..8).contains(&rate) && SPRITE_SLOTS[slot] != SPRITE_SLOTS[slot + 1]
}

/// Looks up a region index by name.
///
/// # Arguments
/// * `name` - one of the `REGIONS` names
///
/// # Returns
/// the stored region value (index plus one), or `None` if the name is not a region
pub fn region_id(name: &str) -> Option<u8> {
    REGIONS.iter().position(|r| *r == name).map(|i| i as u8 + 1)
}
