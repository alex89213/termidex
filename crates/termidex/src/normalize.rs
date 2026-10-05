//! Name normalization shared by the runtime and the generator.
//!
//! `termidex-gen` compiles this exact file in through a `#[path]` module, so the keys it bakes
//! into the lookup tables and the keys the binary computes can never drift apart. Keep it free of
//! anything outside `std`.

/// Folds a user-typed name or a slug down to a lookup key.
///
/// Lowercases, maps the three characters that show up in Pokémon names but not on a keyboard
/// (`é`, `♀`, `♂`), then drops everything that is not alphanumeric. So `Mr. Mime`, `mr-mime` and
/// `MRMIME` all come out as `mrmime`.
///
/// # Arguments
/// * `s` - the raw name, slug or selector
///
/// # Returns
/// the normalized key
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            'é' | 'É' => out.push('e'),
            '♀' => out.push('f'),
            '♂' => out.push('m'),
            c if c.is_ascii_alphanumeric() => out.push(c.to_ascii_lowercase()),
            // keep any other letter or digit so nothing is silently merged, the generator
            // reports these so a new one cannot sneak in unnoticed
            c if c.is_alphanumeric() => out.extend(c.to_lowercase()),
            _ => {}
        }
    }
    out
}

/// Reports whether a normalized key still holds a character outside `[a-z0-9]`.
///
/// # Arguments
/// * `key` - an already normalized key
///
/// # Returns
/// `true` if some character survived normalization that the explicit mappings do not cover
pub fn has_unmapped_char(key: &str) -> bool {
    !key.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}
