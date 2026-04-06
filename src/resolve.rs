use crate::parse::parse_conflicts;
use crate::types::{Chunk, Conflict, FileResult, Resolution, Sides};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveOptions {
    pub trivial: bool,
    pub reduce: bool,
    pub untabify: Option<usize>,
    pub line_endings: bool,
    pub lines_added_around: bool,
    pub split_markers: bool,
    pub indentation: bool,
}

impl Default for ResolveOptions {
    fn default() -> Self {
        Self {
            trivial: true,
            reduce: true,
            untabify: None,
            line_endings: true,
            lines_added_around: false,
            split_markers: true,
            indentation: false,
        }
    }
}

pub fn resolve_conflict(conflict: &Conflict) -> Resolution {
    resolve_conflict_with_options(conflict, &ResolveOptions::default())
}

pub fn resolve_conflict_with_options(conflict: &Conflict, options: &ResolveOptions) -> Resolution {
    let conflict = preprocess_conflict(conflict.clone(), options);
    resolve_conflict_struct(&conflict, options)
}

pub fn resolve_chunks(chunks: Vec<Chunk>) -> (Vec<Chunk>, FileResult) {
    resolve_chunks_with_options(chunks, &ResolveOptions::default())
}

pub fn resolve_chunks_with_options(chunks: Vec<Chunk>, options: &ResolveOptions) -> (Vec<Chunk>, FileResult) {
    let mut result = Vec::new();
    let mut stats = FileResult::default();

    for chunk in chunks {
        match chunk {
            Chunk::Plain(text) => result.push(Chunk::Plain(text)),
            Chunk::Conflict(conflict) => {
                let (chunk_stats, text) = resolve_conflict_text(&conflict, options);
                stats.resolved += chunk_stats.resolved;
                stats.partially_resolved += chunk_stats.partially_resolved;
                stats.failed += chunk_stats.failed;

                let rebuilt = parse_conflicts(&text)
                    .expect("resolver should always emit valid plain text or diff3 conflicts");
                result.extend(rebuilt);
            }
        }
    }

    (result, stats)
}

fn resolve_conflict_struct(conflict: &Conflict, options: &ResolveOptions) -> Resolution {
    let bodies = &conflict.bodies;
    if let Some(lines) = resolve_reduced(options, bodies) {
        return Resolution::Resolved(lines_to_string(&lines));
    }

    let reduction = compute_reduction(conflict, options);
    if (reduction.match_top == 0 && reduction.match_bottom == 0) || !options.reduce {
        return Resolution::Unchanged;
    }

    if let Some(lines) = resolve_gen_lines(options, &reduction.reduced.a, &reduction.reduced.base, &reduction.reduced.b) {
        let mut out = reduction.prefix.clone();
        out.extend(lines);
        out.extend(reduction.suffix.clone());
        return Resolution::Resolved(lines_to_string(&out));
    }

    let mut reduced = conflict.clone();
    reduced.bodies = reduction.reduced;
    Resolution::PartiallyReduced(reduced)
}

fn resolve_conflict_text(conflict: &Conflict, options: &ResolveOptions) -> (FileResult, String) {
    let parts = if options.split_markers {
        split_conflict(conflict)
    } else {
        vec![conflict.clone()]
    };

    let mut combined = String::new();
    let mut aggregate = FileResult::default();

    for part in &parts {
        let processed = preprocess_conflict(part.clone(), options);
        let (part_stats, text) = resolve_part_text(&processed, options);
        aggregate.resolved += part_stats.resolved;
        aggregate.partially_resolved += part_stats.partially_resolved;
        aggregate.failed += part_stats.failed;
        combined.push_str(&text);
    }

    if parts.len() > 1 {
        if aggregate.failed > 0 || aggregate.partially_resolved > 0 {
            aggregate = FileResult {
                resolved: 0,
                partially_resolved: 1,
                failed: 0,
            };
        } else {
            aggregate = FileResult {
                resolved: 1,
                partially_resolved: 0,
                failed: 0,
            };
        }
    }

    (aggregate, combined)
}

