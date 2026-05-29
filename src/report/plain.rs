//! Plain-text reporter: maximally portable, pipeable, no styling.
//!
//! Writes to any `std::io::Write`. The output shape is intentionally
//! line-oriented and easy to grep.

use std::io;

use crate::compare::{ConstraintDiff, DiffResult, ObjectiveDiff, PreservedDiff};

pub fn write(out: &mut dyn io::Write, diff: &DiffResult) -> io::Result<()> {
    let mut wrote_any_diff = false;

    if let Some(()) = write_objective(out, &diff.objective)? {
        wrote_any_diff = true;
    }
    if let Some(()) = write_preserved(out, &diff.preserved)? {
        wrote_any_diff = true;
    }
    for d in &diff.constraints {
        if write_constraint(out, d)?.is_some() {
            wrote_any_diff = true;
        }
    }

    let summary = diff.summary();
    if wrote_any_diff {
        writeln!(out)?;
        write!(
            out,
            "Summary: {} matches, {} differing, {} extra in A, {} extra in B",
            summary.matches, summary.differing, summary.extra_in_a, summary.extra_in_b,
        )?;
        if summary.objective_difference {
            write!(out, ", objective differs")?;
        }
        if summary.preserved_difference {
            write!(out, ", preserved differs")?;
        }
        writeln!(out, ".")?;
    } else {
        writeln!(
            out,
            "Files are semantically equivalent ({} constraints compared).",
            summary.matches,
        )?;
    }

    Ok(())
}

fn write_objective(out: &mut dyn io::Write, o: &ObjectiveDiff) -> io::Result<Option<()>> {
    match o {
        ObjectiveDiff::BothAbsent | ObjectiveDiff::Match => Ok(None),
        ObjectiveDiff::Differ { a, b } => {
            writeln!(out, "Objectives differ:")?;
            writeln!(out, "  A: {}", a.raw.trim())?;
            writeln!(out, "  B: {}", b.raw.trim())?;
            Ok(Some(()))
        }
        ObjectiveDiff::OnlyInA(a) => {
            writeln!(out, "Objective only in A:")?;
            writeln!(out, "  A: {}", a.raw.trim())?;
            Ok(Some(()))
        }
        ObjectiveDiff::OnlyInB(b) => {
            writeln!(out, "Objective only in B:")?;
            writeln!(out, "  B: {}", b.raw.trim())?;
            Ok(Some(()))
        }
    }
}

fn write_preserved(out: &mut dyn io::Write, p: &PreservedDiff) -> io::Result<Option<()>> {
    match p {
        PreservedDiff::BothAbsent | PreservedDiff::Match => Ok(None),
        PreservedDiff::Differ { a, b } => {
            writeln!(out, "Preserved lines differ:")?;
            writeln!(out, "  A: {}", a.raw.trim())?;
            writeln!(out, "  B: {}", b.raw.trim())?;
            Ok(Some(()))
        }
        PreservedDiff::OnlyInA(a) => {
            writeln!(out, "Preserved only in A:")?;
            writeln!(out, "  A: {}", a.raw.trim())?;
            Ok(Some(()))
        }
        PreservedDiff::OnlyInB(b) => {
            writeln!(out, "Preserved only in B:")?;
            writeln!(out, "  B: {}", b.raw.trim())?;
            Ok(Some(()))
        }
    }
}

fn write_constraint(out: &mut dyn io::Write, d: &ConstraintDiff) -> io::Result<Option<()>> {
    match d {
        ConstraintDiff::Match { .. } => Ok(None),
        ConstraintDiff::Differ { index, a, b } => {
            writeln!(
                out,
                "Differing at constraint #{} (A line {}, B line {}):",
                index + 1,
                a.line,
                b.line,
            )?;
            writeln!(out, "  A: {}", a.raw.trim())?;
            writeln!(out, "  B: {}", b.raw.trim())?;
            Ok(Some(()))
        }
        ConstraintDiff::ExtraInA { index, a } => {
            writeln!(
                out,
                "Extra in A at constraint #{} (A line {}):",
                index + 1,
                a.line,
            )?;
            writeln!(out, "  A: {}", a.raw.trim())?;
            Ok(Some(()))
        }
        ConstraintDiff::ExtraInB { index, b } => {
            writeln!(
                out,
                "Extra in B at constraint #{} (B line {}):",
                index + 1,
                b.line,
            )?;
            writeln!(out, "  B: {}", b.raw.trim())?;
            Ok(Some(()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::compare_ordered;
    use crate::model::normalise_file;
    use crate::parser::parse;

    fn diff(a: &str, b: &str) -> DiffResult {
        compare_ordered(
            &normalise_file(&parse(a).unwrap()).unwrap(),
            &normalise_file(&parse(b).unwrap()).unwrap(),
        )
    }

    fn report(a: &str, b: &str) -> String {
        let mut buf: Vec<u8> = Vec::new();
        write(&mut buf, &diff(a, b)).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn equivalent_files_print_equivalent_message() {
        let out = report("1 x1 >= 1 ;\n", "+1 ~x1 <= 0 ;\n");
        assert!(out.contains("semantically equivalent"), "output was: {out}",);
        assert!(out.contains("1 constraints compared"));
    }

    #[test]
    fn differing_constraint_is_printed_with_originals() {
        let out = report("1 x1 >= 1 ;\n", "1 x2 >= 1 ;\n");
        assert!(out.contains("Differing at constraint #1"));
        assert!(out.contains("A: 1 x1 >= 1 ;"));
        assert!(out.contains("B: 1 x2 >= 1 ;"));
        assert!(out.contains("Summary:"));
    }

    #[test]
    fn extra_constraint_shows_in_summary() {
        let out = report("1 x1 >= 1 ;\n1 x2 >= 1 ;\n", "1 x1 >= 1 ;\n");
        assert!(out.contains("Extra in A at constraint #2"));
        assert!(out.contains("1 extra in A"));
    }

    #[test]
    fn objective_difference_is_called_out_in_summary() {
        let out = report("min: 1 x1 ;\n1 x1 >= 1 ;\n", "min: 1 x2 ;\n1 x1 >= 1 ;\n");
        assert!(out.contains("Objectives differ"));
        assert!(out.contains("objective differs"));
    }

    #[test]
    fn preserved_difference_is_called_out_in_summary() {
        let out = report(
            "preserved: x1 x2 ;\n1 x1 >= 1 ;\n",
            "preserved: x1 x3 ;\n1 x1 >= 1 ;\n",
        );
        assert!(out.contains("Preserved lines differ"));
        assert!(out.contains("preserved differs"));
    }
}
