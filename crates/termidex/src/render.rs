//! The text blocks that go under the sprite: name, dex line, Pokédex entry and stat bars.

use crate::color::{self, Caps};
use crate::data::{self, Entry, Rgb};

/// Stat row labels, in the order `Entry::stats` stores them.
const STAT_LABELS: [&str; 6] = ["HP", "Attack", "Defense", "Sp. Atk", "Sp. Def", "Speed"];

/// Width of the label column, wide enough for `Defense` and `Sp. Atk`.
const LABEL_WIDTH: usize = 7;

/// How many cells a full 255 stat fills.
const BAR_CELLS: u32 = 30;

/// The highest base stat any Pokémon has, which every bar is scaled against.
const STAT_MAX: u32 = 255;

/// Eighth-width block characters, from one eighth up to a full block.
const EIGHTHS: [&str; 8] = ["▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];

/// The widest a Pokédex entry is ever wrapped, however wide the terminal is.
const MAX_WRAP: usize = 80;

/// One global color scale for every stat. This is a display convention, not anything the games
/// define. Each row is the inclusive lower bound of a band.
const STAT_SCALE: [(u8, Rgb); 6] = [
    (150, Rgb(64, 224, 208)), // turquoise
    (120, Rgb(46, 139, 56)),  // dark green
    (90, Rgb(122, 199, 76)),  // green
    (60, Rgb(240, 208, 68)),  // yellow
    (30, Rgb(240, 142, 50)),  // orange
    (0, Rgb(230, 62, 62)),    // red
];

/// Appends the display name, marking a shiny with a star and a distinct female look with the sign.
pub fn name_line(out: &mut Vec<u8>, entry: &Entry, shiny: bool, female: bool) {
    out.extend_from_slice(entry.display_name.as_bytes());
    if female && (1..8).contains(&entry.species().gender_rate) {
        out.extend_from_slice(" ♀".as_bytes());
    }
    if shiny {
        out.extend_from_slice(" ★".as_bytes());
    }
    out.push(b'\n');
}

/// Appends the one-line dex summary: genus, types, height and weight in both unit systems.
pub fn dex_line(out: &mut Vec<u8>, entry: &Entry, caps: Caps) {
    let mut fields: Vec<Vec<u8>> = Vec::with_capacity(4);

    let genus = entry.species().genus;
    if !genus.is_empty() {
        fields.push(genus.as_bytes().to_vec());
    }

    let mut types = Vec::new();
    for (i, info) in entry.type_infos().enumerate() {
        if i > 0 {
            types.extend_from_slice(b" / ");
        }
        match info.color {
            // dual types are colored one at a time, so each keeps its own color
            Some(c) if caps.color_text => {
                color::set_fg(&mut types, c, caps.mode);
                types.extend_from_slice(info.name.as_bytes());
                color::reset(&mut types);
            }
            _ => types.extend_from_slice(info.name.as_bytes()),
        }
    }
    if !types.is_empty() {
        fields.push(types);
    }

    let mut height = Vec::new();
    push_tenths(&mut height, entry.height as u32);
    height.extend_from_slice(b" m (");
    let inches = (entry.height as u32 * 39370 + 5000) / 10000;
    color::push_num(&mut height, inches / 12);
    height.extend_from_slice("\u{2032}".as_bytes());
    let rem = inches % 12;
    if rem < 10 {
        height.push(b'0');
    }
    color::push_num(&mut height, rem);
    height.extend_from_slice("\u{2033})".as_bytes());
    fields.push(height);

    let mut weight = Vec::new();
    push_tenths(&mut weight, entry.weight as u32);
    weight.extend_from_slice(b" kg (");
    push_tenths(&mut weight, (entry.weight as u32 * 220462 + 50000) / 100000);
    weight.extend_from_slice(b" lbs)");
    fields.push(weight);

    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b"  ");
        }
        out.extend_from_slice(field);
    }
    out.push(b'\n');
}