fn resolve_part_text(conflict: &Conflict, options: &ResolveOptions) -> (FileResult, String) {
    if let Some(lines) = resolve_reduced(options, &conflict.bodies) {
        return (
            FileResult {
                resolved: 1,
                partially_resolved: 0,
                failed: 0,
            },
            lines_to_string(&lines),
        );
    }

    let reduction = compute_reduction(conflict, options);
    if (reduction.match_top == 0 && reduction.match_bottom == 0) || !options.reduce {
        return (
            FileResult {
                resolved: 0,
                partially_resolved: 0,
                failed: 1,
            },
            conflict.to_conflict_text(),
        );
    }

    if let Some(lines) = resolve_gen_lines(options, &reduction.reduced.a, &reduction.reduced.base, &reduction.reduced.b) {
        let mut out = reduction.prefix;
        out.extend(lines);
        out.extend(reduction.suffix);
        return (
            FileResult {
                resolved: 1,
                partially_resolved: 0,
                failed: 0,
            },
            lines_to_string(&out),
        );
    }

    let mut reduced_lines = reduction.prefix;
    reduced_lines.extend(conflict_to_lines(&Conflict {
        marker_a: conflict.marker_a.clone(),
        marker_base: conflict.marker_base.clone(),
        marker_b: conflict.marker_b.clone(),
        marker_end: conflict.marker_end.clone(),
        bodies: reduction.reduced,
    }));
    reduced_lines.extend(reduction.suffix);

    (
        FileResult {
            resolved: 0,
            partially_resolved: 1,
            failed: 0,
        },
        lines_to_string(&reduced_lines),
    )
}

fn preprocess_conflict(mut conflict: Conflict, options: &ResolveOptions) -> Conflict {
    if let Some(tabsize) = options.untabify {
        conflict = map_conflict_strings(&conflict, |line| untabify_str(line, tabsize));
    }
    if options.line_endings {
        conflict = line_break_fix(&conflict);
    }
    conflict
}

fn map_conflict_strings(conflict: &Conflict, f: impl Fn(&str) -> String) -> Conflict {
    let mut mapped = conflict.clone();
    mapped.bodies = Sides::new(
        conflict.bodies.a.iter().map(|line| f(line)).collect(),
        conflict.bodies.base.iter().map(|line| f(line)).collect(),
        conflict.bodies.b.iter().map(|line| f(line)).collect(),
    );
    mapped
}

fn untabify_str(input: &str, tabsize: usize) -> String {
    let mut out = String::new();
    let mut col = 0usize;
    for ch in input.chars() {
        if ch == '\t' {
            let spaces = tabsize.saturating_sub(col % tabsize).max(1);
            out.push_str(&" ".repeat(spaces));
            col += spaces;
        } else {
            out.push(ch);
            col += 1;
        }
    }
    out
}

