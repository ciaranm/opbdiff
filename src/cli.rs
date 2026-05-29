//! CLI definitions, kept in a module of its own so the binary entry
//! point stays tiny and integration tests can construct `Args` values
//! without going through argv.

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "opbdiff",
    version,
    about = "Semantic diff for VeriPB-extended OPB pseudo-Boolean files",
    long_about = "Compares two OPB files by what they mean rather than by bytes. \
                  See dev_docs/0003-normalization.md for the canonical form."
)]
pub struct Args {
    /// First OPB file (candidate, in --check-labels mode once that lands).
    pub a: PathBuf,
    /// Second OPB file (reference, in --check-labels mode once that lands).
    pub b: PathBuf,
}
