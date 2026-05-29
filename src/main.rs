//! `opbdiff` binary entry point.
//!
//! Exits 0 if the two files are semantically equivalent under the
//! chosen options, 1 if they differ, and 2 on any error (I/O, parse,
//! normalisation).

mod cli;

use std::io::Write as _;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser as _;

use opbdiff::compare::compare;
use opbdiff::model::{CanonicalFile, normalise_file};
use opbdiff::parser::parse;
use opbdiff::report::plain;

fn main() -> ExitCode {
    let args = cli::Args::parse();
    match run(&args) {
        Ok(true) => ExitCode::from(0),
        Ok(false) => ExitCode::from(1),
        Err(err) => {
            let mut stderr = std::io::stderr();
            let _ = writeln!(stderr, "opbdiff: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &cli::Args) -> Result<bool> {
    let a = load(&args.a)?;
    let b = load(&args.b)?;
    let diff = compare(&a, &b, args.compare_options());

    let stdout = std::io::stdout();
    let handle = stdout.lock();
    let mut coloured = anstream::AutoStream::new(handle, args.color.into());
    plain::write(&mut coloured, &diff).context("writing report")?;

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
