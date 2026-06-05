//! Plain-text reporter, optionally ANSI-coloured.
//!
//! The reporter unconditionally emits ANSI SGR escape sequences. Whether
//! the user actually sees colour is decided by the stream the binary
//! writes through: `anstream::AutoStream` strips the codes when the
//! destination is not a TTY (or when the user passed `--color=never`),
//! and keeps them when it is. Tests that compare against substrings
//! still work, because the substrings are present *between* the SGR
//! pairs.

use std::collections::HashSet;
use std::io;

use anstyle::{AnsiColor, Color, Style};

use crate::compare::{
    CompareMode, ConstraintDiff, DiffResult, LabelPermutation, ObjectiveDiff, PreservedDiff,
    ReferenceSide,
};
use crate::model::CanonicalConstraint;

// ---- styles -----------------------------------------------------------

const HEADING: Style = Style::new().bold();
const A_LINE: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red)));
const B_LINE: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green)));
const A_HEAD: Style = A_LINE.bold();
const B_HEAD: Style = B_LINE.bold();
const LABEL_HEAD: Style = Style::new()
    .fg_color(Some(Color::Ansi(AnsiColor::Yellow)))
    .bold();
const SUCCESS: Style = Style::new()
    .fg_color(Some(Color::Ansi(AnsiColor::Green)))
    .bold();

// ---- entry point ------------------------------------------------------

