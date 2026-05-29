//! Plain-text reporter: maximally portable, pipeable, no styling.
//!
//! Writes to any `std::io::Write`. The output shape is intentionally
//! line-oriented and easy to grep.

use std::io;

use crate::compare::{
    CompareMode, ConstraintDiff, DiffResult, ObjectiveDiff, PreservedDiff, ReferenceSide,
};

pub fn write(out: &mut dyn io::Write, diff: &DiffResult) -> io::Result<()> {
    let mut wrote_any_diff = false;

    if write_objective(out, &diff.objective)?.is_some() {
        wrote_any_diff = true;
    }
    if write_preserved(out, &diff.preserved)?.is_some() {
        wrote_any_diff = true;
    }
    for d in &diff.constraints {
        if write_constraint(out, d, diff.mode)?.is_some() {
            wrote_any_diff = true;
        }
    }

    let summary = diff.summary();
    if wrote_any_diff {
        writeln!(out)?;
        write!(
            out,
            "Summary ({mode}): {m} matches, {d} differing, {a} only in A, {b} only in B",
            mode = mode_label(diff.mode),
            m = summary.matches,
            d = summary.differing,
            a = summary.only_in_a,
            b = summary.only_in_b,
        )?;
        if summary.label_mismatches > 0 {
            write!(out, ", {} label mismatch", summary.label_mismatches)?;
            if summary.label_mismatches != 1 {
                write!(out, "es")?;
            }
        }
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
            "Files are semantically equivalent ({} constraints compared, {mode}).",
            summary.matches,
            mode = mode_label(diff.mode),
        )?;
    }

    Ok(())
}

fn mode_label(mode: CompareMode) -> &'static str {
    match mode {
        CompareMode::Ordered => "ordered",
        CompareMode::Unordered => "unordered",
    }
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

fn write_constraint(
    out: &mut dyn io::Write,
    d: &ConstraintDiff,
    mode: CompareMode,
) -> io::Result<Option<()>> {
    match d {
        ConstraintDiff::Match { .. } => Ok(None),

        ConstraintDiff::Differ {
            index_a,
            index_b,
            a,
            b,
        } => {
            // Differ only happens in ordered mode, so index_a == index_b.
            let _ = (mode, index_b);
            writeln!(
                out,
                "Differing at constraint #{} (A line {}, B line {}):",
                index_a + 1,
                a.line,
                b.line,
            )?;
            writeln!(out, "  A: {}", a.raw.trim())?;
            writeln!(out, "  B: {}", b.raw.trim())?;
            Ok(Some(()))
        }

        ConstraintDiff::OnlyInA { index, a } => {
            match mode {
                CompareMode::Ordered => writeln!(
                    out,
                    "Only in A at constraint #{} (A line {}):",
                    index + 1,
                    a.line,
                )?,
                CompareMode::Unordered => writeln!(out, "Only in A (line {}):", a.line)?,
            }
            writeln!(out, "  A: {}", a.raw.trim())?;
            Ok(Some(()))
        }

        ConstraintDiff::OnlyInB { index, b } => {
            match mode {
                CompareMode::Ordered => writeln!(
                    out,
                    "Only in B at constraint #{} (B line {}):",
                    index + 1,
                    b.line,
                )?,
                CompareMode::Unordered => writeln!(out, "Only in B (line {}):", b.line)?,
            }
            writeln!(out, "  B: {}", b.raw.trim())?;
            Ok(Some(()))
        }

        ConstraintDiff::LabelMismatch {
            index_a,
            index_b,
            a,
            b,
            reference,
        } => {
            let (ref_label, cand_label, ref_side) = match reference {
                ReferenceSide::A => (a.label.as_deref(), b.label.as_deref(), "A"),
                ReferenceSide::B => (b.label.as_deref(), a.label.as_deref(), "B"),
            };
            writeln!(
                out,
                "Label mismatch at constraint A#{} / B#{} (reference={ref_side}):",
                index_a + 1,
                index_b + 1,
            )?;
            writeln!(out, "  expected label: {}", ref_label.unwrap_or("(none)"),)?;
            writeln!(out, "  actual label:   {}", cand_label.unwrap_or("(none)"),)?;
            writeln!(out, "  A: {}", a.raw.trim())?;
            writeln!(out, "  B: {}", b.raw.trim())?;
            Ok(Some(()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::{CompareOptions, compare, compare_ordered};
    use crate::model::normalise_file;
    use crate::parser::parse;

    fn diff(a: &str, b: &str) -> DiffResult {
        compare_ordered(
            &normalise_file(&parse(a).unwrap()).unwrap(),
            &normalise_file(&parse(b).unwrap()).unwrap(),
        )
    }

    fn diff_with(a: &str, b: &str, options: CompareOptions) -> DiffResult {
        compare(
            &normalise_file(&parse(a).unwrap()).unwrap(),
            &normalise_file(&parse(b).unwrap()).unwrap(),
            options,
        )
    }

    fn render(d: &DiffResult) -> String {
        let mut buf: Vec<u8> = Vec::new();
        write(&mut buf, d).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn equivalent_files_print_equivalent_message() {
        let out = render(&diff("1 x1 >= 1 ;\n", "+1 ~x1 <= 0 ;\n"));
        assert!(out.contains("semantically equivalent"), "output: {out}");
        assert!(out.contains("ordered"));
    }

    #[test]
    fn differing_constraint_is_printed_with_originals() {
        let out = render(&diff("1 x1 >= 1 ;\n", "1 x2 >= 1 ;\n"));
        assert!(out.contains("Differing at constraint #1"));
        assert!(out.contains("A: 1 x1 >= 1 ;"));
        assert!(out.contains("B: 1 x2 >= 1 ;"));
        assert!(out.contains("Summary (ordered)"));
    }

    #[test]
    fn only_in_a_under_ordered_uses_position() {
        let out = render(&diff("1 x1 >= 1 ;\n1 x2 >= 1 ;\n", "1 x1 >= 1 ;\n"));
        assert!(out.contains("Only in A at constraint #2"));
    }

    #[test]
    fn only_in_a_under_unordered_omits_position() {
        let out = render(&diff_with(
            "1 x1 >= 1 ;\n",
            "1 x2 >= 1 ;\n",
            CompareOptions {
                mode: CompareMode::Unordered,
                label_check: None,
            },
        ));
        assert!(out.contains("Only in A (line"));
        assert!(out.contains("Only in B (line"));
        assert!(!out.contains("at constraint #"));
        assert!(out.contains("Summary (unordered)"));
    }

    #[test]
    fn label_mismatch_is_reported_with_reference_side() {
        let out = render(&diff_with(
            "1 x1 >= 1 ;\n",
            "@card 1 x1 >= 1 ;\n",
            CompareOptions {
                mode: CompareMode::Ordered,
                label_check: Some(ReferenceSide::B),
            },
        ));
        assert!(out.contains("Label mismatch"));
        assert!(out.contains("reference=B"));
        assert!(out.contains("expected label: card"));
        assert!(out.contains("actual label:   (none)"));
        assert!(out.contains("1 label mismatch"));
    }
}
