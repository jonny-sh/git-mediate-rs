use crate::types::{Conflict, ConflictBody, ConflictSides};

use super::ResolveOptions;
use super::strategies::resolve_value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PreprocessedConflict(Conflict);

impl PreprocessedConflict {
    pub(super) fn new(conflict: &Conflict, options: &ResolveOptions) -> Self {
        let mut conflict = conflict.clone();

        if let Some(tabsize) = options.untabify {
            conflict = map_conflict_lines(&conflict, |line| untabify_str(line, tabsize));
        }
        if options.line_endings {
            conflict = line_break_fix(&conflict);
        }

        Self(conflict)
    }

    pub(super) fn as_conflict(&self) -> &Conflict {
        &self.0
    }
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
    if endings.ours == endings.base && endings.base == endings.theirs {
        return conflict.clone();
    }

    match target_line_ending(&endings) {
        Some(LineEnding::Lf) => map_conflict_lines(conflict, normalize_lf),
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

fn normalize_lf(line: &str) -> String {
    line.trim_end_matches('\r').to_string()
}

fn target_line_ending(endings: &ConflictSides<LineEnding>) -> Option<LineEnding> {
    match resolve_value(&endings.ours, &endings.base, &endings.theirs) {
        Some(LineEnding::Lf) => return Some(LineEnding::Lf),
        Some(LineEnding::Crlf) => return Some(LineEnding::Crlf),
        _ => {}
    }

    let known = [endings.ours, endings.base, endings.theirs]
        .into_iter()
        .filter(|ending| matches!(ending, LineEnding::Lf | LineEnding::Crlf))
        .count();

    if known != 2 {
        return None;
    }

    match endings.base {
        LineEnding::Lf | LineEnding::Crlf => Some(endings.base),
        _ => match endings.ours {
            LineEnding::Lf | LineEnding::Crlf => Some(endings.ours),
            _ => match endings.theirs {
                LineEnding::Lf | LineEnding::Crlf => Some(endings.theirs),
                _ => None,
            },
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineEnding {
    Lf,
    Crlf,
    Mixed,
    Unknown,
}

fn infer_line_endings(lines: &ConflictBody) -> LineEnding {
    let mut current = None;
    for line in lines {
        if line.is_empty() {
            continue;
        }

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

    current.unwrap_or(LineEnding::Unknown)
}