fn line_break_fix(conflict: &Conflict) -> Conflict {
    let endings = [
        infer_line_endings(&conflict.bodies.a),
        infer_line_endings(&conflict.bodies.base),
        infer_line_endings(&conflict.bodies.b),
    ];
    if conflict.bodies.a.iter().any(|line| line.is_empty())
        || conflict.bodies.base.iter().any(|line| line.is_empty())
        || conflict.bodies.b.iter().any(|line| line.is_empty())
        || endings.windows(2).all(|pair| pair[0] == pair[1])
    {
        return conflict.clone();
    }

    match resolve_gen(&endings[0], &endings[1], &endings[2]) {
        Some(LineEnding::Lf) => map_conflict_strings(conflict, |line| line.trim_end_matches('\r').to_string()),
        Some(LineEnding::Crlf) => map_conflict_strings(conflict, |line| {
            if line.ends_with('\r') {
                line.to_string()
            } else {
                format!("{line}\r")
            }
        }),
        _ => conflict.clone(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineEnding {
    Lf,
    Crlf,
    Mixed,
}

fn infer_line_endings(lines: &[String]) -> LineEnding {
    if lines.is_empty() {
        return LineEnding::Mixed;
    }

    let mut current: Option<LineEnding> = None;
    for line in lines {
        let ending = if line.ends_with('\r') {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        };
        match current {
            None => current = Some(ending),
            Some(existing) if existing == ending => {}
            Some(_) => return LineEnding::Mixed,
        }
    }

    current.unwrap_or(LineEnding::Mixed)
}

fn resolve_reduced(options: &ResolveOptions, sides: &Sides<Vec<String>>) -> Option<Vec<String>> {
    if options.indentation {
        let prefixes = Sides::new(
            indentation_prefix(&sides.a),
            indentation_prefix(&sides.base),
            indentation_prefix(&sides.b),
        );
        let unprefixed = Sides::new(
            strip_prefix_from_lines(&sides.a, &prefixes.a),
            strip_prefix_from_lines(&sides.base, &prefixes.base),
            strip_prefix_from_lines(&sides.b, &prefixes.b),
        );
        let prefix = resolve_gen(&prefixes.a, &prefixes.base, &prefixes.b)?;
        let lines = resolve_gen_lines(options, &unprefixed.a, &unprefixed.base, &unprefixed.b)?;
        return Some(
            lines
                .into_iter()
                .map(|line| {
                    if line.is_empty() {
                        line
                    } else {
                        format!("{prefix}{line}")
                    }
                })
                .collect(),
        );
    }

    resolve_gen_lines(options, &sides.a, &sides.base, &sides.b)
}

fn resolve_gen<T: Eq + Clone>(a: &T, base: &T, b: &T) -> Option<T> {
    if a == base {
        Some(b.clone())
    } else if b == base {
        Some(a.clone())
    } else if a == b {
        Some(a.clone())
    } else {
        None
    }
}

fn resolve_gen_lines(
    options: &ResolveOptions,
    a: &[String],
    base: &[String],
    b: &[String],
) -> Option<Vec<String>> {
    if options.trivial {
        if a == base {
            return Some(b.to_vec());
        }
        if b == base {
            return Some(a.to_vec());
        }
        if a == b {
            return Some(a.to_vec());
        }
    }

    if options.lines_added_around {
        let mut candidates = Vec::new();
        if let Some(lines) = added_both_sides(a, base, b) {
            candidates.push(lines);
        }
        if let Some(lines) = added_both_sides(b, base, a) {
            candidates.push(lines);
        }
        if candidates.len() == 1 {
            return candidates.into_iter().next();
        }
    }

    None
}

fn added_both_sides(
    left: &[String],
    base: &[String],
    right: &[String],
) -> Option<Vec<String>> {
    if left.len() < base.len() || right.len() < base.len() {
        return None;
    }
    if left[left.len() - base.len()..] != *base || right[..base.len()] != *base {
        return None;
    }

    let mut out = left.to_vec();
    out.extend_from_slice(&right[base.len()..]);
    Some(out)
}

fn indentation_prefix(lines: &[String]) -> String {
    let common = common_string_prefixes(lines);
    common.chars().take_while(|c| *c == ' ').collect()
}

fn common_string_prefixes(lines: &[String]) -> String {
    let mut iter = lines.iter();
    let Some(first) = iter.next() else {
        return String::new();
    };
    let mut prefix = first.clone();
    for line in iter {
        prefix = common_string_prefix(&prefix, line);
        if prefix.is_empty() {
            break;
        }
    }
    prefix
}

fn strip_prefix_from_lines(lines: &[String], prefix: &str) -> Vec<String> {
    lines
        .iter()
        .map(|line| line.strip_prefix(prefix).unwrap_or(line).to_string())
        .collect()
}

fn common_string_prefix(a: &str, b: &str) -> String {
    a.chars()
        .zip(b.chars())
        .take_while(|(left, right)| left == right)
        .map(|(ch, _)| ch)
        .collect()
}

struct Reduction {
    match_top: usize,
    match_bottom: usize,
    prefix: Vec<String>,
    suffix: Vec<String>,
    reduced: Sides<Vec<String>>,
}

fn compute_reduction(conflict: &Conflict, options: &ResolveOptions) -> Reduction {
    let a = &conflict.bodies.a;
    let base = &conflict.bodies.base;
    let b = &conflict.bodies.b;

    let match_top = match_prefix(base, a, b);
    let base_bottom: Vec<String> = base.iter().skip(match_top).cloned().rev().collect();
    let a_bottom: Vec<String> = a.iter().skip(match_top).cloned().rev().collect();
    let b_bottom: Vec<String> = b.iter().skip(match_top).cloned().rev().collect();
    let match_bottom = match_prefix(&base_bottom, &a_bottom, &b_bottom);

    let reduced = if options.reduce {
        Sides::new(
            take_middle(a, match_top, match_bottom),
            take_middle(base, match_top, match_bottom),
            take_middle(b, match_top, match_bottom),
        )
    } else {
        Sides::new(a.clone(), base.clone(), b.clone())
    };

    Reduction {
        match_top,
        match_bottom,
        prefix: a.iter().take(match_top).cloned().collect(),
        suffix: a.iter().skip(a.len().saturating_sub(match_bottom)).cloned().collect(),
        reduced,
    }
}

fn match_prefix(base: &[String], a: &[String], b: &[String]) -> usize {
    if base.is_empty() {
        common_prefix_len(a, b)
    } else {
        common_prefix_len(base, a).min(common_prefix_len(base, b))
    }
}

fn common_prefix_len(left: &[String], right: &[String]) -> usize {
    left.iter()
        .zip(right.iter())
        .take_while(|(a, b)| a == b)
        .count()
}

fn take_middle(lines: &[String], match_top: usize, match_bottom: usize) -> Vec<String> {
    let end = lines.len().saturating_sub(match_bottom);
    lines[match_top..end].to_vec()
}

fn split_conflict(conflict: &Conflict) -> Vec<Conflict> {
    let parts_a = split_side(&conflict.bodies.a);
    let parts_base = split_side(&conflict.bodies.base);
    let parts_b = split_side(&conflict.bodies.b);
    if parts_a.len() != parts_base.len() || parts_a.len() != parts_b.len() || parts_a.len() <= 1 {
        return vec![conflict.clone()];
    }

    parts_a
        .into_iter()
        .zip(parts_base)
        .zip(parts_b)
        .map(|((a, base), b)| Conflict {
            marker_a: conflict.marker_a.clone(),
            marker_base: conflict.marker_base.clone(),
            marker_b: conflict.marker_b.clone(),
            marker_end: conflict.marker_end.clone(),
            bodies: Sides::new(a, base, b),
        })
        .collect()
}

fn split_side(lines: &[String]) -> Vec<Vec<String>> {
    let mut parts = vec![Vec::new()];
    for line in lines {
        if line.starts_with("~~~~~~~") {
            parts.push(Vec::new());
        } else {
            parts.last_mut().expect("parts is never empty").push(line.clone());
        }
    }
    parts
}

fn conflict_to_lines(conflict: &Conflict) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(conflict.marker_a.text.clone());
    lines.extend(conflict.bodies.a.clone());
    lines.push(conflict.marker_base.text.clone());
    lines.extend(conflict.bodies.base.clone());
    lines.push(conflict.marker_b.text.clone());
    lines.extend(conflict.bodies.b.clone());
    lines.push(conflict.marker_end.text.clone());
    lines
}

fn lines_to_string(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::chunks_to_string;
    use crate::types::SrcContent;

    fn make_conflict(a: &[&str], base: &[&str], b: &[&str]) -> Conflict {
        Conflict {
            marker_a: SrcContent::new(1, "<<<<<<< HEAD".to_string()),
            marker_base: SrcContent::new(2, "||||||| base".to_string()),
            marker_b: SrcContent::new(3, "=======".to_string()),
            marker_end: SrcContent::new(4, ">>>>>>> branch".to_string()),
            bodies: Sides::new(
                a.iter().map(|line| line.to_string()).collect(),
                base.iter().map(|line| line.to_string()).collect(),
                b.iter().map(|line| line.to_string()).collect(),
            ),
        }
    }

    #[test]
    fn test_default_options_match_upstream() {
        let opts = ResolveOptions::default();
        assert!(opts.trivial);
        assert!(opts.reduce);
        assert!(opts.line_endings);
        assert!(opts.split_markers);
        assert!(!opts.indentation);
        assert!(!opts.lines_added_around);
        assert_eq!(opts.untabify, None);
    }

    #[test]
    fn test_partial_reduction_preserves_context_outside_markers() {
        let input = "\
<<<<<<< HEAD
common
ours
tail
||||||| ancestor
common
base
tail
=======
common
theirs
tail
>>>>>>> branch
";
        let chunks = parse_conflicts(input).unwrap();
        let (resolved, stats) = resolve_chunks(chunks);

        assert_eq!(stats.partially_resolved, 1);
        assert_eq!(
            chunks_to_string(&resolved),
            "common\n<<<<<<< HEAD\nours\n||||||| ancestor\nbase\n=======\ntheirs\n>>>>>>> branch\ntail\n"
        );
    }

    #[test]
    fn test_indentation_is_opt_in() {
        let c = make_conflict(
            &["        foo", "        bar"],
            &["    foo", "    bar"],
            &["    foo", "    baz"],
        );

        assert!(matches!(resolve_conflict(&c), Resolution::Unchanged));

        let opts = ResolveOptions {
            indentation: true,
            ..ResolveOptions::default()
        };
        assert!(matches!(
            resolve_conflict_with_options(&c, &opts),
            Resolution::Resolved(text) if text == "        foo\n        baz\n"
        ));
    }

    #[test]
    fn test_lines_added_around_option() {
        let c = make_conflict(
            &["before", "base"],
            &["base"],
            &["base", "after"],
        );

        assert!(matches!(resolve_conflict(&c), Resolution::Unchanged));

        let opts = ResolveOptions {
            lines_added_around: true,
            ..ResolveOptions::default()
        };
        assert!(matches!(
            resolve_conflict_with_options(&c, &opts),
            Resolution::Resolved(text) if text == "before\nbase\nafter\n"
        ));
    }

    #[test]
    fn test_split_markers_are_enabled_by_default() {
        let input = "\
<<<<<<< HEAD
base
~~~~~~~
base
||||||| base
base
~~~~~~~
base
=======
theirs
~~~~~~~
base
>>>>>>> branch
";
        let chunks = parse_conflicts(input).unwrap();
        let (resolved, stats) = resolve_chunks(chunks);
        assert_eq!(stats.resolved, 1);
        assert_eq!(chunks_to_string(&resolved), "theirs\nbase\n");
    }

    #[test]
    fn test_untabify_option() {
        let c = make_conflict(&["Hello\tBooya"], &["Hello   Booya"], &["Hello   Booya"]);
        let opts = ResolveOptions {
            untabify: Some(4),
            ..ResolveOptions::default()
        };
        assert!(matches!(
            resolve_conflict_with_options(&c, &opts),
            Resolution::Resolved(text) if text == "Hello   Booya\n"
        ));
    }
}
