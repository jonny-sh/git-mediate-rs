use std::fmt::Write;

use colored::Colorize;
use similar::{ChangeTag, TextDiff};

use crate::types::Conflict;

/// Display diffs of each side against the base for a conflict.
///
/// Shows two diffs:
/// - "Ours" vs base
/// - "Theirs" vs base
pub fn show_side_diffs(conflict: &Conflict, color: bool, context: usize) -> String {
    let base_text = conflict.bodies.base.join("\n");
    let ours_text = conflict.bodies.ours.join("\n");
    let theirs_text = conflict.bodies.theirs.join("\n");

    let mut out = String::new();

    let ours_label = conflict.markers.ours.text.trim_start_matches('<').trim();
    let theirs_label = conflict.markers.theirs.text.trim_start_matches('>').trim();

    writeln!(
        out,
        "--- {}, line {}",
        if ours_label.is_empty() {
            "ours"
        } else {
            ours_label
        },
        conflict.start_line()
    )
    .unwrap();
    format_diff(&mut out, &base_text, &ours_text, color, context);

    writeln!(
        out,
        "--- {}, line {}",
        if theirs_label.is_empty() {
            "theirs"
        } else {
            theirs_label
        },
        conflict.start_line()
    )
    .unwrap();
    format_diff(&mut out, &base_text, &theirs_text, color, context);

    out
}

/// Display a direct diff between side A and side B.
pub fn show_diff2(conflict: &Conflict, color: bool, context: usize) -> String {
    let ours_text = conflict.bodies.ours.join("\n");
    let theirs_text = conflict.bodies.theirs.join("\n");

    let mut out = String::new();
    writeln!(out, "--- line {}", conflict.start_line()).unwrap();
    format_diff(&mut out, &ours_text, &theirs_text, color, context);
    out
}

fn format_diff(out: &mut String, old: &str, new: &str, color: bool, context: usize) {
    let diff = TextDiff::from_lines(old, new);
    let changes: Vec<_> = diff.iter_all_changes().collect();
    let trimmed = trim_changes(&changes, context);

    for change in trimmed {
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
                ChangeTag::Equal => writeln!(out, "{formatted}"),
            }
        } else {
            writeln!(out, "{formatted}")
        }
        .unwrap();
    }
}

fn trim_changes<'a>(
    changes: &'a [similar::Change<&'a str>],
    context: usize,
) -> &'a [similar::Change<&'a str>] {
    if changes.is_empty() {
        return changes;
    }

    let start_equal = changes
        .iter()
        .take_while(|change| change.tag() == ChangeTag::Equal)
        .count();
    let end_equal = changes
        .iter()
        .rev()
        .take_while(|change| change.tag() == ChangeTag::Equal)
        .count();

    let start = start_equal.saturating_sub(context);
    let end = (changes.len() - end_equal + context).min(changes.len());
    &changes[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ConflictBody, ConflictMarkers, ConflictSides, SrcContent};

    fn body(lines: &[&str]) -> ConflictBody {
        ConflictBody::from(
            lines
                .iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>(),
        )
    }

    fn make_conflict(ours: &[&str], base: &[&str], theirs: &[&str]) -> Conflict {
        Conflict {
            markers: ConflictMarkers::new(
                SrcContent::new(1, "<<<<<<< HEAD".to_string()),
                SrcContent::new(2, "||||||| base".to_string()),
                SrcContent::new(3, "=======".to_string()),
                SrcContent::new(4, ">>>>>>> feature".to_string()),
            ),
            bodies: ConflictSides::new(body(ours), body(base), body(theirs)),
        }
    }

    #[test]
    fn test_side_diffs_no_color() {
        let c = make_conflict(&["changed_a"], &["original"], &["changed_b"]);
        let output = show_side_diffs(&c, false, 3);

        assert!(output.contains("HEAD"));
        assert!(output.contains("feature"));
        assert!(output.contains("-original"));
        assert!(output.contains("+changed_a"));
        assert!(output.contains("+changed_b"));
    }

    #[test]
    fn test_diff2_no_color() {
        let c = make_conflict(&["line_a"], &["base"], &["line_b"]);
        let output = show_diff2(&c, false, 3);

        assert!(output.contains("-line_a"));
        assert!(output.contains("+line_b"));
    }

    #[test]
    fn test_side_diffs_one_side_unchanged() {
        let c = make_conflict(&["original"], &["original"], &["changed"]);
        let output = show_side_diffs(&c, false, 3);

        assert!(output.contains("+changed"));
        assert!(output.contains("-original"));
    }
}