pub fn write(out: &mut dyn io::Write, diff: &DiffResult) -> io::Result<()> {
    let mut wrote_any_diff = false;

    if write_objective(out, &diff.objective)?.is_some() {
        wrote_any_diff = true;
    }
    // A one-sided missing `preserved:` line that was ignored via
    // `--ignore-no-preserved-in` is not a difference: stay silent here
    // (the relaxation is surfaced in the mode descriptor instead).
    if diff.ignored_missing_preserved.is_none() && write_preserved(out, &diff.preserved)?.is_some()
    {
        wrote_any_diff = true;
    }
    for d in &diff.constraints {
        if write_constraint(
            out,
            d,
            diff.mode,
            diff.aux_projection.as_ref(),
            diff.label_permutation.as_ref(),
        )?
        .is_some()
        {
            wrote_any_diff = true;
        }
    }

    let summary = diff.summary();
    if wrote_any_diff {
        writeln!(out)?;
        write!(
            out,
            "{HEADING}Summary ({mode}): {m} matches, {d} differing, {a} only in A, {b} only in B",
            mode = mode_descriptor(diff),
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
        if let Some(perm) = &diff.label_permutation {
            write!(out, "{}", describe_permutation(perm, summary.differing))?;
        }
        writeln!(out, ".{HEADING:#}")?;
    } else {
        writeln!(
            out,
            "{SUCCESS}Files are semantically equivalent ({} constraints compared, {mode}).{SUCCESS:#}",
            summary.matches,
            mode = mode_descriptor(diff),
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

/// Human description of how constraints were paired, e.g. `ordered` or
/// `label-matched, unordered fallback`.
fn mode_descriptor(diff: &DiffResult) -> String {
    let mut s = if diff.matched_by_label {
        format!("label-matched, {} fallback", mode_label(diff.mode))
    } else {
        mode_label(diff.mode).to_string()
    };
    if diff.aux_projection.is_some() {
        s.push_str(", aux names ignored");
    }
    match diff.ignored_missing_preserved {
        Some(ReferenceSide::A) => s.push_str(", missing preserved in A ignored"),
        Some(ReferenceSide::B) => s.push_str(", missing preserved in B ignored"),
        None => {}
    }
    s
}

/// Summary clause describing how many of the `differing` constraints are
/// explained by a label permutation, e.g. `, all differing explained by
/// a label permutation (3 swaps)` or `, 1 of 4 differing explained by a
/// label permutation (1 longer cycle)`. Empty when nothing lined up.
fn describe_permutation(perm: &LabelPermutation, differing: usize) -> String {
    let explained = perm.correspondences.len();
    if explained == 0 {
        return String::new();
    }
    let swaps = perm.swaps();
    let longer = perm.cycles.iter().filter(|c| c.len() > 2).count();
    let mut parts = Vec::new();
    if swaps > 0 {
        parts.push(format!("{swaps} swap{}", plural(swaps)));
    }
    if longer > 0 {
        parts.push(format!("{longer} longer cycle{}", plural(longer)));
    }
    let detail = if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    };
    // "all differing" only when every differing constraint — including
    // any non-label fallback ones — is accounted for by the permutation.
    if perm.all_explained() && explained == differing {
        format!(", all differing explained by a label permutation{detail}")
    } else {
        format!(", {explained} of {differing} differing explained by a label permutation{detail}")
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

// ---- per-section writers ---------------------------------------------

fn write_objective(out: &mut dyn io::Write, o: &ObjectiveDiff) -> io::Result<Option<()>> {
    match o {
        ObjectiveDiff::BothAbsent | ObjectiveDiff::Match => Ok(None),
        ObjectiveDiff::Differ { a, b } => {
            writeln!(out, "{HEADING}Objectives differ:{HEADING:#}")?;
            writeln!(out, "{A_LINE}  A: {}{A_LINE:#}", a.raw.trim())?;
            writeln!(out, "{B_LINE}  B: {}{B_LINE:#}", b.raw.trim())?;
            Ok(Some(()))
        }
        ObjectiveDiff::OnlyInA(a) => {
            writeln!(out, "{A_HEAD}Objective only in A:{A_HEAD:#}")?;
            writeln!(out, "{A_LINE}  A: {}{A_LINE:#}", a.raw.trim())?;
            Ok(Some(()))
        }
        ObjectiveDiff::OnlyInB(b) => {
            writeln!(out, "{B_HEAD}Objective only in B:{B_HEAD:#}")?;
            writeln!(out, "{B_LINE}  B: {}{B_LINE:#}", b.raw.trim())?;
            Ok(Some(()))
        }
    }
}

fn write_preserved(out: &mut dyn io::Write, p: &PreservedDiff) -> io::Result<Option<()>> {
    match p {
        PreservedDiff::BothAbsent | PreservedDiff::Match => Ok(None),
        PreservedDiff::Differ { a, b } => {
            writeln!(out, "{HEADING}Preserved lines differ:{HEADING:#}")?;
            writeln!(out, "{A_LINE}  A: {}{A_LINE:#}", a.raw.trim())?;
            writeln!(out, "{B_LINE}  B: {}{B_LINE:#}", b.raw.trim())?;
            Ok(Some(()))
        }
        PreservedDiff::OnlyInA(a) => {
            writeln!(out, "{A_HEAD}Preserved only in A:{A_HEAD:#}")?;
            writeln!(out, "{A_LINE}  A: {}{A_LINE:#}", a.raw.trim())?;
            Ok(Some(()))
        }
        PreservedDiff::OnlyInB(b) => {
            writeln!(out, "{B_HEAD}Preserved only in B:{B_HEAD:#}")?;
            writeln!(out, "{B_LINE}  B: {}{B_LINE:#}", b.raw.trim())?;
            Ok(Some(()))
        }
    }
}

fn write_constraint(
    out: &mut dyn io::Write,
    d: &ConstraintDiff,
    mode: CompareMode,
    projection: Option<&HashSet<String>>,
    permutation: Option<&LabelPermutation>,
) -> io::Result<Option<()>> {
    match d {
        ConstraintDiff::Match { .. } => Ok(None),

        ConstraintDiff::Differ {
            index_a,
            index_b,
            a,
            b,
        } => {
            let _ = (mode, index_b);
            writeln!(
                out,
                "{HEADING}Differing at constraint #{}{label} (A line {}, B line {}):{HEADING:#}",
                index_a + 1,
                a.line,
                b.line,
                label = shared_label_tag(a.label.as_deref(), b.label.as_deref()),
            )?;
            writeln!(out, "{A_LINE}  A: {}{A_LINE:#}", a.raw.trim())?;
            writeln!(out, "{B_LINE}  B: {}{B_LINE:#}", b.raw.trim())?;
            // If this difference is just a permuted label, the canonical
            // forms are wholly unequal but reappear under another label;
            // the one-line correspondence is far more useful than a
            // full term-by-term dump, so it replaces the canonical view.
            match permutation.and_then(|p| p.correspondence_for(a.label.as_deref())) {
                Some(corr) => {
                    let kind = if corr.swap {
                        "label swap"
                    } else {
                        "label permutation"
                    };
                    writeln!(
                        out,
                        "  {HEADING}A's @{} canonically matches B's @{} ({kind}){HEADING:#}",
                        corr.from, corr.to,
                    )?;
                }
                None => write_canonical_diff(out, &a.form, &b.form, projection)?,
            }
            Ok(Some(()))
        }

        ConstraintDiff::OnlyInA { index, a } => {
            match mode {
                CompareMode::Ordered => writeln!(
                    out,
                    "{A_HEAD}Only in A at constraint #{} (A line {}):{A_HEAD:#}",
                    index + 1,
                    a.line,
                )?,
                CompareMode::Unordered => {
                    writeln!(out, "{A_HEAD}Only in A (line {}):{A_HEAD:#}", a.line)?
                }
            }
            writeln!(out, "{A_LINE}  A: {}{A_LINE:#}", a.raw.trim())?;
            Ok(Some(()))
        }

        ConstraintDiff::OnlyInB { index, b } => {
            match mode {
                CompareMode::Ordered => writeln!(
                    out,
                    "{B_HEAD}Only in B at constraint #{} (B line {}):{B_HEAD:#}",
                    index + 1,
                    b.line,
                )?,
                CompareMode::Unordered => {
                    writeln!(out, "{B_HEAD}Only in B (line {}):{B_HEAD:#}", b.line)?
                }
            }
            writeln!(out, "{B_LINE}  B: {}{B_LINE:#}", b.raw.trim())?;
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
                "{LABEL_HEAD}Label mismatch at constraint A#{} / B#{} (reference={ref_side}):{LABEL_HEAD:#}",
                index_a + 1,
                index_b + 1,
            )?;
            writeln!(out, "  expected label: {}", ref_label.unwrap_or("(none)"))?;
            writeln!(out, "  actual label:   {}", cand_label.unwrap_or("(none)"))?;
            writeln!(out, "{A_LINE}  A: {}{A_LINE:#}", a.raw.trim())?;
            writeln!(out, "{B_LINE}  B: {}{B_LINE:#}", b.raw.trim())?;
            Ok(Some(()))
        }
    }
}

/// A ` [@label]` tag for the `Differ` header when both sides carry the
/// same label (the usual case under label-matched mode). Empty
/// otherwise, to avoid noise — differing or one-sided labels are the
/// concern of `--check-labels`, not the content diff.
fn shared_label_tag(a: Option<&str>, b: Option<&str>) -> String {
    match (a, b) {
        (Some(la), Some(lb)) if la == lb => format!(" [@{la}]"),
        _ => String::new(),
    }
}

/// Render a sorted, per-variable comparison of two canonical
/// constraints. Each row shows the variable name and either side's
/// coefficient (or `(absent)` if one side doesn't reference that
/// variable). The RHS is shown as a synthetic last row. Rows where
/// both sides agree are summarised as a count rather than printed
/// in full, so a 100-term constraint with one differing coefficient
/// shows one row.
///
/// When `projection` is `Some`, auxiliary terms (variables not in the
/// projected set) are not compared by name: they are collapsed into a
/// single folded row showing each side's sorted multiset of auxiliary
/// coefficients, and shown only if those multisets differ.
fn write_canonical_diff(
    out: &mut dyn io::Write,
    a: &CanonicalConstraint,
    b: &CanonicalConstraint,
    projection: Option<&HashSet<String>>,
) -> io::Result<()> {
    let is_real = |v: &str| projection.is_none_or(|p| p.contains(v));

    let real_a: Vec<(String, i64)> = a
        .terms
        .iter()
        .filter(|(v, _)| is_real(v))
        .cloned()
        .collect();
    let real_b: Vec<(String, i64)> = b
        .terms
        .iter()
        .filter(|(v, _)| is_real(v))
        .cloned()
        .collect();

    let rows = collect_canonical_rows(&real_a, a.rhs, &real_b, b.rhs);
    let differing: Vec<&Row> = rows.iter().filter(|r| r.differs()).collect();
    let identical_count = rows.len() - differing.len();

    let aux_a = aux_coefficients(a, projection);
    let aux_b = aux_coefficients(b, projection);
    let aux_differs = aux_a != aux_b;

    if differing.is_empty() && !aux_differs {
        return Ok(());
    }

    let heading = if projection.is_some() {
        "canonical-form view (sorted, aux names ignored):"
    } else {
        "canonical-form view (sorted):"
    };
    writeln!(out, "  {HEADING}{heading}{HEADING:#}")?;

    let var_width = differing
        .iter()
        .map(|r| r.var.chars().count())
        .max()
        .unwrap_or(0);

    let a_width = differing
        .iter()
        .map(|r| coef_display_len(r.a))
        .max()
        .unwrap_or(0);

    for row in &differing {
        let a_text = format_coef(row.a);
        let b_text = format_coef(row.b);
        let a_pad = a_width.saturating_sub(coef_display_len(row.a));
        writeln!(
            out,
            "    {var:<var_width$}   {A_LINE}A={a_text}{A_LINE:#}{pad}   {B_LINE}B={b_text}{B_LINE:#}",
            var = row.var,
            pad = " ".repeat(a_pad),
        )?;
    }

    if aux_differs {
        writeln!(
            out,
            "    aux (names ignored): {A_LINE}A={a}{A_LINE:#}   {B_LINE}B={b}{B_LINE:#}",
            a = format_aux_list(&aux_a),
            b = format_aux_list(&aux_b),
        )?;
    }

    if identical_count > 0 {
        let plural = if identical_count == 1 { "" } else { "s" };
        writeln!(out, "    ({identical_count} identical row{plural} hidden)",)?;
    }

    Ok(())
}

/// Sorted multiset of the coefficients of `c`'s auxiliary terms (those
/// whose variable is not in `projection`). Empty when not folding.
fn aux_coefficients(c: &CanonicalConstraint, projection: Option<&HashSet<String>>) -> Vec<i64> {
    let Some(p) = projection else {
        return Vec::new();
    };
    let mut coefs: Vec<i64> = c
        .terms
        .iter()
        .filter(|(v, _)| !p.contains(v))
        .map(|(_, coef)| *coef)
        .collect();
    coefs.sort_unstable();
    coefs
}

fn format_aux_list(coefs: &[i64]) -> String {
    if coefs.is_empty() {
        return "(none)".to_string();
    }
    let parts: Vec<String> = coefs.iter().map(|c| format!("{c:+}")).collect();
    format!("[{}]", parts.join(", "))
}

struct Row {
    var: String,
    a: Option<i64>,
    b: Option<i64>,
}

impl Row {
    fn differs(&self) -> bool {
        self.a != self.b
    }
}

fn collect_canonical_rows(
    a_terms: &[(String, i64)],
    a_rhs: i64,
    b_terms: &[(String, i64)],
    b_rhs: i64,
) -> Vec<Row> {
    let mut rows = Vec::with_capacity(a_terms.len() + b_terms.len() + 1);
    let mut i = 0;
    let mut j = 0;
    while i < a_terms.len() && j < b_terms.len() {
        let (va, ca) = &a_terms[i];
        let (vb, cb) = &b_terms[j];
        match va.as_str().cmp(vb.as_str()) {
            std::cmp::Ordering::Less => {
                rows.push(Row {
                    var: va.clone(),
                    a: Some(*ca),
                    b: None,
                });
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                rows.push(Row {
                    var: vb.clone(),
                    a: None,
                    b: Some(*cb),
                });
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                rows.push(Row {
                    var: va.clone(),
                    a: Some(*ca),
                    b: Some(*cb),
                });
                i += 1;
                j += 1;
            }
        }
    }
    while i < a_terms.len() {
        let (va, ca) = &a_terms[i];
        rows.push(Row {
            var: va.clone(),
            a: Some(*ca),
            b: None,
        });
        i += 1;
    }
    while j < b_terms.len() {
        let (vb, cb) = &b_terms[j];
        rows.push(Row {
            var: vb.clone(),
            a: None,
            b: Some(*cb),
        });
        j += 1;
    }
    rows.push(Row {
        var: "rhs".to_string(),
        a: Some(a_rhs),
        b: Some(b_rhs),
    });
    rows
}

fn format_coef(c: Option<i64>) -> String {
    match c {
        Some(n) => format!("{n:+}"),
        None => "(absent)".to_string(),
    }
}

fn coef_display_len(c: Option<i64>) -> usize {
    format_coef(c).chars().count()
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

    fn strip_ansi(s: &str) -> String {
        // Crude SGR stripper that's good enough for tests: drop every
        // ESC ... 'm' sequence.
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // skip until we see 'm'
                for c2 in chars.by_ref() {
                    if c2 == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn equivalent_files_print_equivalent_message() {
        let out = render(&diff("1 x1 >= 1 ;\n", "+1 ~x1 <= 0 ;\n"));
        assert!(out.contains("semantically equivalent"), "output: {out}");
        assert!(out.contains("ordered"));
    }

    #[test]
    fn output_contains_ansi_when_writing_to_buffer() {
        // The reporter unconditionally emits SGR codes; the stripping
        // is done one layer up by AutoStream. So a raw Vec<u8> sink
        // should see escapes.
        let out = render(&diff("1 x1 >= 1 ;\n", "1 x2 >= 1 ;\n"));
        assert!(out.contains("\x1b["), "expected ANSI escapes, got: {out}");
    }

    #[test]
    fn differing_constraint_text_survives_ansi_strip() {
        let out = render(&diff("1 x1 >= 1 ;\n", "1 x2 >= 1 ;\n"));
        let plain = strip_ansi(&out);
        assert!(plain.contains("Differing at constraint #1"));
        assert!(plain.contains("A: 1 x1 >= 1 ;"));
        assert!(plain.contains("B: 1 x2 >= 1 ;"));
        assert!(plain.contains("Summary (ordered)"));
    }

    #[test]
    fn only_in_a_under_ordered_uses_position() {
        let plain = strip_ansi(&render(&diff(
            "1 x1 >= 1 ;\n1 x2 >= 1 ;\n",
            "1 x1 >= 1 ;\n",
        )));
        assert!(plain.contains("Only in A at constraint #2"));
    }

    #[test]
    fn only_in_a_under_unordered_omits_position() {
        let plain = strip_ansi(&render(&diff_with(
            "1 x1 >= 1 ;\n",
            "1 x2 >= 1 ;\n",
            CompareOptions {
                mode: CompareMode::Unordered,
                match_labels: false,
                label_check: None,
                aux_projection: None,
                ignore_missing_preserved: None,
            },
        )));
        assert!(plain.contains("Only in A (line"));
        assert!(plain.contains("Only in B (line"));
        assert!(!plain.contains("at constraint #"));
        assert!(plain.contains("Summary (unordered)"));
    }

    #[test]
    fn differ_includes_canonical_form_view() {
        let plain = strip_ansi(&render(&diff(
            "1 x1 1 x2 1 x3 >= 1 ;\n",
            "1 x1 2 x2 1 x3 >= 1 ;\n",
        )));
        // Should include the canonical-form block listing the
        // differing term, the rhs only if it differs, and a count of
        // identical rows.
        assert!(plain.contains("canonical-form view"), "got: {plain}");
        assert!(plain.contains("x2"));
        assert!(plain.contains("A=+1"));
        assert!(plain.contains("B=+2"));
        // x1, x3, and rhs all agree → 3 identical rows hidden.
        assert!(plain.contains("3 identical rows hidden"));
        // rhs row should not appear when it agrees.
        assert!(!plain.contains("rhs"));
    }

    #[test]
    fn differ_canonical_view_shows_term_only_on_one_side() {
        // A has x3, B doesn't. B has x4, A doesn't.
        let plain = strip_ansi(&render(&diff("1 x1 1 x3 >= 1 ;\n", "1 x1 1 x4 >= 1 ;\n")));
        assert!(plain.contains("x3"));
        assert!(plain.contains("x4"));
        // (absent) marker for the missing side
        assert!(plain.contains("(absent)"));
    }

    #[test]
    fn differ_canonical_view_includes_rhs_when_rhs_differs() {
        let plain = strip_ansi(&render(&diff("1 x1 >= 1 ;\n", "1 x1 >= 2 ;\n")));
        assert!(plain.contains("rhs"));
        assert!(plain.contains("A=+1"));
        assert!(plain.contains("B=+2"));
    }

    #[test]
    fn aux_folding_view_folds_aux_and_notes_in_descriptor() {
        // x is projected; the aux vars (f vs g) differ in name, and the
        // rhs differs too, so this stays a Differ. The folded view
        // should show the projected rhs row and a single folded aux row
        // rather than per-aux-name rows, and the summary should say
        // "aux names ignored".
        let projection: HashSet<String> = ["x".to_string()].into_iter().collect();
        let plain = strip_ansi(&render(&diff_with(
            "preserved: x ;\n1 x 8 f >= 1 ;\n",
            "preserved: x ;\n1 x 7 g >= 2 ;\n",
            CompareOptions {
                mode: CompareMode::Ordered,
                match_labels: false,
                label_check: None,
                aux_projection: Some(projection),
                ignore_missing_preserved: None,
            },
        )));
        assert!(plain.contains("aux names ignored"), "got: {plain}");
        // The folded aux row carries the coefficient multisets by side,
        // not per-name rows.
        assert!(
            plain.contains("aux (names ignored): A=[+8]"),
            "got: {plain}"
        );
        assert!(plain.contains("B=[+7]"), "got: {plain}");
        // The projected rhs difference is still shown by name/row.
        assert!(plain.contains("rhs"), "got: {plain}");
    }

    #[test]
    fn label_mismatch_is_reported_with_reference_side() {
        let plain = strip_ansi(&render(&diff_with(
            "1 x1 >= 1 ;\n",
            "@card 1 x1 >= 1 ;\n",
            CompareOptions {
                mode: CompareMode::Ordered,
                match_labels: false,
                label_check: Some(ReferenceSide::B),
                aux_projection: None,
                ignore_missing_preserved: None,
            },
        )));
        assert!(plain.contains("Label mismatch"));
        assert!(plain.contains("reference=B"));
        assert!(plain.contains("expected label: card"));
        assert!(plain.contains("actual label:   (none)"));
        assert!(plain.contains("1 label mismatch"));
    }

    #[test]
    fn label_permutation_swap_is_annotated_and_summarised() {
        // Two constraints with labels swapped between the files. Each
        // label-pair "differs", but the one-line correspondence replaces
        // the canonical dump and the summary calls out the permutation.
        let plain = strip_ansi(&render(&diff_with(
            "@le 1 x1 >= 1 ;\n@ge 1 x2 >= 1 ;\n",
            "@le 1 x2 >= 1 ;\n@ge 1 x1 >= 1 ;\n",
            CompareOptions {
                mode: CompareMode::Ordered,
                match_labels: true,
                label_check: None,
                aux_projection: None,
                ignore_missing_preserved: None,
            },
        )));
        assert!(
            plain.contains("A's @le canonically matches B's @ge (label swap)"),
            "got: {plain}"
        );
        assert!(
            plain.contains("all differing explained by a label permutation (1 swap)"),
            "got: {plain}"
        );
        // The one-liner replaces the full term-by-term dump.
        assert!(!plain.contains("canonical-form view"), "got: {plain}");
    }

    #[test]
    fn ignored_missing_preserved_is_silent_but_noted_in_descriptor() {
        // A lacks preserved:, B has it, constraints agree. With the
        // ignore option the report shows the success message (no
        // "Preserved" difference block) and notes the relaxation in the
        // descriptor.
        let plain = strip_ansi(&render(&diff_with(
            "1 x1 >= 1 ;\n",
            "preserved: x1 ;\n1 x1 >= 1 ;\n",
            CompareOptions {
                ignore_missing_preserved: Some(ReferenceSide::A),
                ..Default::default()
            },
        )));
        assert!(plain.contains("semantically equivalent"), "got: {plain}");
        assert!(
            plain.contains("missing preserved in A ignored"),
            "got: {plain}"
        );
        assert!(!plain.contains("Preserved"), "got: {plain}");
    }
}
