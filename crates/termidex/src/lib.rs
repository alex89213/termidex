//! Fast, accurate Pokémon sprite and Pokédex viewer for the terminal.
//!
//! Everything the binary needs is baked in at build time by `termidex-gen`, so a run does no
//! network, no filesystem and no JSON work: a lookup is an array index and the sprite is a slice
//! of a static blob. The modules split up as:
//!
//! * [`data`] - the generated static tables and the accessors over them
//! * [`lookup`] - turning typed names and `--form` selectors into entry ids
//! * [`normalize`] - the name folding both this crate and the generator use
//! * [`sprite`] - the baked sprite format and half-block rendering
//! * [`color`] - terminal colour capability and the ANSI escapes for it
//! * [`render`] - the name, dex, Pokédex and stat-bar text blocks
//! * [`cli`] - the clap command tree, plus the constructors the argv fast path builds
//! * [`app`] - the fast path, the three modes, and the single write to stdout
//! * `complete` - shell completion, behind the `unstable-dynamic` feature

pub mod app;
pub mod cli;
pub mod color;
#[cfg(feature = "unstable-dynamic")]
pub mod complete;
pub mod data;
pub mod lookup;
pub mod normalize;
pub mod render;
pub mod sprite;
