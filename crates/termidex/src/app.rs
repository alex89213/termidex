//! Wires the CLI to the data and builds the output.
//!
//! Every mode appends into one `Vec<u8>` and the caller writes it out in a single call, so a run
//! touches stdout exactly once.
//!
//! Two parsers reach the same place. [`fast_path`] hand-reads the two command shapes that need
//! none of clap's command tree and builds the `Cli` clap would have produced; everything else goes
//! to clap. Both then go through [`dispatch`], so there is one implementation of the behaviour and
//! the only thing the fast path can get wrong is deciding when to take itself.

use std::io::Write;
use std::process::ExitCode;

use clap::{CommandFactory, Parser};

use crate::cli::{self, Cli, Command, DisplayArgs, ListArgs, RandomArgs};
use crate::color::Caps;
use crate::data::{self, Entry};
use crate::lookup::{self, Resolved};
use crate::render;
use crate::sprite::Sprite;

/// Exit code for a lookup or data error, as opposed to clap's 2 for a usage error.
const EXIT_LOOKUP: u8 = 1;

/// Parses the command line, renders, and writes the result.
///
/// # Returns
/// the process exit code: 0 on success, 1 for a lookup error, 2 for a usage error
pub fn run() -> ExitCode {
    // a completion request has to be answered before anything reaches stdout
    #[cfg(feature = "unstable-dynamic")]
    crate::complete::install();

    #[cfg(windows)]
    let _ = enable_ansi_support::enable_ansi_support();

    // building clap's command tree costs more than decoding and drawing the sprite does, so the
    // one shape that needs none of it skips it
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let cli = fast_path(&args).unwrap_or_else(Cli::parse);
    if let Err(error) = cli.validate_mode() {
        error.exit();
    }
    let caps = Caps::detect();
    let mut out: Vec<u8> = Vec::with_capacity(64 << 10);

    let code = match dispatch(&cli, caps, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(EXIT_LOOKUP)
        }
    };

    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    match lock.write_all(&out).and_then(|()| lock.flush()) {
        Ok(()) => code,
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => code,
        Err(error) => {
            eprintln!("error: cannot write stdout: {error}");
            ExitCode::from(EXIT_LOOKUP)
        }
    }
}

/// Parses the two command shapes that need none of clap's machinery.
///
/// The grammar is deliberately tiny: either a Pokémon name or the bare word `random`, optionally
/// followed by any mix of the five display flags. Nothing in that set takes a value and nothing in
/// it conflicts with anything else in it, which is the whole reason hand-parsing it is safe.
/// Everything else, `--form`, `--gen`, `--region`, `--shiny-rate`, `--seed`, `--list`, `list`,
/// `--help`, a second positional, a short flag, a `--flag=value`, or a non-UTF-8 word, falls
/// through to clap untouched.
///
/// The `list` subcommand is left out on purpose: its flags all take values, and it writes a
/// thousand lines, so parsing is noise next to the output.
///
/// # Arguments
/// * `args` - the full process arguments, program name included
///
/// # Returns
/// the `Cli` clap would have produced, or `None` when clap has to do the parsing
fn fast_path(args: &[std::ffi::OsString]) -> Option<Cli> {
    let (first, rest) = args.get(1..)?.split_first()?;
    let head = first.to_str()?;

    let mut display = DisplayArgs::default();
    let mut seen = 0u8;
    for word in rest {
        let flag = word.to_str()?;
        let index = cli::FAST_DISPLAY_FLAGS
            .iter()
            .position(|(name, _)| *name == flag)?;
        // clap refuses a flag given twice, so hand the repeat back and let it say so properly
        let bit = 1u8 << index;
        if seen & bit != 0 {
            return None;
        }
        seen |= bit;
        cli::FAST_DISPLAY_FLAGS[index].1(&mut display);
    }

    match head {
        "random" => Some(Cli::for_random(display)),
        // every other subcommand belongs to clap. `list` takes only value-carrying flags, and
        // `help` is one clap generates itself, which is easy to forget: the fast path shipped
        // without it once and answered `termidex help` with a did-you-mean for Helioptile.
        // fast_path_excludes_every_subcommand keeps this list honest.
        name if SUBCOMMANDS_FOR_CLAP.contains(&name) => None,
        // a name cannot be empty or look like a flag
        name if name.is_empty() || name.starts_with('-') => None,
        name => Some(Cli::for_name(name.to_string(), display)),
    }
}

