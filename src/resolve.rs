use crate::types::{Chunk, Conflict, Resolution, Sides};

/// Attempt to resolve a single conflict by applying strategies in sequence.
///
/// Resolution strategies (applied in order):
/// 1. **Line endings normalization** — strip `\r` differences, retry trivial
/// 2. **Trivial** — all three sides identical → resolved
/// 3. **One-side unchanged** — if A==base, take B (and vice versa)
/// 4. **Both same change** — if A==B (but ≠ base), take A
/// 5. **Indentation-aware** — strip common leading whitespace, retry trivial
/// 6. **Prefix/suffix reduction** — strip common leading/trailing lines,
///    then re-check the reduced conflict
pub fn resolve_conflict(conflict: &Conflict) -> Resolution {
    // First try line-ending normalization
    if let Some(res) = try_line_endings(conflict) {
        return res;
    }

    // Core trivial checks
    if let Some(res) = try_trivial(conflict) {
        return res;
    }

    // Indentation-aware resolution
    if let Some(res) = try_indentation(conflict) {
        return res;
    }

    // Prefix/suffix reduction
    try_reduce(conflict)
}

/// Try trivial resolution: all-equal, one-side-unchanged, both-same-change.
fn try_trivial(conflict: &Conflict) -> Option<Resolution> {
    let a = &conflict.bodies.a;
    let base = &conflict.bodies.base;
    let b = &conflict.bodies.b;

    if a == base && base == b {
        return Some(Resolution::Resolved(lines_to_string(a)));
    }
    if a == base {
        return Some(Resolution::Resolved(lines_to_string(b)));
    }
    if b == base {
        return Some(Resolution::Resolved(lines_to_string(a)));
    }
    if a == b {
        return Some(Resolution::Resolved(lines_to_string(a)));
    }
    None
}

/// Normalize line endings (strip `\r`) across all sides, then retry trivial.
fn try_line_endings(conflict: &Conflict) -> Option<Resolution> {
    let normalize = |lines: &[String]| -> Vec<String> {
        lines.iter().map(|l| l.replace('\r', "")).collect()
    };

    let norm_a = normalize(&conflict.bodies.a);
    let norm_base = normalize(&conflict.bodies.base);
    let norm_b = normalize(&conflict.bodies.b);

    // Only useful if normalization actually changed something
    if norm_a == conflict.bodies.a && norm_base == conflict.bodies.base && norm_b == conflict.bodies.b {
        return None;
    }

    let mut normalized = conflict.clone();
    normalized.bodies = Sides::new(norm_a, norm_base, norm_b);
    try_trivial(&normalized)
}

/// Resolve conflicts where one side re-indented code while the other changed content.
///
/// The approach:
/// 1. Compute each side's common indentation prefix independently
/// 2. Strip each side by its own prefix
/// 3. Resolve indent and content as two independent trivial merges
/// 4. Combine winning indent + winning content
///
/// Example: base=`    foo`, A=`        foo` (re-indented), B=`    bar` (changed content)
/// → indent: A changed (4→8), B unchanged → take A's indent (8 spaces)
/// → content: A unchanged ("foo"), B changed → take B's content ("bar")
/// → result: `        bar`
fn try_indentation(conflict: &Conflict) -> Option<Resolution> {
    let a = &conflict.bodies.a;
    let base = &conflict.bodies.base;
    let b = &conflict.bodies.b;

    // Compute each side's own common indent (longest whitespace prefix
    // shared by all non-empty lines within that side)
    let indent_a = common_indent(a);
    let indent_base = common_indent(base);
    let indent_b = common_indent(b);

    // If all indents are identical, indentation isn't the issue — bail out
    if indent_a == indent_base && indent_base == indent_b {
        return None;
    }

    // Strip each side by its own indent
    let strip = |lines: &[String], indent: &str| -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                if indent.is_empty() {
                    l.clone()
                } else if l.starts_with(indent) {
                    l[indent.len()..].to_string()
                } else if l.trim().is_empty() {
                    l.clone()
                } else {
                    l.clone()
                }
            })
            .collect()
    };

    let stripped_a = strip(a, &indent_a);
    let stripped_base = strip(base, &indent_base);
    let stripped_b = strip(b, &indent_b);

    // Resolve indentation trivially (three-way merge of the indent strings)
    let resolved_indent = trivial_pick(&indent_a, &indent_base, &indent_b)?;

    // Resolve content trivially (three-way merge of the stripped lines)
    let resolved_content = trivial_pick(&stripped_a, &stripped_base, &stripped_b)?;

    // Re-apply the resolved indent to the resolved content
    let result: Vec<String> = resolved_content
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                l.clone()
            } else {
                format!("{}{}", resolved_indent, l)
            }
        })
        .collect();

    Some(Resolution::Resolved(lines_to_string(&result)))
}

