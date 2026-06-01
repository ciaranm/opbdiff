//! CLI definitions, kept in a module of its own so the binary entry
//! point stays tiny and integration tests can construct `Args` values
//! without going through argv.

use std::path::PathBuf;

use anstream::ColorChoice as AnstreamColor;
use clap::{Parser, ValueEnum};

use opbdiff::compare::{CompareMode, CompareOptions, ReferenceSide};

#[derive(Parser, Debug)]
#[command(
    name = "opbdiff",
    version,
    about = "Semantic diff for VeriPB-extended OPB pseudo-Boolean files",
    long_about = "Compares two OPB files by what they mean rather than by bytes. \
                  See dev_docs/0003-normalization.md for the canonical form."
)]
pub struct Args {
    /// First OPB file.
    pub a: PathBuf,
    /// Second OPB file.
    pub b: PathBuf,

    /// Compare constraints as a multiset rather than by position.
    #[arg(short, long)]
    pub unordered: bool,

    /// Pair constraints that share a label before diffing, regardless
    /// of position; then compare the content of each pair. Constraints
    /// with no matching label fall back to the ordered/`--unordered`
    /// matching. Useful for lining up two encoders that agree on
    /// labels but differ in order or auxiliary-variable names.
    #[arg(short = 'm', long)]
    pub match_labels: bool,

    /// Check that labels on the reference side are honoured by the
    /// candidate side. Extra labels on the candidate side that
    /// correspond to an unlabelled reference-side constraint are
    /// tolerated.
    #[arg(short = 'L', long)]
    pub check_labels: bool,

    /// Treat any variable not in the projected (`preserved:`) set as
    /// auxiliary and compare it by coefficient only, so constraints
    /// that differ purely in the *names* of their auxiliary variables
    /// are considered equal. Requires at least one file to carry a
    /// `preserved:` line; if both do, they must agree.
    #[arg(long)]
    pub ignore_aux_names: bool,

    /// Which side carries the reference labels for `--check-labels`.
    /// Default is `b`, matching the verbal "A=candidate, B=reference"
    /// description.
    #[arg(short, long, value_enum, default_value_t = ReferenceArg::B)]
    pub reference: ReferenceArg,

    /// When to emit ANSI colour. `auto` keeps colour for TTY stdout
    /// and strips it otherwise; the `NO_COLOR` environment variable
    /// also forces stripping. `always` keeps colour even when piped;
    /// `never` strips unconditionally.
    #[arg(long, value_enum, default_value_t = ColorChoiceArg::Auto, value_name = "WHEN")]
    pub color: ColorChoiceArg,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ReferenceArg {
    A,
    B,
}

impl From<ReferenceArg> for ReferenceSide {
    fn from(r: ReferenceArg) -> Self {
        match r {
            ReferenceArg::A => ReferenceSide::A,
            ReferenceArg::B => ReferenceSide::B,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ColorChoiceArg {
    Auto,
    Always,
    Never,
}

impl From<ColorChoiceArg> for AnstreamColor {
    fn from(c: ColorChoiceArg) -> Self {
        match c {
            ColorChoiceArg::Auto => AnstreamColor::Auto,
            ColorChoiceArg::Always => AnstreamColor::Always,
            ColorChoiceArg::Never => AnstreamColor::Never,
        }
    }
}

impl Args {
    pub fn compare_options(&self) -> CompareOptions {
        CompareOptions {
            mode: if self.unordered {
                CompareMode::Unordered
            } else {
                CompareMode::Ordered
            },
            match_labels: self.match_labels,
            label_check: if self.check_labels {
                Some(self.reference.into())
            } else {
                None
            },
            // Resolved later, in the binary, once both files are loaded;
            // see `main::run`.
            aux_projection: None,
        }
    }
}
