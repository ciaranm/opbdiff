//! `opbdiff` binary entry point.
//!
//! Exits 0 if the two files are semantically equivalent, 1 if they
//! differ, and 2 on any error (I/O, parse, normalisation).

mod cli;

use std::io::Write as _;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser as _;

use opbdiff::compare::compare_ordered;
use opbdiff::model::{CanonicalFile, normalise_file};
use opbdiff::parser::parse;
use opbdiff::report::plain;

fn main() -> ExitCode {
    let args = cli::Args::parse();
    match run(&args) {
        Ok(true) => ExitCode::from(0),
        Ok(false) => ExitCode::from(1),
        Err(err) => {
            // Print the full error chain to stderr so the user sees both
            // the failure and its context (file name, parse line, etc.).
            let mut stderr = std::io::stderr();
            let _ = writeln!(stderr, "opbdiff: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &cli::Args) -> Result<bool> {
    let a = load(&args.a)?;
    let b = load(&args.b)?;
    let diff = compare_ordered(&a, &b);

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    plain::write(&mut handle, &diff).context("writing report")?;

    Ok(diff.is_equivalent())
}

fn load(path: &Path) -> Result<CanonicalFile> {
    let input =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let ast = parse(&input).with_context(|| format!("parsing {}", path.display()))?;
    let canonical =
        normalise_file(&ast).with_context(|| format!("normalising {}", path.display()))?;
    Ok(canonical)
}
