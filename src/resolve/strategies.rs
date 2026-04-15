use crate::types::{ConflictBody, ConflictSides};

use super::ResolveOptions;

pub(super) fn resolve_body(
    options: &ResolveOptions,
    sides: &ConflictSides<ConflictBody>,
) -> Option<ConflictBody> {
    if options.indentation {
        resolve_with_indentation(options, sides)
    } else {
        resolve_without_indentation(options, sides)
    }
}

pub(super) fn resolve_value<T: Eq + Clone>(ours: &T, base: &T, theirs: &T) -> Option<T> {
    if ours == base {
        Some(theirs.clone())
    } else if theirs == base {
        Some(ours.clone())
    } else if ours == theirs {
        Some(ours.clone())
    } else {
        None
    }
}

fn resolve_with_indentation(
    options: &ResolveOptions,
    sides: &ConflictSides<ConflictBody>,
) -> Option<ConflictBody> {
    let prefixes = ConflictSides::new(
        indentation_prefix(&sides.ours),
        indentation_prefix(&sides.base),
        indentation_prefix(&sides.theirs),
    );
    let prefix = resolve_value(&prefixes.ours, &prefixes.base, &prefixes.theirs)?;

    let unprefixed = ConflictSides::new(
        strip_prefix_from_body(&sides.ours, &prefixes.ours),
        strip_prefix_from_body(&sides.base, &prefixes.base),
        strip_prefix_from_body(&sides.theirs, &prefixes.theirs),
    );

    let resolved = resolve_without_indentation(options, &unprefixed)?;
    Some(
        resolved
            .into_iter()
            .map(|line| {
                if line.is_empty() {
                    line
                } else {
                    format!("{prefix}{line}")
                }
            })
            .collect(),
    )
}

fn resolve_without_indentation(
    options: &ResolveOptions,
    sides: &ConflictSides<ConflictBody>,
) -> Option<ConflictBody> {
    if options.trivial {
        if let Some(body) = resolve_value(&sides.ours, &sides.base, &sides.theirs) {
            return Some(body);
        }
    }

    if options.lines_added_around {
        let mut candidates = Vec::new();
        if let Some(lines) = added_both_sides(&sides.ours, &sides.base, &sides.theirs) {
            candidates.push(lines);
        }
        if let Some(lines) = added_both_sides(&sides.theirs, &sides.base, &sides.ours) {
            candidates.push(lines);
        }
        if candidates.len() == 1 {
            return candidates.into_iter().next();
        }
    }

    None
}

fn added_both_sides(
    left: &ConflictBody,
    base: &ConflictBody,
    right: &ConflictBody,
) -> Option<ConflictBody> {
    if left.len() < base.len() || right.len() < base.len() {
        return None;
    }

    if left.lines()[left.len() - base.len()..] != *base.lines()
        || right.lines()[..base.len()] != *base.lines()
    {
        return None;
    }

    let mut out = left.lines().to_vec();
    out.extend_from_slice(&right.lines()[base.len()..]);
    Some(ConflictBody::from(out))
}

fn indentation_prefix(lines: &ConflictBody) -> String {
    let common = common_string_prefixes(lines);
    common.chars().take_while(|c| *c == ' ').collect()
}

fn common_string_prefixes(lines: &ConflictBody) -> String {
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

fn strip_prefix_from_body(lines: &ConflictBody, prefix: &str) -> ConflictBody {
    lines
        .iter()
        .map(|line| line.strip_prefix(prefix).unwrap_or(line).to_string())
        .collect()
}

fn common_string_prefix(left: &str, right: &str) -> String {
    left.chars()
        .zip(right.chars())
        .take_while(|(left, right)| left == right)
        .map(|(ch, _)| ch)
        .collect()
}
