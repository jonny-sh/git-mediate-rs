use crate::types::{Conflict, ConflictBody, ConflictSides};

use super::ResolveOptions;
use super::strategies::resolve_value;

pub(super) fn preprocess_conflict(mut conflict: Conflict, options: &ResolveOptions) -> Conflict {
    if let Some(tabsize) = options.untabify {
        conflict = map_conflict_lines(&conflict, |line| untabify_str(line, tabsize));
    }
    if options.line_endings {
        conflict = line_break_fix(&conflict);
    }
    conflict
}

fn map_conflict_lines(conflict: &Conflict, f: impl Fn(&str) -> String) -> Conflict {
    Conflict {
        markers: conflict.markers.clone(),
        bodies: ConflictSides::new(
            map_body_lines(&conflict.bodies.ours, &f),
            map_body_lines(&conflict.bodies.base, &f),
            map_body_lines(&conflict.bodies.theirs, &f),
        ),
    }
}

fn map_body_lines(body: &ConflictBody, f: &impl Fn(&str) -> String) -> ConflictBody {
    body.iter().map(|line| f(line)).collect()
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
    let endings = ConflictSides::new(
        infer_line_endings(&conflict.bodies.ours),
        infer_line_endings(&conflict.bodies.base),
        infer_line_endings(&conflict.bodies.theirs),
    );
    if conflict.bodies.ours.iter().any(|line| line.is_empty())
        || conflict.bodies.base.iter().any(|line| line.is_empty())
        || conflict.bodies.theirs.iter().any(|line| line.is_empty())
        || (endings.ours == endings.base && endings.base == endings.theirs)
    {
        return conflict.clone();
    }

    match resolve_value(&endings.ours, &endings.base, &endings.theirs) {
        Some(LineEnding::Lf) => {
            map_conflict_lines(conflict, |line| line.trim_end_matches('\r').to_string())
        }
        Some(LineEnding::Crlf) => map_conflict_lines(conflict, |line| {
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

fn infer_line_endings(lines: &ConflictBody) -> LineEnding {
    if lines.is_empty() {
        return LineEnding::Mixed;
    }

    let mut current = None;
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
