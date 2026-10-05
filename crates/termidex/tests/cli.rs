//! End-to-end tests over the real generated data, driving the built binary.
//!
//! These cover the behavior a user sees: name spellings, `--form` resolution, `--list`, the pool
//! filters, seeds and exit codes. Rendering, encoding and argv parsing are unit tested next to the
//! code they belong to, including the property test that keeps the argv fast path and clap in step.

use std::process::{Command, Output};

/// Runs termidex with a fixed environment so output never depends on the developer's terminal.
///
/// # Arguments
/// * `args` - the command line, without the program name
///
/// # Returns
/// the finished process output
fn run(args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_termidex"));
    command.args(args);
    // Pin the incidental shiny draw so comparisons never fail one time in 4096.
    if !args
        .iter()
        .any(|a| *a == "--shiny-rate" || a.starts_with("--shiny-rate="))
    {
        command.args(["--shiny-rate", "0"]);
    }
    command
        .env("COLORTERM", "truecolor")
        .env_remove("NO_COLOR")
        .env_remove("WT_SESSION")
        .output()
        .expect("termidex should start")
}

fn stdout(args: &[&str]) -> String {
    let out = run(args);
    assert!(
        out.status.success(),
        "termidex {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("output should be utf8")
}

fn stderr(args: &[&str]) -> String {
    String::from_utf8(run(args).stderr).expect("stderr should be utf8")
}

fn code(args: &[&str]) -> i32 {
    run(args)
        .status
        .code()
        .expect("process should exit normally")
}

/// Reads the `--name` line, which is the first line after the sprite.
fn shown_name(args: &[&str]) -> String {
    let text = stdout(args);
    text.lines()
        .last()
        .expect("there should be a name line")
        .to_string()
}

/// Returns only the text blocks, dropping the sprite lines above them.
///
/// Comparing whole stdout would compare sprites too, which differ between forms even when the
/// data under them is identical.
fn text_block(args: &[&str]) -> String {
    stdout(args)
        .lines()
        .skip_while(|l| l.contains('▀') || l.contains('▄'))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// 1. the two Nidoran species
// ---------------------------------------------------------------------------

/// nidoran-f and nidoran-m should be separate species with separate dex numbers
#[test]
fn nidoran_are_two_species() {
    assert_eq!(shown_name(&["nidoran-f", "--name"]), "Nidoran♀");
    assert_eq!(shown_name(&["nidoran-m", "--name"]), "Nidoran♂");
    assert_eq!(shown_name(&["nidoranf", "--name"]), "Nidoran♀");
    assert_eq!(shown_name(&["nidoran♀", "--name"]), "Nidoran♀");

    let list = stdout(&["list"]);
    assert!(
        list.contains("0029 nidoran-f"),
        "nidoran-f should be dex 29"
    );
    assert!(
        list.contains("0032 nidoran-m"),
        "nidoran-m should be dex 32"
    );
}

/// --female on a female-only or male-only species should not fail
#[test]
fn female_flag_survives_nidoran() {
    assert_eq!(code(&["nidoran-f", "--female"]), 0);
    assert_eq!(code(&["nidoran-m", "--female"]), 0);
}

// ---------------------------------------------------------------------------
// 2. punctuation is always optional
// ---------------------------------------------------------------------------

/// hyphens, spaces, dots, apostrophes, colons and accents should all be optional
#[test]
fn awkward_names_resolve() {
    for spelling in ["mr. mime", "mr-mime", "mrmime", "mr mime", "MR. MIME"] {
        assert_eq!(
            shown_name(&[spelling, "--name"]),
            "Mr. Mime",
            "{spelling} should resolve"
        );
    }
    for spelling in ["ho-oh", "hooh", "HoOh"] {
        assert_eq!(shown_name(&[spelling, "--name"]), "Ho-Oh");
    }
    for spelling in ["tapu-koko", "tapukoko", "Tapu Koko"] {
        assert_eq!(shown_name(&[spelling, "--name"]), "Tapu Koko");
    }
    // PokeAPI spells these with a typographic apostrophe, so only check the lookup worked
    assert!(shown_name(&["farfetch'd", "--name"]).starts_with("Farfetch"));
    assert!(shown_name(&["farfetchd", "--name"]).starts_with("Farfetch"));
    assert_eq!(shown_name(&["type: null", "--name"]), "Type: Null");
    assert_eq!(shown_name(&["typenull", "--name"]), "Type: Null");
    // Flabébé's default entry is its Red form, which is what PokeAPI names it
    assert!(shown_name(&["flabébé", "--name"]).contains("Flabébé"));
    assert!(shown_name(&["flabebe", "--name"]).contains("Flabébé"));
}

/// --form should accept the same spellings a name does
#[test]
fn form_selectors_ignore_punctuation() {
    for spelling in ["mega-x", "megax", "mega x", "MEGA-X"] {
        assert_eq!(
            shown_name(&["charizard", "--form", spelling, "--name"]),
            "Mega Charizard X",
            "--form {spelling} should resolve"
        );
    }
}

/// the regional and gigantamax spellings players use should map onto PokeAPI's
#[test]
fn form_aliases_resolve() {
    for spelling in ["alola", "alolan"] {
        assert_eq!(
            shown_name(&["raichu", "--form", spelling, "--name"]),
            "Alolan Raichu"
        );
    }
    for spelling in ["gmax", "gigantamax"] {
        assert_eq!(
            shown_name(&["charizard", "--form", spelling, "--name"]),
            "Gigantamax Charizard"
        );
    }
    assert_eq!(
        shown_name(&["typhlosion", "--form", "hisuian", "--name"]),
        "Hisuian Typhlosion"
    );
    // PokeAPI's own English name for this one carries the breed in parentheses
    assert_eq!(
        shown_name(&["tauros", "--form", "paldean-combat-breed", "--name"]),
        "Paldean Tauros (Combat Breed)"
    );
}

// ---------------------------------------------------------------------------
// 3. a species resolves to its default variety, alternates need --form
// ---------------------------------------------------------------------------

/// deoxys should give the Normal forme, and --form attack should give the Attack forme
#[test]
fn deoxys_defaults_to_normal() {
    assert_eq!(shown_name(&["deoxys", "--name"]), "Normal Deoxys");
    assert_eq!(
        shown_name(&["deoxys", "--form", "attack", "--name"]),
        "Attack Deoxys"
    );
    // the default variety's own selector still works
    assert_eq!(
        shown_name(&["deoxys", "--form", "normal", "--name"]),
        "Normal Deoxys"
    );
}

/// a variety slug used as a name should fail with the --form command that would work
#[test]
fn variety_slugs_are_not_names() {
    assert_eq!(code(&["deoxys-attack"]), 1);
    assert!(
        stderr(&["deoxys-attack"]).contains("termidex deoxys --form attack"),
        "should point at the right command, got: {}",
        stderr(&["deoxys-attack"])
    );

    assert_eq!(code(&["charizard-mega-x"]), 1);
    assert!(stderr(&["charizard-mega-x"]).contains("termidex charizard --form mega-x"));
}

/// an unknown name should suggest at most three species and never dump the dex
#[test]
fn unknown_names_suggest() {
    assert_eq!(code(&["pikchu"]), 1);
    let message = stderr(&["pikchu"]);
    assert!(message.contains("pikachu"), "got: {message}");
    assert_eq!(message.lines().count(), 1, "one line only, got: {message}");
    assert!(message.matches(',').count() <= 2, "at most 3 suggestions");
}

// ---------------------------------------------------------------------------
// 4. Arceus: many forms, one set of stats, per-form types
// ---------------------------------------------------------------------------

/// arceus should list its type forms, all sharing the default variety's stats
#[test]
fn arceus_forms() {
    let listed = stdout(&["arceus", "--list"]);
    let count = listed.lines().count();
    assert!(count >= 17, "expected 17+ arceus alternates, got {count}");
    assert!(listed.contains("bug (Bug Arceus)"), "got: {listed}");

    // every alternate is cosmetic, so the stat block must match even though the sprite does not
    let base = text_block(&["arceus", "--stats"]);
    for line in listed.lines() {
        let selector = line.split(' ').next().unwrap();
        assert_eq!(
            text_block(&["arceus", "--form", selector, "--stats"]),
            base,
            "arceus --form {selector} should have the same stats"
        );
    }
}

/// a form that overrides its variety's types should show the override in --dex
#[test]
fn arceus_form_types_override() {
    let dex = stdout(&["arceus", "--form", "bug", "--dex"]);
    assert!(dex.contains("Bug"), "got: {dex}");
    assert!(
        !dex.contains("Normal"),
        "the Normal type should be gone: {dex}"
    );
}

// ---------------------------------------------------------------------------
// 5. Charizard: alternate varieties have their own stats
// ---------------------------------------------------------------------------

/// charizard's alternates should be the two Megas and Gigantamax, with their real display names
#[test]
fn charizard_alternates() {
    let list = stdout(&["charizard", "--list"]);
    assert!(list.contains("mega-x (Mega Charizard X)"), "got: {list}");
    assert!(list.contains("mega-y (Mega Charizard Y)"), "got: {list}");
    assert!(list.contains("gmax (Gigantamax Charizard)"), "got: {list}");
}

/// a Mega is a variety, so its stats must differ from the base form's
#[test]
fn mega_charizard_has_its_own_stats() {
    let base = stdout(&["charizard", "--stats"]);
    let mega = stdout(&["charizard", "--form", "mega-x", "--stats"]);
    assert_ne!(base, mega);
    // Mega Charizard X trades Sp. Atk for Attack, so its Attack row should read 130
    assert!(mega.contains("Attack  130"), "got: {mega}");
}

// ---------------------------------------------------------------------------
// 6. --form does not care whether the alternate is a variety or a form
// ---------------------------------------------------------------------------

/// busted Mimikyu and Ash's Greninja should both be reachable through --form
#[test]
fn form_flag_covers_both_kinds() {
    assert_eq!(
        shown_name(&["mimikyu", "--form", "busted", "--name"]),
        "Busted Mimikyu"
    );
    // Ash-Greninja is a variety, so it carries different stats
    assert!(shown_name(&["greninja", "--form", "ash", "--name"]).contains("Greninja"));
    assert_ne!(
        stdout(&["greninja", "--stats"]),
        stdout(&["greninja", "--form", "ash", "--stats"])
    );
}

/// an unknown selector should exit 1 and list what the species does have
#[test]
fn unknown_selector_lists_options() {
    assert_eq!(code(&["charizard", "--form", "nope"]), 1);
    let message = stderr(&["charizard", "--form", "nope"]);
    assert!(message.contains("mega-x"), "got: {message}");
    assert!(message.contains("gmax"), "got: {message}");
}

// ---------------------------------------------------------------------------
// 7. female sprites
// ---------------------------------------------------------------------------

/// pikachu has a real female sprite, charizard does not
#[test]
fn female_sprite_or_a_reason() {
    let male = stdout(&["pikachu"]);
    let female = stdout(&["pikachu", "--female"]);
    assert_ne!(male, female, "pikachu's female sprite should differ");
    assert!(stderr(&["pikachu", "--female"]).is_empty());
    // --shiny-rate 0 pins it, otherwise a 1-in-4096 shiny roll appends ★ after the ♀
    assert!(shown_name(&["pikachu", "--female", "--shiny-rate", "0", "--name"]).ends_with('♀'));

    let note = stderr(&["charizard", "--female"]);
    assert!(note.contains("no visual gender difference"), "got: {note}");
    assert_eq!(stdout(&["charizard", "--female"]), stdout(&["charizard"]));
}

/// the female marker should only appear when the sprite genuinely differs
///
/// PokeAPI often fills front_female with the same URL as front_default, so a female-only Pokémon
/// like Nidoqueen was flagged as having a female sprite and printed ♀ over an identical image. The
/// generator now clears the flag when the two sprites dedupe to the same blob entry.
#[test]
fn female_marker_needs_a_different_sprite() {
    use termidex::data;

    // Nidoqueen is female-only and its "female" sprite is the default one
    assert_eq!(stdout(&["nidoqueen", "--female"]), stdout(&["nidoqueen"]));
    assert!(!shown_name(&["nidoqueen", "--female", "--shiny-rate", "0", "--name"]).contains('♀'));
    assert!(stderr(&["nidoqueen", "--female"]).contains("female-only"));

    // Pikachu genuinely differs, so it still gets the marker and no note
    assert_ne!(stdout(&["pikachu", "--female"]), stdout(&["pikachu"]));
    assert!(shown_name(&["pikachu", "--female", "--shiny-rate", "0", "--name"]).ends_with('♀'));
    assert!(stderr(&["pikachu", "--female"]).is_empty());

    // every entry still flagged must actually have a distinct sprite in the blob
    for id in 0..data::ENTRY_COUNT as u32 {
        let entry = data::entry(id);
        let differs = data::sprite_bytes(id, false, false) != data::sprite_bytes(id, false, true);
        assert_eq!(
            entry.has_female(),
            differs,
            "{} claims has_female {} but the sprites differ: {differs}",
            entry.display_name,
            entry.has_female()
        );
    }
}

/// Check the actual CLI name for every entry and both shiny settings.
#[test]
fn every_female_marker_matches_the_rendered_appearance() {
    use termidex::data;

    for id in 0..data::ENTRY_COUNT as u32 {
        let entry = data::entry(id);
        let species = entry.species();
        for shiny_rate in ["0", "1"] {
            let mut args = vec![
                species.slug,
                "--female",
                "--name",
                "--shiny-rate",
                shiny_rate,
            ];
            if !entry.is_default() {
                args.extend(["--form", entry.selector]);
            }
            let female = entry.has_female();
            let shiny = shiny_rate == "1" && !entry.no_shiny_for(female);
            let differs =
                data::sprite_bytes(id, shiny, false) != data::sprite_bytes(id, shiny, true);
            let marker = female && (1..8).contains(&species.gender_rate) && differs;
            let expected = format!(
                "{}{}{}",
                entry.display_name,
                if marker { " ♀" } else { "" },
                if shiny { " ★" } else { "" }
            );
            assert_eq!(shown_name(&args), expected, "{args:?}");
            assert_eq!(
                data::female_sprite_differs(id, shiny),
                (1..8).contains(&species.gender_rate) && differs,
                "entry {id}, shiny {shiny}"
            );
        }
    }
}

/// the fallback note should name the actual reason
#[test]
fn female_notes_are_specific() {
    assert!(stderr(&["magnemite", "--female"]).contains("genderless"));
    assert!(stderr(&["tauros", "--female"]).contains("male-only"));
    assert!(stderr(&["charizard", "--female"]).contains("no visual gender difference"));
    // a female-only species has nothing to switch to, the default already is the female
    assert!(stderr(&["chansey", "--female"]).contains("female-only"));
}

#[test]
fn random_female_fallback_also_explains_the_reason() {
    let args = ["random", "--gen", "1", "--seed", "1", "--name"];
    assert_eq!(shown_name(&args), "Voltorb");
    let forced = [args.as_slice(), &["--female"]].concat();
    assert_eq!(stdout(&args), stdout(&forced));
    assert!(stderr(&forced).contains("genderless"));
}

/// a form with its own sprite should not borrow a "female" one from its variety
///
/// Red Florges has its own front_default but no front_female, and taking the female slot from the
/// variety paired it with the plain Florges sprite, a different picture of the same Pokémon.
#[test]
fn female_sprite_comes_from_the_same_record() {
    use termidex::data;

    for name in ["flabebe", "floette", "florges"] {
        assert_eq!(stdout(&[name, "--female"]), stdout(&[name]));
        assert!(!shown_name(&[name, "--female", "--shiny-rate", "0", "--name"]).contains('♀'));
        assert!(stderr(&[name, "--female"]).contains("female-only"));
    }

    // a species with one gender, or none, can never have a separate female look
    for id in 0..data::ENTRY_COUNT as u32 {
        let entry = data::entry(id);
        if matches!(entry.species().gender_rate, -1 | 0 | 8) {
            assert!(
                !entry.has_female(),
                "{} has a female sprite",
                entry.display_name
            );
        }
    }
}

/// species split by gender reach their female through --form only, so no command has two spellings
///
/// Pyroar's default entry is "Male Pyroar", and its variety also carries a female sprite, which
/// printed "Male Pyroar ♀" over the male's stats. The female is its own entry, so --female points
/// there instead of showing a second copy of it.
#[test]
fn gender_split_species_reach_their_female_through_form() {
    use termidex::data;

    let split = [
        "frillish",
        "jellicent",
        "pyroar",
        "meowstic",
        "indeedee",
        "basculegion",
        "oinkologne",
    ];
    for name in split {
        // --female changes nothing and says where the female is
        assert_eq!(
            stdout(&[name, "--female"]),
            stdout(&[name]),
            "{name} --female"
        );
        assert!(
            stderr(&[name, "--female"]).contains("use --form female"),
            "{name} --female should point at --form female"
        );
        let shown = shown_name(&[name, "--female", "--shiny-rate", "0", "--name"]);
        assert!(
            shown.starts_with("Male ") && !shown.contains('♀'),
            "{name} printed {shown}"
        );

        // --form female is the one way to ask for it
        let shown = shown_name(&[name, "--form", "female", "--shiny-rate", "0", "--name"]);
        assert!(
            shown.starts_with("Female ") && !shown.contains('♀'),
            "{name} printed {shown}"
        );
    }

    // a male alternate points at its female alternate
    assert!(
        stderr(&["meowstic", "--form", "male-mega", "--female"]).contains("use --form female-mega")
    );

    // the female entry has nothing further to switch to
    assert!(stderr(&["pyroar", "--form", "female", "--female"]).contains("already the female"));

    // exactly the male entries of those species have a counterpart, and neither side keeps a
    // female sprite of its own
    let mut paired = Vec::new();
    for id in 0..data::ENTRY_COUNT as u32 {
        if let Some(female_id) = termidex::lookup::female_counterpart(id) {
            assert_eq!(data::entry(female_id).species, data::entry(id).species);
            assert!(!data::entry(id).has_female() && !data::entry(female_id).has_female());
            paired.push(data::entry(id).display_name);
        }
    }
    paired.sort_unstable();
    let mut expected: Vec<String> = split.iter().map(|n| format!("Male {}", title(n))).collect();
    expected.push("Mega Meowstic".to_string());
    expected.sort_unstable();
    assert_eq!(paired, expected);
}

/// random has no --form, so a female roll on a gender-split species shows the female entry
#[test]
fn random_female_reaches_gender_split_entries() {
    // Gen 6 holds Male Pyroar and Male Meowstic among roughly 70 defaults, so a few hundred seeds
    // are plenty to land on them
    let mut checked = 0;
    for seed in 0..600 {
        let seed = seed.to_string();
        let plain = shown_name(&[
            "random",
            "--gen",
            "6",
            "--name",
            "--shiny-rate",
            "0",
            "--seed",
            &seed,
        ]);
        if !plain.contains("Pyroar") && !plain.contains("Meowstic") {
            continue;
        }
        checked += 1;
        let forced = shown_name(&[
            "random",
            "--gen",
            "6",
            "--name",
            "--shiny-rate",
            "0",
            "--seed",
            &seed,
            "--female",
        ]);
        assert!(
            forced.starts_with("Female ") && !forced.contains('♀'),
            "seed {seed}: {forced}"
        );
    }
    assert!(checked > 0, "no seed landed on Pyroar or Meowstic");
}

/// Capitalizes a plain ascii species slug the way its English name is written.
fn title(slug: &str) -> String {
    let mut chars = slug.chars();
    chars
        .next()
        .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// 8. shiny rate
// ---------------------------------------------------------------------------

/// rate 1 should always be shiny and rate 0 never
#[test]
fn shiny_rate_bounds() {
    for seed in 0..12 {
        let seed = seed.to_string();
        let always = shown_name(&["pikachu", "--shiny-rate", "1", "--name", "--seed", &seed]);
        assert!(always.ends_with('★'), "rate 1 should be shiny: {always}");

        let never = shown_name(&["pikachu", "--shiny-rate", "0", "--name", "--seed", &seed]);
        assert!(
            !never.contains('★'),
            "rate 0 should never be shiny: {never}"
        );
    }
    // and the sprite itself must actually change
    assert_ne!(
        stdout(&["pikachu", "--shiny-rate", "1"]),
        stdout(&["pikachu", "--shiny-rate", "0"])
    );
}

/// Mega Venusaur has a female sprite but no shiny female sprite in the source data.
#[test]
fn missing_shiny_female_keeps_the_female_without_a_star() {
    let args = ["venusaur", "--form", "mega", "--female", "--name"];
    let ordinary = stdout(&[args.as_slice(), &["--shiny-rate", "0"]].concat());
    let shiny = stdout(&[args.as_slice(), &["--shiny-rate", "1"]].concat());
    assert_eq!(ordinary, shiny);
    assert!(shiny.ends_with("Mega Venusaur ♀\n"));
    assert!(stderr(&[args.as_slice(), &["--shiny-rate", "1"]].concat()).contains("no shiny sprite"));
    // Its default-gender shiny is present and remains usable.
    assert_eq!(
        shown_name(&["venusaur", "--form", "mega", "--shiny-rate", "1", "--name"]),
        "Mega Venusaur ★"
    );
}

// ---------------------------------------------------------------------------
// 9. seeds
// ---------------------------------------------------------------------------

/// the same seed should give byte identical output, and different seeds should vary
#[test]
fn seeds_are_reproducible() {
    let first = stdout(&["random", "--seed", "12345", "--name", "--dex", "--stats"]);
    let second = stdout(&["random", "--seed", "12345", "--name", "--dex", "--stats"]);
    assert_eq!(first, second);

    let names: std::collections::HashSet<String> = (0..25)
        .map(|s| shown_name(&["random", "--seed", &s.to_string(), "--name"]))
        .collect();
    assert!(names.len() > 15, "25 seeds gave only {} names", names.len());
}

// ---------------------------------------------------------------------------
// 10. generation filter
// ---------------------------------------------------------------------------

/// random --gen 1 should only ever yield Gen 1 species
#[test]
fn gen_filter_holds_over_many_seeds() {
    let gen1: std::collections::HashSet<String> = stdout(&["list", "--gen", "1"])
        .lines()
        .map(|l| l.split_whitespace().nth(1).unwrap().to_string())
        .collect();
    assert_eq!(gen1.len(), 151, "gen 1 should have 151 species");

    for seed in 0..120 {
        let name = shown_name(&[
            "random",
            "--gen",
            "1",
            "--seed",
            &seed.to_string(),
            "--name",
        ]);
        // strip the shiny and female markers to get back to a plain display name
        let plain = name.trim_end_matches(['★', '♀', ' ']);
        let slug = termidex::normalize::normalize(plain);
        assert!(
            gen1.iter()
                .any(|g| termidex::normalize::normalize(g) == slug),
            "seed {seed} gave {plain}, which is not Gen 1"
        );
    }
}

/// multiple --gen values should union, not intersect
#[test]
fn gen_values_union() {
    let one = stdout(&["list", "--gen", "1"]).lines().count();
    let two = stdout(&["list", "--gen", "2"]).lines().count();
    let both = stdout(&["list", "--gen", "1", "2"]).lines().count();
    assert_eq!(both, one + two);
}

// ---------------------------------------------------------------------------
// 11. region filter
// ---------------------------------------------------------------------------

/// random --region R should only ever yield entries whose own games are from R
#[test]
fn region_filter_holds_over_many_seeds() {
    use termidex::data;

    let alola = data::region_id("alola").expect("alola is a region");
    let in_alola: std::collections::HashSet<&str> = (0..data::ENTRY_COUNT as u32)
        .map(data::entry)
        .filter(|e| e.region == alola)
        .map(|e| e.display_name)
        .collect();
    assert!(in_alola.len() > 100, "alola should be a real pool");

    for seed in 0..80 {
        let name = shown_name(&[
            "random",
            "--region",
            "alola",
            "--seed",
            &seed.to_string(),
            "--name",
        ]);
        let plain = name.trim_end_matches(['★', '♀', ' ']);
        assert!(
            in_alola.contains(plain),
            "seed {seed} gave {plain}, not an Alola entry"
        );
    }
}

/// --region should define its own pool, with no include flag needed
#[test]
fn region_needs_no_include_flag() {
    // the Alolan forms are alternate varieties, so this used to require --include-varieties
    for region in [
        "kanto", "johto", "hoenn", "sinnoh", "unova", "kalos", "alola", "galar", "hisui", "paldea",
    ] {
        let args = ["random", "--region", region, "--seed", "3", "--name"];
        assert_eq!(code(&args), 0, "--region {region} should work alone");
        assert!(stderr(&args).is_empty(), "{region}: {}", stderr(&args));
    }
    // and because it defines the pool, combining it with an include flag is a usage error
    assert_eq!(
        code(&["random", "--region", "alola", "--include-varieties"]),
        2
    );
    assert_eq!(code(&["random", "--region", "alola", "--include-forms"]), 2);
}

/// an entry belongs to the region of the games that introduced it, not its species'
#[test]
fn region_follows_the_entry_not_the_species() {
    // Raichu is Kanto, its Megas are Legends Z-A so Kalos, its Alolan form is Alola
    assert_eq!(
        stdout(&["list", "--region", "kanto"])
            .lines()
            .find(|l| l.starts_with("0026 ")),
        Some("0026 raichu")
    );
    assert_eq!(
        stdout(&["list", "--region", "kalos"])
            .lines()
            .find(|l| l.starts_with("0026 ")),
        Some("0026 raichu  mega-x, mega-y")
    );
    assert_eq!(
        stdout(&["list", "--region", "alola"])
            .lines()
            .find(|l| l.starts_with("0026 ")),
        Some("0026 raichu  alola")
    );

    // Charizard: base in Kanto, Megas in Kalos, Gigantamax in Galar
    assert_eq!(
        stdout(&["list", "--region", "kalos"])
            .lines()
            .find(|l| l.starts_with("0006 ")),
        Some("0006 charizard  mega-x, mega-y")
    );
    assert_eq!(
        stdout(&["list", "--region", "galar"])
            .lines()
            .find(|l| l.starts_with("0006 ")),
        Some("0006 charizard  gmax")
    );

    // Kyogre and Primal Kyogre are both Hoenn, because ORAS is a Hoenn game even though it is Gen 6
    assert_eq!(
        stdout(&["list", "--region", "hoenn"])
            .lines()
            .find(|l| l.starts_with("0382 ")),
        Some("0382 kyogre  primal")
    );
}

/// list --region should only show species with an entry from that region
#[test]
fn list_region_filters_species() {
    let alola = stdout(&["list", "--region", "alola"]);
    assert!(alola.contains("0026 raichu  alola"), "got: {alola}");
    assert!(alola.contains("0785 tapu-koko"), "Alola natives belong too");
    // Bulbasaur has no Alola entry at all
    assert!(!alola.lines().any(|l| l.starts_with("0001 ")));
}

/// forms that only exist mid-battle should be out of the pool unless asked for
///
/// A Pokédex cannot show you a Mega, so they do not belong in a default pool. They are also the
/// same shape as the alternates that do belong (154 of the 156 are varieties), so the shape flags
/// cannot separate them, which is why they get an axis of their own.
#[test]
fn battle_forms_are_off_by_default() {
    use termidex::data;

    // no species disappears, because no species' default entry is battle-only
    assert!(!(0..data::ENTRY_COUNT as u32)
        .map(data::entry)
        .any(|e| e.is_battle_only() && e.is_default()));

    let megas = |args: &[&str]| {
        (0..90)
            .map(|s| shown_name(&[args, &["--seed", &s.to_string(), "--name"]].concat()))
            .filter(|n| {
                n.starts_with("Mega ") || n.starts_with("Primal ") || n.starts_with("Gigantamax ")
            })
            .count()
    };
    assert_eq!(
        megas(&["random", "--include-varieties"]),
        0,
        "off by default"
    );
    assert!(
        megas(&["random", "--include-varieties", "--battle-forms"]) > 0,
        "on with the flag"
    );
    // --region admits every shape but still not battle forms
    assert_eq!(megas(&["random", "--region", "kalos"]), 0);
    assert!(megas(&["random", "--region", "kalos", "--battle-forms"]) > 0);
    // and the flag is meaningful on its own, adding battle forms to the default pool
    assert!(megas(&["random", "--battle-forms"]) > 0);
}

/// the three include axes should cover the non-default entries exactly once each
#[test]
fn include_axes_partition_the_alternates() {
    use termidex::data;
    let (mut varieties, mut forms, mut battle, mut non_default) = (0, 0, 0, 0);
    for id in 0..data::ENTRY_COUNT as u32 {
        let e = data::entry(id);
        if e.is_default() {
            continue;
        }
        non_default += 1;
        match (e.is_battle_only(), e.is_alt_variety()) {
            (true, _) => battle += 1,
            (false, true) => varieties += 1,
            (false, false) => forms += 1,
        }
    }
    assert_eq!(varieties + forms + battle, non_default);
    assert!(varieties > 0 && forms > 0 && battle > 0);
}

/// --gen should accept inclusive ranges as well as single values
#[test]
fn gen_accepts_ranges() {
    // a range and the values it covers should give the same species list
    assert_eq!(
        stdout(&["list", "--gen", "1-3"]),
        stdout(&["list", "--gen", "1", "2", "3"])
    );
    // ranges and singles can be mixed
    assert_eq!(
        stdout(&["list", "--gen", "1-2", "5"]),
        stdout(&["list", "--gen", "1", "2", "5"])
    );
    // a one-wide range is just that generation
    assert_eq!(
        stdout(&["list", "--gen", "4-4"]),
        stdout(&["list", "--gen", "4"])
    );
    // duplicates collapse rather than listing a species twice
    assert_eq!(
        stdout(&["list", "--gen", "1-3", "2"]),
        stdout(&["list", "--gen", "1-3"])
    );

    // a backwards range and an out-of-range generation are usage errors
    assert_eq!(code(&["random", "--gen", "7-1"]), 2);
    assert!(stderr(&["random", "--gen", "7-1"]).contains("backwards"));
    assert_eq!(code(&["random", "--gen", "1-99"]), 2);
    assert_eq!(code(&["random", "--gen", "x-3"]), 2);
}

/// the scenario that motivated the battle-forms axis
///
/// Everything from Gens 1 to 7 with all the cosmetic alternates, including the Pikachu hats, but
/// no Megas: Mega Raichu is Gen 9 and Gigantamax is Gen 8 so the range excludes those, while Mega
/// Charizard is Gen 6 and needs the battle-forms default to keep it out.
#[test]
fn gens_one_to_seven_with_hats_but_no_megas() {
    let mut names = std::collections::HashSet::new();
    for seed in 0..250 {
        names.insert(shown_name(&[
            "random",
            "--gen",
            "1-7",
            "--include-varieties",
            "--include-forms",
            "--seed",
            &seed.to_string(),
            "--name",
        ]));
    }
    assert!(
        names.iter().all(|n| !n.starts_with("Mega ")
            && !n.starts_with("Primal ")
            && !n.starts_with("Gigantamax ")),
        "no battle form should appear"
    );
    assert!(
        names
            .iter()
            .any(|n| n.contains("Cap Pikachu") || n.contains("Pikachu ")),
        "the Pikachu hats should be reachable, got {names:?}"
    );
}

// ---------------------------------------------------------------------------
// 12. usage errors exit 2
// ---------------------------------------------------------------------------

/// contradictory or unknown flags should be usage errors, not lookup errors
#[test]
fn usage_errors_exit_two() {
    // --list prints a list and nothing else, so it rules out every display flag and --form
    assert_eq!(code(&["charizard", "--list", "--stats"]), 2);
    assert_eq!(code(&["charizard", "--list", "--name"]), 2);
    assert_eq!(code(&["charizard", "--list", "--form", "mega-x"]), 2);
    // --list-forms and --list-varieties were merged into --list, so they are unknown now
    assert_eq!(code(&["charizard", "--list-forms"]), 2);
    assert_eq!(code(&["charizard", "--list-varieties"]), 2);
    // a generation the data has no Pokémon for
    assert_eq!(code(&["random", "--gen", "99"]), 2);
    // a region that is not a region
    assert_eq!(code(&["random", "--region", "orre"]), 2);
    assert_eq!(code(&["random", "--region", "nowhere"]), 2);
    // --region defines its own pool, so pairing it with an include flag is contradictory
    assert_eq!(
        code(&["random", "--region", "kanto", "--include-varieties"]),
        2
    );
    assert_eq!(code(&["random", "--region", "alola", "--include-forms"]), 2);
}

/// No root arguments should disappear when clap recognizes a later subcommand.
#[test]
fn incomplete_or_mixed_modes_are_usage_errors() {
    for args in [
        vec!["--name"],
        vec!["--form", "mega-x"],
        vec!["--list"],
        vec!["--seed", "1"],
        vec!["pikachu", "random"],
        vec!["pikachu", "list"],
        vec!["--form", "mega-x", "random"],
        vec!["--list", "random"],
        vec!["--name", "random"],
        vec!["--female", "list"],
    ] {
        let result = run(&args);
        assert_eq!(result.status.code(), Some(2), "{args:?}");
        assert!(
            result.stdout.is_empty(),
            "{args:?} should not render anything"
        );
    }
}

/// A failed output destination must not be reported as a successful render.
#[cfg(target_os = "linux")]
#[test]
fn output_errors_exit_one() {
    let result = Command::new(env!("CARGO_BIN_EXE_termidex"))
        .args(["list"])
        .stdout(
            std::fs::File::options()
                .write(true)
                .open("/dev/full")
                .unwrap(),
        )
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot write stdout"));
}

// ---------------------------------------------------------------------------
// 14. render order is fixed
// ---------------------------------------------------------------------------

/// flag order on the command line must not change the output order
#[test]
fn render_order_is_independent_of_flag_order() {
    let canonical = stdout(&["charizard", "--name", "--dex", "--entry", "--stats"]);
    for order in [
        ["--stats", "--entry", "--dex", "--name"],
        ["--entry", "--stats", "--name", "--dex"],
        ["--dex", "--name", "--stats", "--entry"],
    ] {
        let args = ["charizard", order[0], order[1], order[2], order[3]];
        assert_eq!(stdout(&args), canonical, "order {order:?} differed");
    }

    // and the blocks really are in the documented order
    let text = stdout(&["charizard", "--name", "--dex", "--entry", "--stats"]);
    let name_at = text.find("Charizard").expect("name line");
    let dex_at = text.find("Flame Pokémon").expect("dex line");
    let stats_at = text.find("Base Stats").expect("stat block");
    assert!(name_at < dex_at && dex_at < stats_at);
}

// ---------------------------------------------------------------------------
// extra coverage on the pieces the checklist calls out
// ---------------------------------------------------------------------------

/// the dex line should carry genus, types and both unit systems
#[test]
fn dex_line_shape() {
    let text = stdout(&["pikachu", "--dex"]);
    let line = text.lines().last().unwrap();
    assert!(line.contains("Mouse Pokémon"), "got: {line}");
    assert!(line.contains("Electric"), "got: {line}");
    assert!(line.contains("0.4 m (1′04″)"), "got: {line}");
    assert!(line.contains("6.0 kg (13.2 lbs)"), "got: {line}");
}

/// NO_COLOR should strip color from text but leave the sprite in color
#[test]
fn no_color_keeps_the_sprite() {
    let colored = stdout(&["pikachu", "--dex", "--stats"]);
    let plain = String::from_utf8(
        Command::new(env!("CARGO_BIN_EXE_termidex"))
            .args(["pikachu", "--dex", "--stats"])
            .env("COLORTERM", "truecolor")
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();

    assert_ne!(colored, plain);
    // the sprite is still escape heavy, so only the trailing text block should have lost color
    let text_only = |s: &str| {
        s.lines()
            .skip_while(|l| l.contains('▀') || l.contains('▄'))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(!text_only(&plain).contains('\x1b'), "text should be plain");
    assert!(
        text_only(&colored).contains('\x1b'),
        "text should be colored"
    );
    assert!(plain.contains('\x1b'), "the sprite should keep its color");
}

/// a species with no Pokédex text should say so rather than print nothing
#[test]
fn flavor_text_always_prints_something() {
    let text = stdout(&["charizard", "--entry"]);
    let body: Vec<&str> = text
        .lines()
        .skip_while(|l| l.contains('▀') || l.contains('▄'))
        .collect();
    assert!(!body.is_empty());
    assert!(
        body.iter().all(|l| l.chars().count() <= 80),
        "wrapped to 80"
    );
}

/// the pool should widen as the include flags are added
#[test]
fn include_flags_widen_the_pool() {
    // charizard's Megas are varieties, so they can only turn up with --include-varieties
    let mut plain_names = std::collections::HashSet::new();
    let mut wide_names = std::collections::HashSet::new();
    for seed in 0..40 {
        let s = seed.to_string();
        plain_names.insert(shown_name(&["random", "--seed", &s, "--name"]));
        wide_names.insert(shown_name(&[
            "random",
            "--include-varieties",
            "--include-forms",
            "--seed",
            &s,
            "--name",
        ]));
    }
    assert!(plain_names.iter().all(|n| !n.starts_with("Mega ")));
    assert_ne!(plain_names, wide_names);
}

/// a global flag before a subcommand should still reach the subcommand
///
/// This used to route `random` into the name lookup and fail, because
/// args_conflicts_with_subcommands makes clap stop looking for a subcommand once it has seen any
/// argument. Dropping that setting fixed it.
#[test]
fn globals_may_precede_a_subcommand() {
    for args in [
        vec!["--seed", "7", "random", "--name"],
        vec!["--shiny-rate", "1", "random", "--name"],
        vec!["--seed", "7", "list"],
    ] {
        assert_eq!(code(&args), 0, "{args:?} should work");
        assert!(stderr(&args).is_empty(), "{args:?}: {}", stderr(&args));
    }
    // and the same seed either side of the subcommand gives the same Pokémon
    assert_eq!(
        stdout(&["--seed", "99", "random", "--name"]),
        stdout(&["random", "--seed", "99", "--name"])
    );
}

/// every entry's sprite should decode and render without panicking
#[test]
fn every_entry_renders() {
    use termidex::color::ColorMode;
    use termidex::sprite::Sprite;

    for id in 0..termidex::data::ENTRY_COUNT as u32 {
        for (shiny, female) in [(false, false), (true, false), (false, true), (true, true)] {
            let bytes = termidex::data::sprite_bytes(id, shiny, female);
            let sprite = Sprite::decode(bytes);
            assert!(sprite.width > 0 && sprite.height > 0, "entry {id} is empty");
            assert!(
                sprite.width <= 96 && sprite.height <= 96,
                "entry {id} is oversized"
            );
            let mut out = Vec::new();
            sprite.render(&mut out, ColorMode::Ansi256);
            assert!(!out.is_empty(), "entry {id} rendered nothing");
        }
    }
}

/// the generator's invariants should hold in the data that shipped
#[test]
fn generated_data_invariants() {
    use termidex::data;

    assert_eq!(data::SPRITE_SLOTS.len(), data::ENTRY_COUNT * 4);
    assert_eq!(data::SPECIES.len(), data::SPECIES_COUNT);

    for name in ["random", "list", "help"] {
        assert!(
            data::NAMES.get(name).is_none(),
            "'{name}' would shadow a subcommand"
        );
    }

    let mut previous_dex = 0;
    for idx in 0..data::SPECIES_COUNT as u32 {
        let species = data::species(idx);
        assert!(species.dex > previous_dex, "species must be dex sorted");
        previous_dex = species.dex;

        // the default entry is the first of the species' run and really is the default
        assert_eq!(species.default_entry, species.entry_start);
        assert!(data::entry(species.default_entry).is_default());
        assert!(species.entry_start < species.entry_end);

        // selectors are sorted so the runtime can binary search them
        let keys: Vec<&str> = species.selectors().iter().map(|(k, _)| *k).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted, "{} selectors are not sorted", species.slug);

        // every non-default entry is reachable through --form
        for id in species.entry_ids() {
            let entry = data::entry(id);
            assert_eq!(entry.species, idx);
            if entry.is_default() {
                assert!(entry.selector.is_empty());
            } else {
                assert!(!entry.selector.is_empty());
                assert_eq!(
                    termidex::lookup::resolve_selector(idx, entry.selector),
                    Some(id),
                    "{} --form {} should resolve",
                    species.slug,
                    entry.selector
                );
            }
        }
    }
}

/// an entry's generation is its own, not its species'
#[test]
fn generation_follows_the_entry_not_the_species() {
    use termidex::data;

    let find = |display: &str| {
        (0..data::ENTRY_COUNT as u32)
            .map(data::entry)
            .find(|e| e.display_name == display)
            .unwrap_or_else(|| panic!("{display} should exist"))
    };

    // Charizard is Gen 1, but Mega Charizard X arrived in Gen 6 and Gigantamax in Gen 8
    assert_eq!(find("Charizard").generation, 1);
    assert_eq!(find("Mega Charizard X").generation, 6);
    assert_eq!(find("Gigantamax Charizard").generation, 8);
    // Mega Raichu is Legends Z-A, so Gen 9 on a Gen 1 Pokémon
    assert_eq!(find("Raichu").generation, 1);
    assert_eq!(find("Mega Raichu X").generation, 9);

    // so --gen 1 must not serve a later Mega
    let mut names = std::collections::HashSet::new();
    for seed in 0..60 {
        names.insert(shown_name(&[
            "random",
            "--gen",
            "1",
            "--include-varieties",
            "--include-forms",
            "--seed",
            &seed.to_string(),
            "--name",
        ]));
    }
    assert!(
        names
            .iter()
            .all(|n| !n.starts_with("Mega ") && !n.starts_with("Gigantamax ")),
        "gen 1 should hold no Megas or Gmax, got {names:?}"
    );
}

/// every displayable entry should be placed in a region
#[test]
fn every_entry_has_a_region() {
    use termidex::data;
    for id in 0..data::ENTRY_COUNT as u32 {
        let entry = data::entry(id);
        assert!(
            entry.region >= 1 && entry.region as usize <= data::REGIONS.len(),
            "{} has region {}",
            entry.display_name,
            entry.region
        );
        assert!(data::GENERATIONS.contains(&entry.generation));
    }
}