/// Three-way trivial merge: if one side matches base, take the other.
/// If both sides are equal, take either. Otherwise, conflict.
fn trivial_pick<'a, T: PartialEq>(a: &'a T, base: &'a T, b: &'a T) -> Option<&'a T> {
    if a == base && base == b {
        Some(a)
    } else if a == base {
        Some(b)
    } else if b == base {
        Some(a)
    } else if a == b {
        Some(a)
    } else {
        None
    }
}

/// Find the longest whitespace prefix common to all non-empty lines within a single side.
fn common_indent(lines: &[String]) -> String {
    let mut prefix: Option<String> = None;

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        prefix = Some(match prefix {
            None => ws,
            Some(existing) => common_string_prefix(&existing, &ws),
        });
    }

    prefix.unwrap_or_default()
}

fn common_string_prefix(a: &str, b: &str) -> String {
    a.chars()
        .zip(b.chars())
        .take_while(|(ca, cb)| ca == cb)
        .map(|(c, _)| c)
        .collect()
}

/// Strip common prefix and suffix lines from a conflict, producing a reduced conflict.
fn try_reduce(conflict: &Conflict) -> Resolution {
    let a = &conflict.bodies.a;
    let base = &conflict.bodies.base;
    let b = &conflict.bodies.b;

    // Find common prefix length (lines shared by all three sides)
    let prefix_len = a
        .iter()
        .zip(base.iter())
        .zip(b.iter())
        .take_while(|((la, lb), lc)| la == lb && lb == lc)
        .count();

    // Find common suffix length (don't overlap with prefix)
    let max_suffix = a.len().min(base.len()).min(b.len()) - prefix_len;
    let suffix_len = a
        .iter()
        .rev()
        .zip(base.iter().rev())
        .zip(b.iter().rev())
        .take(max_suffix)
        .take_while(|((la, lb), lc)| la == lb && lb == lc)
        .count();

    if prefix_len == 0 && suffix_len == 0 {
        return Resolution::Unchanged;
    }

    let new_a = a[prefix_len..a.len() - suffix_len].to_vec();
    let new_base = base[prefix_len..base.len() - suffix_len].to_vec();
    let new_b = b[prefix_len..b.len() - suffix_len].to_vec();

    let prefix = &a[..prefix_len];
    let suffix = &a[a.len() - suffix_len..];

    // Re-check trivial cases on the reduced conflict
    if new_a == new_base && new_base == new_b {
        let mut result = prefix.to_vec();
        result.extend(new_a);
        result.extend_from_slice(suffix);
        return Resolution::Resolved(lines_to_string(&result));
    }
    if new_a == new_base {
        let mut result = prefix.to_vec();
        result.extend(new_b);
        result.extend_from_slice(suffix);
        return Resolution::Resolved(lines_to_string(&result));
    }
    if new_b == new_base {
        let mut result = prefix.to_vec();
        result.extend(new_a);
        result.extend_from_slice(suffix);
        return Resolution::Resolved(lines_to_string(&result));
    }
    if new_a == new_b {
        let mut result = prefix.to_vec();
        result.extend(new_a);
        result.extend_from_slice(suffix);
        return Resolution::Resolved(lines_to_string(&result));
    }

    // Still conflicting — return reduced conflict
    let mut reduced = conflict.clone();
    reduced.bodies = Sides::new(new_a, new_base, new_b);
    Resolution::PartiallyReduced(reduced)
}

