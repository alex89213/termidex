//! The clap command tree, and the pieces the argv fast path needs to bypass it.
//!
//! `Cli::for_name` and `Cli::for_random` build by hand what clap would have parsed, so the fast
//! path in [`crate::app`] can skip constructing the command tree for the two shapes that need none
//! of it. `FAST_DISPLAY_FLAGS` is the set it is allowed to hand-parse: every one is a boolean that
//! takes no value and conflicts with nothing, which is what makes the equivalence provable rather
//! than hoped for.

use clap::builder::PossibleValuesParser;
use clap::{Args, Parser, Subcommand};

use crate::data;

/// The Gen 6+ full-odds shiny rate, and the default for `--shiny-rate`.
///
/// Both clap and the argv fast path read this, so the two cannot drift apart.
pub const DEFAULT_SHINY_RATE: u32 = 4096;

/// A display flag's spelling and the field it turns on.
pub type FastFlag = (&'static str, fn(&mut DisplayArgs));

/// The display flags the argv fast path understands, paired with the field each one sets.
///
/// Every one is a plain boolean that takes no value and conflicts with nothing else in this set,
/// which is what makes hand-parsing them safe.
pub const FAST_DISPLAY_FLAGS: [FastFlag; 5] = [
    ("--name", |d| d.show_name = true),
    ("--dex", |d| d.dex = true),
    ("--entry", |d| d.entry = true),
    ("--stats", |d| d.stats = true),
    ("--female", |d| d.female = true),
];

/// One `--gen` token: either a single generation or an inclusive range like `1-7`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenSpec {
    pub start: u8,
    pub end: u8,
}

impl std::str::FromStr for GenSpec {
    type Err = String;

    /// Parses `6` or `1-7`, rejecting a backwards range.
    fn from_str(token: &str) -> Result<GenSpec, String> {
        let parse = |part: &str| {
            part.parse::<u8>()
                .map_err(|_| format!("'{part}' is not a generation number"))
        };
        match token.split_once('-') {
            Some((start, end)) => {
                let (start, end) = (parse(start)?, parse(end)?);
                if start > end {
                    return Err(format!("range '{token}' runs backwards"));
                }
                Ok(GenSpec { start, end })
            }
            None => {
                let only = parse(token)?;
                Ok(GenSpec {
                    start: only,
                    end: only,
                })
            }
        }
    }
}

/// Which Pokémon a subcommand should consider, shared by `random` and `list`.
///
/// `--gen` reads differently at the two levels, and both are the right answer for their level:
/// `random` works on entries, so it filters by the generation that introduced the entry, while
/// `list` prints species, so it filters by the generation that introduced the species.
#[derive(Args, Debug, Default, PartialEq, Eq)]
pub struct FilterArgs {
    /// Limit to these generations, e.g. --gen 1 2 5 or --gen 1-7
    #[arg(long = "gen", value_name = "N|A-B", num_args = 1..)]
    pub gens: Vec<GenSpec>,

    /// Limit to Pokémon whose games belong to this region
    #[arg(long, value_name = "REGION", value_parser = PossibleValuesParser::new(data::REGIONS))]
    pub region: Option<String>,
}

impl FilterArgs {
    /// Flattens the `--gen` tokens into the individual generations they name.
    ///
    /// # Returns
    /// every generation asked for, deduplicated and sorted, empty when `--gen` was not given
    pub fn generations(&self) -> Vec<u8> {
        let mut out: Vec<u8> = self.gens.iter().flat_map(|g| g.start..=g.end).collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// Flags that change what gets printed under the sprite. Shared by name mode and `random`.
#[derive(Args, Clone, Debug, Default, PartialEq, Eq)]
pub struct DisplayArgs {
    /// Print the display name, then ♀ if the female sprite is shown, then ★ if shiny
    #[arg(long = "name")]
    pub show_name: bool,

    /// Print genus, types, height and weight on one line
    #[arg(long)]
    pub dex: bool,

    /// Print the Pokédex entry, wrapped to the terminal
    #[arg(long)]
    pub entry: bool,

    /// Print base-stat bars
    #[arg(long)]
    pub stats: bool,

    /// Show the female sprite when one exists
    #[arg(long)]
    pub female: bool,
}

impl DisplayArgs {
    /// Reports whether any display flag was given, used to reject them alongside the list flags.
    pub fn any(&self) -> bool {
        self.show_name || self.dex || self.entry || self.stats || self.female
    }
}

/// `termidex <name>`, `termidex random` or `termidex list`.
#[derive(Parser, Debug, PartialEq, Eq)]
#[command(
    name = "termidex",
    version,
    about = "Print Pokémon sprites and Pokédex data in the terminal",
    arg_required_else_help = true,
    subcommand_negates_reqs = true
)]
pub struct Cli {
    /// Pokémon name, in any spelling: pikachu, "Mr. Mime", hooh, farfetchd
    #[arg(value_name = "NAME", required = true)]
    #[cfg_attr(feature = "unstable-dynamic", arg(add = crate::complete::species_names()))]
    pub name: Option<String>,