/// Subcommand names the fast path must hand to clap rather than read as a Pokémon.
///
/// `random` is absent because the fast path handles it. Everything else clap declares, including
/// the `help` subcommand clap adds on its own, has to fall through.
const SUBCOMMANDS_FOR_CLAP: [&str; 2] = ["list", "help"];

/// Routes to one of the three modes.
fn dispatch(cli: &Cli, caps: Caps, out: &mut Vec<u8>) -> Result<(), String> {
    match &cli.command {
        Some(Command::Random(args)) => random_mode(cli, args, caps, out),
        Some(Command::List(args)) => list_mode(args, out),
        None => match &cli.name {
            Some(name) => name_mode(cli, name, caps, out),
            // unreachable while arg_required_else_help is set, kept so the match stays total
            None => {
                let _ = Cli::command().print_help();
                Ok(())
            }
        },
    }
}

/// `termidex <name> [flags]`.
fn name_mode(cli: &Cli, name: &str, caps: Caps, out: &mut Vec<u8>) -> Result<(), String> {
    let species_idx = match lookup::resolve_name(name) {
        Resolved::Entry(id) => data::entry(id).species,
        Resolved::AltSlug { species, selector } => {
            let slug = data::species(species).slug;
            let command = if selector.is_empty() {
                format!("termidex {slug}")
            } else {
                format!("termidex {slug} --form {selector}")
            };
            return Err(format!(
                "'{name}' is not a Pok\u{e9}mon name. Did you mean: {command}"
            ));
        }
        Resolved::Unknown(suggestions) => {
            return Err(if suggestions.is_empty() {
                format!("'{name}' is not a Pok\u{e9}mon name")
            } else {
                format!(
                    "'{name}' is not a Pok\u{e9}mon name. Did you mean: {}",
                    suggestions.join(", ")
                )
            });
        }
    };

    if cli.list {
        list_alternates(species_idx, out);
        return Ok(());
    }

    let entry_id = match &cli.form {
        Some(selector) => lookup::resolve_selector(species_idx, selector).ok_or_else(|| {
            let species = data::species(species_idx);
            let available = lookup::available_selectors(species_idx);
            if available.is_empty() {
                format!(
                    "{} has no alternate forms, so --form {selector} is not available",
                    species.slug
                )
            } else {
                format!(
                    "'{selector}' is not a form of {}. Available: {}",
                    species.slug,
                    available.join(", ")
                )
            }
        })?,
        None => data::species(species_idx).default_entry,
    };

    let mut rng = seeded_rng(cli.seed);
    let entry = data::entry(entry_id);
    let female = cli.display.female && entry.has_female();
    if cli.display.female && !female {
        warn_no_female(entry_id);
    }

    let shiny = resolve_shiny(&mut rng, cli.shiny_rate, entry, female);
    draw(out, entry_id, shiny, female, &cli.display, caps);
    Ok(())
}