/// Resolve all conflicts in a list of chunks, returning the new content and stats.
pub fn resolve_chunks(chunks: Vec<Chunk>) -> (Vec<Chunk>, crate::types::FileResult) {
    let mut result = Vec::new();
    let mut stats = crate::types::FileResult::default();

    for chunk in chunks {
        match chunk {
            Chunk::Plain(_) => result.push(chunk),
            Chunk::Conflict(conflict) => match resolve_conflict(&conflict) {
                Resolution::Resolved(text) => {
                    stats.resolved += 1;
                    result.push(Chunk::Plain(text));
                }
                Resolution::PartiallyReduced(reduced) => {
                    stats.partially_resolved += 1;
                    result.push(Chunk::Conflict(reduced));
                }
                Resolution::Unchanged => {
                    stats.failed += 1;
                    result.push(Chunk::Conflict(conflict));
                }
            },
        }
    }

    (result, stats)
}

fn lines_to_string(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_conflicts;
    use crate::types::SrcContent;

    fn make_conflict(a: &[&str], base: &[&str], b: &[&str]) -> Conflict {
        Conflict {
            marker_a: SrcContent::new(1, "<<<<<<< HEAD".to_string()),
            marker_base: SrcContent::new(2, "||||||| base".to_string()),
            marker_b: SrcContent::new(3, "=======".to_string()),
            marker_end: SrcContent::new(4, ">>>>>>> branch".to_string()),
            bodies: Sides::new(
                a.iter().map(|s| s.to_string()).collect(),
                base.iter().map(|s| s.to_string()).collect(),
                b.iter().map(|s| s.to_string()).collect(),
            ),
        }
    }

    #[test]
    fn test_all_equal() {
        let c = make_conflict(&["same"], &["same"], &["same"]);
        assert!(matches!(resolve_conflict(&c), Resolution::Resolved(text) if text == "same\n"));
    }

    #[test]
    fn test_a_unchanged_take_b() {
        let c = make_conflict(&["base"], &["base"], &["theirs"]);
        assert!(
            matches!(resolve_conflict(&c), Resolution::Resolved(text) if text == "theirs\n")
        );
    }

    #[test]
    fn test_b_unchanged_take_a() {
        let c = make_conflict(&["ours"], &["base"], &["base"]);
        assert!(matches!(resolve_conflict(&c), Resolution::Resolved(text) if text == "ours\n"));
    }

    #[test]
    fn test_both_same_change() {
        let c = make_conflict(&["new"], &["old"], &["new"]);
        assert!(matches!(resolve_conflict(&c), Resolution::Resolved(text) if text == "new\n"));
    }

    #[test]
    fn test_true_conflict() {
        let c = make_conflict(&["ours"], &["base"], &["theirs"]);
        let res = resolve_conflict(&c);
        assert!(matches!(res, Resolution::Unchanged));
    }

    #[test]
    fn test_prefix_suffix_reduction() {
        let c = make_conflict(
            &["common1", "ours", "common2"],
            &["common1", "base", "common2"],
            &["common1", "theirs", "common2"],
        );
        let res = resolve_conflict(&c);
        match res {
            Resolution::PartiallyReduced(reduced) => {
                assert_eq!(reduced.bodies.a, vec!["ours"]);
                assert_eq!(reduced.bodies.base, vec!["base"]);
                assert_eq!(reduced.bodies.b, vec!["theirs"]);
            }
            other => panic!("expected PartiallyReduced, got {:?}", other),
        }
    }

    #[test]
    fn test_prefix_suffix_resolves() {
        let c = make_conflict(
            &["common1", "base", "common2"],
            &["common1", "base", "common2"],
            &["common1", "theirs", "common2"],
        );
        let res = resolve_conflict(&c);
        assert!(matches!(res, Resolution::Resolved(_)));
    }

    #[test]
    fn test_resolve_chunks_mixed() {
        let input = "\
before
<<<<<<< HEAD
base
||||||| base
base
=======
theirs
>>>>>>> branch
middle
<<<<<<< HEAD
ours
||||||| base
base
=======
theirs
>>>>>>> branch
after
";
        let chunks = parse_conflicts(input).unwrap();
        let (resolved, stats) = resolve_chunks(chunks);

        assert_eq!(stats.resolved, 1);
        assert_eq!(stats.failed, 1);

        let output = crate::parse::chunks_to_string(&resolved);
        assert!(output.contains("before\n"));
        assert!(output.contains("theirs\n"));
        assert!(output.contains("middle\n"));
        assert!(output.contains("<<<<<<<"));
    }

    #[test]
    fn test_empty_sides() {
        let c = make_conflict(&[], &["base"], &["base"]);
        let res = resolve_conflict(&c);
        assert!(matches!(res, Resolution::Resolved(text) if text.is_empty()));
    }

    #[test]
    fn test_both_deleted() {
        let c = make_conflict(&[], &["base"], &[]);
        let res = resolve_conflict(&c);
        assert!(matches!(res, Resolution::Resolved(text) if text.is_empty()));
    }

    #[test]
    fn test_prefix_reduction_then_resolved() {
        let c = make_conflict(
            &["common", "base_line"],
            &["common", "base_line"],
            &["common", "new_line"],
        );
        assert!(matches!(resolve_conflict(&c), Resolution::Resolved(_)));
    }

    #[test]
    fn test_reduce_resolves_when_one_side_matches_base() {
        // After prefix/suffix strip: new_b == new_base → take new_a
        let c = make_conflict(
            &["header", "changed_by_a", "footer"],
            &["header", "original", "footer"],
            &["header", "original", "footer"],
        );
        // b == base at top level
        let res = resolve_conflict(&c);
        match res {
            Resolution::Resolved(text) => {
                assert_eq!(text, "header\nchanged_by_a\nfooter\n");
            }
            other => panic!("expected Resolved, got {:?}", other),
        }
    }

    #[test]
    fn test_reduce_resolves_both_same_inner_change() {
        let c = make_conflict(
            &["header", "new_val", "footer"],
            &["header", "old_val", "footer"],
            &["header", "new_val", "footer"],
        );
        // a == b at top level
        let res = resolve_conflict(&c);
        match res {
            Resolution::Resolved(text) => {
                assert_eq!(text, "header\nnew_val\nfooter\n");
            }
            other => panic!("expected Resolved, got {:?}", other),
        }
    }

    // --- Line endings tests ---

    #[test]
    fn test_line_endings_crlf_vs_lf_a_unchanged() {
        // A has CRLF, base and B have LF — only difference is line endings
        // A matches base after normalization → take B
        let c = make_conflict(&["same\r"], &["same"], &["changed"]);
        let res = resolve_conflict(&c);
        assert!(matches!(res, Resolution::Resolved(text) if text == "changed\n"));
    }

    #[test]
    fn test_line_endings_all_same_after_strip() {
        let c = make_conflict(&["line\r"], &["line"], &["line\r"]);
        let res = resolve_conflict(&c);
        assert!(matches!(res, Resolution::Resolved(_)));
    }

    #[test]
    fn test_line_endings_no_effect_on_real_conflict() {
        // Real content difference, not just line endings
        let c = make_conflict(&["ours\r"], &["base"], &["theirs"]);
        let res = resolve_conflict(&c);
        // After normalization: ours != base != theirs — still a conflict
        assert!(matches!(
            res,
            Resolution::Unchanged | Resolution::PartiallyReduced(_)
        ));
    }

    // --- Indentation tests ---

    #[test]
    fn test_indent_reindent_plus_content_change() {
        // The key use case: A re-indented (4→8 spaces), B changed content.
        // Should merge: A's indent + B's content.
        let c = make_conflict(
            &["        foo", "        bar"],  // A: re-indented to 8 spaces
            &["    foo", "    bar"],           // base: 4 spaces
            &["    foo", "    baz"],           // B: changed bar→baz, kept indent
        );
        let res = resolve_conflict(&c);
        match res {
            Resolution::Resolved(text) => {
                assert_eq!(text, "        foo\n        baz\n");
            }
            other => panic!("expected Resolved, got {:?}", other),
        }
    }

    #[test]
    fn test_indent_content_change_plus_reindent() {
        // Mirror: A changed content, B re-indented.
        let c = make_conflict(
            &["    foo", "    baz"],           // A: changed bar→baz
            &["    foo", "    bar"],           // base: 4 spaces
            &["        foo", "        bar"],  // B: re-indented to 8 spaces
        );
        let res = resolve_conflict(&c);
        match res {
            Resolution::Resolved(text) => {
                assert_eq!(text, "        foo\n        baz\n");
            }
            other => panic!("expected Resolved, got {:?}", other),
        }
    }

    #[test]
    fn test_indent_both_reindented_same_way() {
        // Both sides re-indented identically, one also changed content.
        let c = make_conflict(
            &["        foo", "        baz"],  // A: re-indented + changed content
            &["    foo", "    bar"],           // base
            &["        foo", "        bar"],  // B: only re-indented
        );
        let res = resolve_conflict(&c);
        // indent: A==B → take either (8 spaces)
        // content: B==base after strip → take A's content
        match res {
            Resolution::Resolved(text) => {
                assert_eq!(text, "        foo\n        baz\n");
            }
            other => panic!("expected Resolved, got {:?}", other),
        }
    }

    #[test]
    fn test_indent_same_indent_no_help() {
        // All sides have the same indentation — strategy should bail out,
        // fall through to other strategies.
        let c = make_conflict(
            &["    ours"],
            &["    base"],
            &["    theirs"],
        );
        let res = resolve_conflict(&c);
        // Same indent on all sides → indentation can't help, real conflict
        assert!(matches!(res, Resolution::Unchanged | Resolution::PartiallyReduced(_)));
    }

    #[test]
    fn test_indent_conflicting_reindent_and_content() {
        // Both sides changed indent differently AND changed content → unresolvable
        let c = make_conflict(
            &["        ours"],   // A: 8-space indent + different content
            &["    base"],       // base: 4-space indent
            &["      theirs"],   // B: 6-space indent + different content
        );
        let res = resolve_conflict(&c);
        assert!(matches!(res, Resolution::Unchanged | Resolution::PartiallyReduced(_)));
    }

    #[test]
    fn test_indent_only_reindent_no_content_change() {
        // A re-indented, B unchanged → take A (just an indent change)
        let c = make_conflict(
            &["        foo", "        bar"],  // A: re-indented
            &["    foo", "    bar"],           // base
            &["    foo", "    bar"],           // B: unchanged
        );
        let res = resolve_conflict(&c);
        match res {
            Resolution::Resolved(text) => {
                assert_eq!(text, "        foo\n        bar\n");
            }
            other => panic!("expected Resolved, got {:?}", other),
        }
    }

    #[test]
    fn test_indent_no_indent_at_all() {
        // No whitespace prefix on any side — should bail out
        let c = make_conflict(&["ours"], &["base"], &["theirs"]);
        let res = resolve_conflict(&c);
        assert!(matches!(res, Resolution::Unchanged));
    }
}