    /// Show an alternate variety or form, such as --form mega-x or --form alolan
    #[arg(long, value_name = "SELECTOR")]
    #[cfg_attr(feature = "unstable-dynamic", arg(add = crate::complete::form_selectors()))]
    pub form: Option<String>,

    #[command(flatten)]
    pub display: DisplayArgs,

    /// List the species' alternate forms and exit, printing nothing else
    #[arg(long, conflicts_with_all = ["form", "show_name", "dex", "entry", "stats", "female"])]
    pub list: bool,

    /// One in N renders is shiny. 1 is always, 0 is never
    #[arg(long, global = true, default_value_t = DEFAULT_SHINY_RATE, value_name = "N")]
    pub shiny_rate: u32,

    /// Seed every random draw, for reproducible output
    #[arg(long, global = true, value_name = "SEED")]
    pub seed: Option<u64>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

impl Cli {
    /// Rejects name-mode arguments before a subcommand, where they would otherwise be ignored.
    /// Global flags still work on either side of a subcommand.
    pub fn validate_mode(&self) -> Result<(), clap::Error> {
        if self.command.is_some()
            && (self.name.is_some() || self.form.is_some() || self.list || self.display.any())
        {
            use clap::CommandFactory;
            return Err(Self::command().error(
                clap::error::ErrorKind::ArgumentConflict,
                "a Pokémon name, --form, --list and display flags before a subcommand cannot be \
                 combined with a subcommand; put random's display flags after random",
            ));
        }
        Ok(())
    }

    /// Every field at the default clap would have given it.
    ///
    /// The argv fast path builds on this to skip constructing clap's command tree, which costs
    /// more than decoding and drawing a sprite does. Both paths hand the same `Cli` to the same
    /// dispatch, so the only thing the fast path can get wrong is deciding when to take itself,
    /// and `fast_path_matches_clap` checks exactly that.
    fn defaults() -> Cli {
        Cli {
            name: None,
            form: None,
            display: DisplayArgs::default(),
            list: false,
            shiny_rate: DEFAULT_SHINY_RATE,
            seed: None,
            command: None,
        }
    }

    /// Builds what clap produces for a Pokémon name plus display flags.
    ///
    /// # Arguments
    /// * `name` - the bare positional argument
    /// * `display` - the display flags that followed it
    ///
    /// # Returns
    /// the equivalent `Cli`
    pub fn for_name(name: String, display: DisplayArgs) -> Cli {
        Cli {
            name: Some(name),
            display,
            ..Cli::defaults()
        }
    }

    /// Builds what clap produces for `termidex random` plus display flags.
    ///
    /// # Arguments
    /// * `display` - the display flags that followed the subcommand
    ///
    /// # Returns
    /// the equivalent `Cli`
    pub fn for_random(display: DisplayArgs) -> Cli {
        Cli {
            command: Some(Command::Random(RandomArgs {
                display,
                ..RandomArgs::default()
            })),
            ..Cli::defaults()
        }
    }
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum Command {
    /// Show a random Pokémon
    Random(RandomArgs),
    /// Print every species, one per line, sorted by dex number
    List(ListArgs),
}

#[derive(Args, Debug, Default, PartialEq, Eq)]
pub struct RandomArgs {
    #[command(flatten)]
    pub filter: FilterArgs,

    // --region admits every shape from its region already, so asking for a shape as well is
    // contradictory. Declared here rather than on `region`, because FilterArgs is shared with
    // `list`, which has no include flags for clap to resolve the name against.
    /// Also alternates with their own stats: regional forms, Deoxys formes, cosplay Pikachu
    #[arg(long, conflicts_with = "region")]
    pub include_varieties: bool,

    /// Also alternates that only change appearance: Unown letters, Arceus plates, Burmy cloaks
    #[arg(long, conflicts_with = "region")]
    pub include_forms: bool,

    /// Also forms that only exist mid-battle: Mega, Primal, Gigantamax, Zen Mode
    #[arg(long)]
    pub battle_forms: bool,

    #[command(flatten)]
    pub display: DisplayArgs,
}

#[derive(Args, Debug, Default, PartialEq, Eq)]
pub struct ListArgs {
    #[command(flatten)]
    pub filter: FilterArgs,
}