/// `termidex random [flags]`.
fn random_mode(cli: &Cli, args: &RandomArgs, caps: Caps, out: &mut Vec<u8>) -> Result<(), String> {
    let gens = validate_gens(&args.filter);
    let region = args.filter.region.as_deref().and_then(data::region_id);

    let admits = |id: u32| -> bool {
        let entry = data::entry(id);
        // Forms that only exist mid-battle are out unless asked for. A Pokédex cannot show you
        // a Mega, so they do not belong in a default pool, and they are the same shape as the
        // alternates that do (154 of the 156 are varieties), so shape alone cannot separate
        // them. That makes battle-only its own axis: the three include flags then cover the
        // non-default entries exactly once each.
        if entry.is_battle_only() && !args.battle_forms {
            return false;
        }
        let kind_ok = match region {
            // a region admits its own entries whatever shape they are, since the regional
            // forms are alternate varieties and requiring a flag for them would make
            // --region useless on its own
            Some(r) => entry.region == r,
            None => {
                entry.is_default()
                    || (args.battle_forms && entry.is_battle_only())
                    || (args.include_varieties && entry.is_alt_variety())
                    || (args.include_forms && entry.is_alt_form())
            }
        };
        // an entry's own generation, not its species', so --gen 1 does not serve Mega
        // Charizard X on the grounds that Charizard is Gen 1
        let gen_ok = gens.is_empty() || gens.contains(&entry.generation);
        kind_ok && gen_ok
    };
    let pool: Vec<u32> = (0..data::ENTRY_COUNT as u32)
        .filter(|id| admits(*id))
        .collect();

    if pool.is_empty() {
        return Err("no Pok\u{e9}mon match those filters".to_string());
    }

    // the draw order is fixed and all three are always drawn, so a seed stays stable no matter
    // which branches the picked entry happens to take
    let mut rng = seeded_rng(cli.seed);
    let entry_id = pool[rng.usize(..pool.len())];
    let gender_roll = rng.u8(0..8);
    let shiny_roll = roll_shiny(&mut rng, cli.shiny_rate);

    let species = data::entry(entry_id).species();
    let rolled_female = species.gender_rate > 0 && gender_roll < species.gender_rate as u8;
    let (entry_id, female) = match lookup::female_counterpart(entry_id) {
        // A species split by gender shows its female entry. When that entry is in the pool it
        // gets drawn on its own, so rolling for it here as well would double the female odds.
        Some(female_id) if args.display.female || (rolled_female && !admits(female_id)) => {
            (female_id, false)
        }
        Some(_) => (entry_id, false),
        None => {
            let has = data::entry(entry_id).has_female();
            (entry_id, has && (args.display.female || rolled_female))
        }
    };
    let entry = data::entry(entry_id);
    if args.display.female && !female && !lookup::is_female_counterpart(entry_id) {
        warn_no_female(entry_id);
    }

    let shiny = shiny_roll && !entry.no_shiny_for(female);
    if shiny_roll && entry.no_shiny_for(female) {
        eprintln!(
            "termidex: no shiny sprite for {}, showing the non-shiny sprite",
            entry.display_name
        );
    }

    draw(out, entry_id, shiny, female, &args.display, caps);
    Ok(())
}

/// `termidex list [--gen ...] [--region ...]`.
fn list_mode(args: &ListArgs, out: &mut Vec<u8>) -> Result<(), String> {
    let gens = validate_gens(&args.filter);
    let region = args.filter.region.as_deref().and_then(data::region_id);

    for idx in 0..data::SPECIES_COUNT as u32 {
        let species = data::species(idx);
        if !gens.is_empty() && !gens.contains(&species.generation) {
            continue;
        }
        if let Some(r) = region {
            let in_region = species.entry_ids().map(data::entry).any(|e| e.region == r);
            if !in_region {
                continue;
            }
        }
        render::list_line(out, idx, region);
    }
    Ok(())
}

/// Handles `--list`, which prints a species' alternates and nothing else.
///
/// PokeAPI splits alternates into varieties and forms, but `--form` accepts either and a user has
/// no reason to care which is which, so they are listed together in game order.
fn list_alternates(species_idx: u32, out: &mut Vec<u8>) {
    let rows = render::alternates(species_idx);
    if rows.is_empty() {
        out.extend_from_slice(b"no alternate forms\n");
        return;
    }
    for row in rows {
        out.extend_from_slice(row.as_bytes());
        out.push(b'\n');
    }
}

/// Appends the sprite and whichever text blocks were asked for, always in the same order.
///
/// The order is fixed on purpose: sprite, name, dex line, Pokédex entry, stat bars. Flag order on
/// the command line never changes it.
fn draw(
    out: &mut Vec<u8>,
    entry_id: u32,
    shiny: bool,
    female: bool,
    display: &DisplayArgs,
    caps: Caps,
) {
    let entry = data::entry(entry_id);
    Sprite::decode(data::sprite_bytes(entry_id, shiny, female)).render(out, caps.mode);

    if display.show_name {
        let female_marker = female && data::female_sprite_differs(entry_id, shiny);
        render::name_line(out, entry, shiny, female_marker);
    }
    if display.dex {
        render::dex_line(out, entry, caps);
    }
    if display.entry {
        render::flavor_text(out, entry, render::wrap_width());
    }
    if display.stats {
        render::stat_bars(out, entry, caps);
    }
}