/// Appends the Pokédex entry, wrapped to the terminal width.
///
/// The text is species-level in PokeAPI, so every form of a species shows the same paragraph.
pub fn flavor_text(out: &mut Vec<u8>, entry: &Entry, width: usize) {
    let Some(text) = entry.species().flavor else {
        out.extend_from_slice(b"No Pok\xc3\xa9dex entry available.\n");
        return;
    };

    wrap_text(out, text, width);
}

fn wrap_text(out: &mut Vec<u8>, text: &str, width: usize) {
    let mut column = 0usize;
    for word in text.split_whitespace() {
        let len = word.chars().count();
        if column > 0 && column + 1 + len > width {
            out.push(b'\n');
            column = 0;
        } else if column > 0 {
            out.push(b' ');
            column += 1;
        }
        out.extend_from_slice(word.as_bytes());
        column += len;
    }
    out.push(b'\n');
}

/// Appends the base-stat bar block.
pub fn stat_bars(out: &mut Vec<u8>, entry: &Entry, caps: Caps) {
    out.extend_from_slice(b"Base Stats\n");
    for (i, label) in STAT_LABELS.iter().enumerate() {
        let value = entry.stats[i];
        out.extend_from_slice(label.as_bytes());
        for _ in label.chars().count()..LABEL_WIDTH {
            out.push(b' ');
        }
        push_right(out, value as u32, 4);
        out.push(b' ');
        bar(out, value, caps);
        out.push(b'\n');
    }
    out.extend_from_slice(b"Total  ");
    push_right(out, entry.stat_total() as u32, 4);
    out.push(b'\n');
}

/// Appends one bar, using eighth blocks so short stats still show something.
fn bar(out: &mut Vec<u8>, value: u8, caps: Caps) {
    let color = STAT_SCALE
        .iter()
        .find(|(floor, _)| value >= *floor)
        .map(|(_, c)| *c)
        .unwrap_or(Rgb(230, 62, 62));
    if caps.color_text {
        color::set_fg(out, color, caps.mode);
    }

    // round a tiny non-zero stat up to one eighth so it is still visible
    let eighths = (value as u32 * BAR_CELLS * 8 / STAT_MAX).max((value > 0) as u32);
    for _ in 0..eighths / 8 {
        out.extend_from_slice(EIGHTHS[7].as_bytes());
    }
    let remainder = (eighths % 8) as usize;
    if remainder > 0 {
        out.extend_from_slice(EIGHTHS[remainder - 1].as_bytes());
    }

    if caps.color_text {
        color::reset(out);
    }
}

/// Appends a number that PokeAPI stores in tenths, so 60 becomes `6.0`.
fn push_tenths(out: &mut Vec<u8>, tenths: u32) {
    color::push_num(out, tenths / 10);
    out.push(b'.');
    color::push_num(out, tenths % 10);
}

/// Appends a number right-aligned in a fixed column.
fn push_right(out: &mut Vec<u8>, n: u32, width: usize) {
    let digits = if n == 0 { 1 } else { n.ilog10() as usize + 1 };
    for _ in digits..width {
        out.push(b' ');
    }
    color::push_num(out, n);
}

/// The width to wrap Pokédex text at: the terminal's, capped at 80.
///
/// # Returns
/// a column count of at least 20, so wrapping never degenerates
pub fn wrap_width() -> usize {
    let cols = terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(MAX_WRAP);
    cols.clamp(20, MAX_WRAP)
}

/// Appends a species line for the `list` subcommand.
///
/// The shape is `0006 charizard  mega-x, mega-y, gmax`, one species per line sorted by dex number,
/// so it is easy to grep. When a region is given only that region's alternates are listed, because
/// a species can have alternates from several regions and showing all of them under one region
/// would be misleading.
///
/// # Arguments
/// * `out` - the output buffer to append to
/// * `species_idx` - the species to print
/// * `region` - a stored region value to filter alternates by, or `None` for all of them
pub fn list_line(out: &mut Vec<u8>, species_idx: u32, region: Option<u8>) {
    let species = data::species(species_idx);
    let dex = species.dex as u32;
    for divisor in [1000, 100, 10, 1] {
        out.push(b'0' + ((dex / divisor) % 10) as u8);
    }
    out.push(b' ');
    out.extend_from_slice(species.slug.as_bytes());

    let selectors = selectors(species_idx, region);
    if !selectors.is_empty() {
        out.extend_from_slice(b"  ");
        out.extend_from_slice(selectors.join(", ").as_bytes());
    }
    out.push(b'\n');
}

