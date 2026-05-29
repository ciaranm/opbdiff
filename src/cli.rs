//! CLI definitions, kept in a module of its own so the binary entry
//! point stays tiny and integration tests can construct `Args` values
//! without going through argv.

use std::path::PathBuf;

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

    /// Check that labels on the reference side are honoured by the
    /// candidate side. Extra labels on the candidate side that
    /// correspond to an unlabelled reference-side constraint are
    /// tolerated.
    #[arg(short = 'L', long)]
    pub check_labels: bool,

    /// Which side carries the reference labels for `--check-labels`.
    /// Default is `b`, matching the verbal "A=candidate, B=reference"
    /// description.
    #[arg(short, long, value_enum, default_value_t = ReferenceArg::B)]
    pub reference: ReferenceArg,
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

impl Args {
    pub fn compare_options(&self) -> CompareOptions {
        CompareOptions {
            mode: if self.unordered {
                CompareMode::Unordered
            } else {
                CompareMode::Ordered
            },
            label_check: if self.check_labels {
                Some(self.reference.into())
            } else {
                None
            },
        }
    }
}