/// Decides whether this render is shiny, and warns when the roll cannot be honored.
fn resolve_shiny(rng: &mut fastrand::Rng, rate: u32, entry: &Entry, female: bool) -> bool {
    let rolled = roll_shiny(rng, rate);
    if rolled && entry.no_shiny_for(female) {
        eprintln!(
            "termidex: no shiny sprite for {}, showing the non-shiny sprite",
            entry.display_name
        );
        return false;
    }
    rolled
}

/// Rolls the shiny check. Rate 0 never hits, rate 1 always does.
///
/// The draw happens whatever the rate is, so changing only the seed changes only the outcome.
fn roll_shiny(rng: &mut fastrand::Rng, rate: u32) -> bool {
    let roll = rng.u32(0..rate.max(1));
    rate != 0 && roll == 0
}

/// Reports the unavailable female sprite consistently in name and random modes.
fn warn_no_female(entry_id: u32) {
    eprintln!(
        "termidex: no female sprite for {} ({}), showing the default sprite",
        data::entry(entry_id).display_name,
        female_note(entry_id)
    );
}

/// Explains why a female sprite could not be shown, in the words a player would use.
fn female_note(entry_id: u32) -> String {
    // the species keeps its female as a separate entry, so point at the one command that shows it
    if let Some(female_id) = lookup::female_counterpart(entry_id) {
        return format!(
            "the female is its own form, use --form {}",
            data::entry(female_id).selector
        );
    }
    if lookup::is_female_counterpart(entry_id) {
        return "this is already the female".to_string();
    }
    match data::entry(entry_id).species().gender_rate {
        -1 => "genderless",
        0 => "male-only",
        // the default sprite already is the female one, so there is nothing to switch to
        8 => "female-only, the default sprite is the female",
        _ => "no visual gender difference",
    }
    .to_string()
}

/// Builds the RNG, from the given seed or from OS entropy.
fn seeded_rng(seed: Option<u64>) -> fastrand::Rng {
    match seed {
        Some(seed) => fastrand::Rng::with_seed(seed),
        None => fastrand::Rng::new(),
    }
}