/// Collects a species' alternate selectors in game order.
///
/// Varieties and forms are listed together, because `--form` takes either and which one a given
/// alternate happens to be is PokeAPI's own modelling rather than anything a user needs to know.
///
/// # Arguments
/// * `species_idx` - the species to list
/// * `region` - a stored region value to filter by, or `None` for every alternate
///
/// # Returns
/// the raw selectors, empty when the species has no matching alternates
pub fn selectors(species_idx: u32, region: Option<u8>) -> Vec<&'static str> {
    data::species(species_idx)
        .entry_ids()
        .map(data::entry)
        .filter(|e| !e.is_default() && region.is_none_or(|r| e.region == r))
        .map(|e| e.selector)
        .collect()
}

/// Collects `selector (Display Name)` lines for `--list`.
///
/// # Arguments
/// * `species_idx` - the species to list
///
/// # Returns
/// one line per alternate, empty when the species has none
pub fn alternates(species_idx: u32) -> Vec<String> {
    data::species(species_idx)
        .entry_ids()
        .map(data::entry)
        .filter(|e| !e.is_default())
        .map(|e| format!("{} ({})", e.selector, e.display_name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> Caps {
        Caps {
            mode: color::ColorMode::Truecolor,
            color_text: false,
        }
    }

    /// a 255 stat should fill every cell and a 1 stat should still show a sliver
    #[test]
    fn bars_scale_against_255() {
        let mut full = Vec::new();
        bar(&mut full, 255, plain());
        assert_eq!(String::from_utf8(full).unwrap(), "█".repeat(30));

        let mut sliver = Vec::new();
        bar(&mut sliver, 1, plain());
        assert_eq!(String::from_utf8(sliver).unwrap(), "▏");

        let mut zero = Vec::new();
        bar(&mut zero, 0, plain());
        assert_eq!(String::from_utf8(zero).unwrap(), "");

        // 5 of 255 across 30 cells is four eighths, so half a cell
        let mut small = Vec::new();
        bar(&mut small, 5, plain());
        assert_eq!(String::from_utf8(small).unwrap(), "▌");

        // 60 of 255 is 56 eighths, which is exactly seven whole cells
        let mut mid = Vec::new();
        bar(&mut mid, 60, plain());
        assert_eq!(String::from_utf8(mid).unwrap(), "███████");

        // 61 pushes it one eighth past seven cells
        let mut past = Vec::new();
        bar(&mut past, 61, plain());
        assert_eq!(String::from_utf8(past).unwrap(), "███████▏");
    }

    /// wrapping should break between words and never mid word
    #[test]
    fn flavor_wraps_on_words() {
        let mut out = Vec::new();
        wrap_text(&mut out, "one two three four five", 9);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "one two\nthree\nfour five\n"
        );
    }

    /// tenths should print with exactly one decimal place
    #[test]
    fn tenths_print_one_decimal() {
        let mut out = Vec::new();
        push_tenths(&mut out, 4);
        out.push(b' ');
        push_tenths(&mut out, 1450);
        assert_eq!(String::from_utf8(out).unwrap(), "0.4 145.0");
    }

    #[test]
    fn a_female_marker_cannot_be_added_to_a_single_gender_or_genderless_species() {
        for (slug, name) in [
            ("nidoqueen", "Nidoqueen"),
            ("nidoran-f", "Nidoran♀"),
            ("tauros", "Tauros"),
            ("voltorb", "Voltorb"),
        ] {
            let species = data::SPECIES.iter().find(|s| s.slug == slug).unwrap();
            let mut out = Vec::new();
            name_line(&mut out, data::entry(species.default_entry), false, true);
            assert_eq!(String::from_utf8(out).unwrap(), format!("{name}\n"));
        }
    }
}
