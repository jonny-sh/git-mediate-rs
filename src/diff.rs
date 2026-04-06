use std::fmt::Write;

use colored::Colorize;
use similar::{ChangeTag, TextDiff};

use crate::types::Conflict;

/// Display diffs of each side against the base for a conflict.
///
/// Shows two diffs:
/// - "Ours" (side A) vs base
/// - "Theirs" (side B) vs base
pub fn show_side_diffs(conflict: &Conflict, color: bool) -> String {
    let base_text = conflict.bodies.base.join("\n");
    let a_text = conflict.bodies.a.join("\n");
    let b_text = conflict.bodies.b.join("\n");

    let mut out = String::new();

    let marker_a_label = conflict.marker_a.text.trim_start_matches('<').trim();
    let marker_b_label = conflict.marker_end.text.trim_start_matches('>').trim();

    writeln!(
        out,
        "--- {}, line {}",
        if marker_a_label.is_empty() {
            "ours"
        } else {
            marker_a_label
        },
        conflict.start_line()
    )
    .unwrap();
    format_diff(&mut out, &base_text, &a_text, color);

    writeln!(
        out,
        "--- {}, line {}",
        if marker_b_label.is_empty() {
            "theirs"
        } else {
            marker_b_label
        },
        conflict.start_line()
    )
    .unwrap();
    format_diff(&mut out, &base_text, &b_text, color);

    out
}

/// Display a direct diff between side A and side B.
pub fn show_diff2(conflict: &Conflict, color: bool) -> String {
    let a_text = conflict.bodies.a.join("\n");
    let b_text = conflict.bodies.b.join("\n");

    let mut out = String::new();
    writeln!(out, "--- line {}", conflict.start_line()).unwrap();
    format_diff(&mut out, &a_text, &b_text, color);
    out
}

fn format_diff(out: &mut String, old: &str, new: &str, color: bool) {
    let diff = TextDiff::from_lines(old, new);

    for change in diff.iter_all_changes() {
        let (sign, line) = match change.tag() {
            ChangeTag::Delete => ("-", change.value()),
            ChangeTag::Insert => ("+", change.value()),
            ChangeTag::Equal => (" ", change.value()),
        };

        let formatted = format!("{}{}", sign, line.trim_end_matches('\n'));
        if color {
            match change.tag() {
                ChangeTag::Delete => writeln!(out, "{}", formatted.red()),
                ChangeTag::Insert => writeln!(out, "{}", formatted.green()),
                ChangeTag::Equal => writeln!(out, "{}", formatted),
            }
        } else {
            writeln!(out, "{}", formatted)
        }
        .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Sides, SrcContent};

    fn make_conflict(a: &[&str], base: &[&str], b: &[&str]) -> Conflict {
        Conflict {
            marker_a: SrcContent::new(1, "<<<<<<< HEAD".to_string()),
            marker_base: SrcContent::new(2, "||||||| base".to_string()),
            marker_b: SrcContent::new(3, "=======".to_string()),
            marker_end: SrcContent::new(4, ">>>>>>> feature".to_string()),
            bodies: Sides::new(
                a.iter().map(|s| s.to_string()).collect(),
                base.iter().map(|s| s.to_string()).collect(),
                b.iter().map(|s| s.to_string()).collect(),
            ),
        }
    }

    #[test]
    fn test_side_diffs_no_color() {
        let c = make_conflict(&["changed_a"], &["original"], &["changed_b"]);
        let output = show_side_diffs(&c, false);

        assert!(output.contains("HEAD"));
        assert!(output.contains("feature"));
        assert!(output.contains("-original"));
        assert!(output.contains("+changed_a"));
        assert!(output.contains("+changed_b"));
    }

    #[test]
    fn test_diff2_no_color() {
        let c = make_conflict(&["line_a"], &["base"], &["line_b"]);
        let output = show_diff2(&c, false);

        assert!(output.contains("-line_a"));
        assert!(output.contains("+line_b"));
    }

    #[test]
    fn test_side_diffs_one_side_unchanged() {
        let c = make_conflict(&["original"], &["original"], &["changed"]);
        let output = show_side_diffs(&c, false);

        // A vs base should show no changes (just equal lines)
        // B vs base should show a change
        assert!(output.contains("+changed"));
        assert!(output.contains("-original"));
    }
}