/// Expands the `--gen` tokens and rejects any generation the data has no Pokémon for.
///
/// Exits with clap's usage code rather than returning, because a bad generation is a usage mistake
/// rather than a lookup failure.
///
/// # Arguments
/// * `filter` - the shared filter arguments, holding the raw `--gen` tokens
///
/// # Returns
/// every generation asked for, sorted, empty when `--gen` was not given
fn validate_gens(filter: &crate::cli::FilterArgs) -> Vec<u8> {
    let gens = filter.generations();
    for g in &gens {
        if !data::GENERATIONS.contains(g) {
            let known: Vec<String> = data::GENERATIONS.iter().map(|g| g.to_string()).collect();
            Cli::command()
                .error(
                    clap::error::ErrorKind::InvalidValue,
                    format!(
                        "generation {g} is not in the data, known generations are {}",
                        known.join(", ")
                    ),
                )
                .exit();
        }
    }
    gens
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn argv(words: &[&str]) -> Vec<OsString> {
        std::iter::once("termidex")
            .chain(words.iter().copied())
            .map(OsString::from)
            .collect()
    }

    /// Every command line the fast path claims to handle.
    fn accepted() -> Vec<Vec<&'static str>> {
        let flags = ["--name", "--dex", "--entry", "--stats", "--female"];
        let mut cases = Vec::new();
        for head in ["pikachu", "random", "Mr. Mime", "ho-oh", "25", "flabébé"] {
            // the head on its own, then every subset of the display flags after it
            for mask in 0u32..(1 << flags.len()) {
                let mut words = vec![head];
                for (i, flag) in flags.iter().enumerate() {
                    if mask & (1 << i) != 0 {
                        words.push(flag);
                    }
                }
                cases.push(words);
            }
        }
        cases
    }

    /// whatever the fast path accepts, it must build exactly what clap would have built
    ///
    /// This is the test that makes the fast path safe. Both paths hand a `Cli` to the same
    /// dispatch, so equal `Cli` values mean identical behavior.
    #[test]
    fn fast_path_matches_clap() {
        for words in accepted() {
            let args = argv(&words);
            let fast =
                fast_path(&args).unwrap_or_else(|| panic!("{words:?} should take the fast path"));
            let slow = Cli::try_parse_from(&args)
                .unwrap_or_else(|e| panic!("clap rejected {words:?}: {e}"));
            assert_eq!(fast, slow, "{words:?} parsed differently");
        }
    }

    /// anything clap needs to reason about must fall through to clap
    #[test]
    fn everything_else_falls_through() {
        let rejected: [&[&str]; 24] = [
            &[],                              // no arguments, clap shows help
            &["pikachu", "--form", "mega-x"], // a flag with a value
            &["pikachu", "--form=mega-x"],    // the inline spelling of one
            &["--name", "pikachu"],           // flag before the name
            &["pikachu", "--shiny-rate", "1"],
            &["pikachu", "--seed", "7"],
            &["pikachu", "--list"],
            &["--help"],
            &["-h"],
            &["--version"],
            &["-V"],
            &["list"], // left to clap on purpose
            &["help"], // clap generates this subcommand itself
            &["help", "random"],
            &["list", "--gen", "1"],
            &["random", "--gen", "1"],
            &["random", "--region", "alola"],
            &["random", "--include-varieties"],
            &["random", "pikachu"],    // a positional random does not take
            &["pikachu", "charizard"], // a second positional
            &[""],                     // empty name
            &["-pikachu"],             // looks like a flag
            &["pikachu", "--dex", "--dex"], // clap refuses a repeated flag
            &["random", "--stats", "--stats"],
        ];
        for words in rejected {
            assert!(
                fast_path(&argv(words)).is_none(),
                "{words:?} must go through clap"
            );
        }
    }

    /// every subcommand clap knows about must fall through, including the ones clap invents
    ///
    /// This is the test the hardcoded list needs. `help` is generated by clap rather than declared
    /// in the derive, so reading the source is not enough to know what the set is. The fast path
    /// shipped without it once and answered `termidex help` with a did-you-mean for Helioptile.
    #[test]
    fn fast_path_excludes_every_subcommand() {
        use clap::CommandFactory;

        // clap injects the help subcommand during build(), not when the Command is constructed,
        // which is the reason the derive source does not reveal it
        let mut cmd = Cli::command();
        cmd.build();
        let names: Vec<String> = cmd
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .collect();
        assert!(
            names.iter().any(|n| n == "help"),
            "clap should still generate a help subcommand, got {names:?}"
        );
        for name in &names {
            let taken = fast_path(&argv(&[name])).is_some();
            if name == "random" {
                assert!(taken, "the fast path should handle `random`");
            } else {
                assert!(!taken, "`{name}` is a subcommand and must go to clap");
            }
        }
    }

    /// a name that is not valid UTF-8 should be clap's problem, not the fast path's
    #[test]
    fn non_utf8_falls_through() {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let bad = OsString::from_vec(vec![0xff, 0xfe]);
            assert!(fast_path(&[OsString::from("termidex"), bad.clone()]).is_none());
            // also as a trailing word, where it would be read as a flag
            assert!(
                fast_path(&[OsString::from("termidex"), OsString::from("pikachu"), bad]).is_none()
            );
        }
    }

    /// the flag table must stay in step with the struct it fills
    #[test]
    fn fast_flag_table_covers_every_display_flag() {
        let mut all = DisplayArgs::default();
        for (_, set) in crate::cli::FAST_DISPLAY_FLAGS {
            set(&mut all);
        }
        // if a sixth display flag is ever added, this stops compiling or starts failing
        let DisplayArgs {
            show_name,
            dex,
            entry,
            stats,
            female,
        } = all;
        assert!(show_name && dex && entry && stats && female);
    }

    /// the shiny-rate default has to be the same constant on both paths
    #[test]
    fn defaults_agree_on_shiny_rate() {
        let parsed = Cli::try_parse_from(["termidex", "pikachu"]).unwrap();
        assert_eq!(parsed.shiny_rate, crate::cli::DEFAULT_SHINY_RATE);
        assert_eq!(
            Cli::for_name("pikachu".to_string(), DisplayArgs::default()).shiny_rate,
            parsed.shiny_rate
        );
    }
}
