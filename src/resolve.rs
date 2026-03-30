use crate::types::{Chunk, Conflict, Resolution, Sides};

/// Attempt to resolve a single conflict.
///
/// Resolution strategies (applied in order):
/// 1. **Trivial** — all three sides identical → resolved
/// 2. **One-side unchanged** — if A==base, take B (and vice versa)
/// 3. **Both same change** — if A==B (but ≠ base), take A
/// 4. **Prefix/suffix reduction** — strip common leading/trailing lines,
///    then re-check the reduced conflict
pub fn resolve_conflict(conflict: &Conflict) -> Resolution {
    let a = &conflict.bodies.a;
    let base = &conflict.bodies.base;
    let b = &conflict.bodies.b;

    // 1. All three sides identical
    if a == base && base == b {
        return Resolution::Resolved(lines_to_string(a));
    }

    // 2. One side unchanged from base
    if a == base {
        return Resolution::Resolved(lines_to_string(b));
    }
    if b == base {
        return Resolution::Resolved(lines_to_string(a));
    }

    // 3. Both made the same change
    if a == b {
        return Resolution::Resolved(lines_to_string(a));
    }

    // 4. Try prefix/suffix reduction
    try_reduce(conflict)
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
}
