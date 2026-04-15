mod normalize;
mod split;
mod strategies;
mod window;

use crate::parse::parse_conflicts;
use crate::types::{Chunk, Conflict, ConflictBody, FileResult, Resolution};

use normalize::preprocess_conflict;
use split::ConflictSplitter;
use strategies::resolve_body;
use window::ConflictWindow;

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

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolverOutcome {
    Resolved(ConflictBody),
    Reduced(ConflictWindow),
    Unchanged,
}

impl ResolverOutcome {
    fn into_resolution(self, template: &Conflict) -> Resolution {
        match self {
            Self::Resolved(body) => Resolution::Resolved(body_to_string(&body)),
            Self::Reduced(window) => {
                Resolution::PartiallyReduced(window.reduced_conflict(template))
            }
            Self::Unchanged => Resolution::Unchanged,
        }
    }

    fn render_text(&self, template: &Conflict) -> String {
        match self {
            Self::Resolved(body) => body_to_string(body),
            Self::Reduced(window) => window.render_reduced_conflict_text(template),
            Self::Unchanged => template.to_conflict_text(),
        }
    }

    fn file_result(&self) -> FileResult {
        match self {
            Self::Resolved(_) => FileResult {
                resolved: 1,
                partially_resolved: 0,
                failed: 0,
            },
            Self::Reduced(_) => FileResult {
                resolved: 0,
                partially_resolved: 1,
                failed: 0,
            },
            Self::Unchanged => FileResult {
                resolved: 0,
                partially_resolved: 0,
                failed: 1,
            },
        }
    }
}

pub fn resolve_conflict(conflict: &Conflict) -> Resolution {
    resolve_conflict_with_options(conflict, &ResolveOptions::default())
}

pub fn resolve_conflict_with_options(conflict: &Conflict, options: &ResolveOptions) -> Resolution {
    let processed = preprocess_conflict(conflict.clone(), options);
    resolve_preprocessed_conflict(&processed, options).into_resolution(&processed)
}

pub fn resolve_chunks(chunks: Vec<Chunk>) -> (Vec<Chunk>, FileResult) {
    resolve_chunks_with_options(chunks, &ResolveOptions::default())
}

pub fn resolve_chunks_with_options(
    chunks: Vec<Chunk>,
    options: &ResolveOptions,
) -> (Vec<Chunk>, FileResult) {
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

fn resolve_conflict_text(conflict: &Conflict, options: &ResolveOptions) -> (FileResult, String) {
    let parts = if options.split_markers {
        ConflictSplitter::split(conflict).unwrap_or_else(|| vec![conflict.clone()])
    } else {
        vec![conflict.clone()]
    };

    let mut aggregate = FileResult::default();
    let mut combined = String::new();

    for part in &parts {
        let processed = preprocess_conflict(part.clone(), options);
        let outcome = resolve_preprocessed_conflict(&processed, options);
        let part_stats = outcome.file_result();

        aggregate.resolved += part_stats.resolved;
        aggregate.partially_resolved += part_stats.partially_resolved;
        aggregate.failed += part_stats.failed;
        combined.push_str(&outcome.render_text(&processed));
    }

    if parts.len() > 1 {
        aggregate = if aggregate.failed > 0 || aggregate.partially_resolved > 0 {
            FileResult {
                resolved: 0,
                partially_resolved: 1,
                failed: 0,
            }
        } else {
            FileResult {
                resolved: 1,
                partially_resolved: 0,
                failed: 0,
            }
        };
    }

    (aggregate, combined)
}

fn resolve_preprocessed_conflict(conflict: &Conflict, options: &ResolveOptions) -> ResolverOutcome {
    if let Some(body) = resolve_body(options, &conflict.bodies) {
        return ResolverOutcome::Resolved(body);
    }

    if !options.reduce {
        return ResolverOutcome::Unchanged;
    }

    let window = ConflictWindow::from_conflict(conflict);
    if !window.is_reduced() {
        return ResolverOutcome::Unchanged;
    }

    if let Some(body) = resolve_body(options, window.core()) {
        return ResolverOutcome::Resolved(window.surround(body));
    }

    ResolverOutcome::Reduced(window)
}

fn body_to_string(body: &ConflictBody) -> String {
    if body.is_empty() {
        return String::new();
    }

    let mut text = body.lines().join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::chunks_to_string;
    use crate::types::{ConflictMarkers, ConflictSides, SrcContent};

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
                SrcContent::new(4, ">>>>>>> branch".to_string()),
            ),
            bodies: ConflictSides::new(body(ours), body(base), body(theirs)),
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
    fn test_partial_reduction_handles_empty_base_body() {
        let input = "\
<<<<<<< HEAD
shared
ours
||||||| ancestor
=======
shared
theirs
>>>>>>> branch
";
        let chunks = parse_conflicts(input).unwrap();
        let (resolved, stats) = resolve_chunks(chunks);

        assert_eq!(stats.partially_resolved, 1);
        assert_eq!(
            chunks_to_string(&resolved),
            "shared\n<<<<<<< HEAD\nours\n||||||| ancestor\n=======\ntheirs\n>>>>>>> branch\n"
        );
    }

    #[test]
    fn test_partial_reduction_handles_empty_base_body_symmetrically() {
        let input = "\
<<<<<<< HEAD
shared
||||||| ancestor
=======
shared
theirs
>>>>>>> branch
";
        let chunks = parse_conflicts(input).unwrap();
        let (resolved, stats) = resolve_chunks(chunks);

        assert_eq!(stats.resolved, 1);
        assert_eq!(chunks_to_string(&resolved), "shared\ntheirs\n");
    }

    #[test]
    fn test_indentation_is_opt_in() {
        let conflict = make_conflict(
            &["        foo", "        bar"],
            &["    foo", "    bar"],
            &["    foo", "    baz"],
        );

        assert!(matches!(resolve_conflict(&conflict), Resolution::Unchanged));

        let opts = ResolveOptions {
            indentation: true,
            ..ResolveOptions::default()
        };
        assert!(matches!(
            resolve_conflict_with_options(&conflict, &opts),
            Resolution::Resolved(text) if text == "        foo\n        baz\n"
        ));
    }

    #[test]
    fn test_lines_added_around_option() {
        let conflict = make_conflict(&["before", "base"], &["base"], &["base", "after"]);

        assert!(matches!(resolve_conflict(&conflict), Resolution::Unchanged));

        let opts = ResolveOptions {
            lines_added_around: true,
            ..ResolveOptions::default()
        };
        assert!(matches!(
            resolve_conflict_with_options(&conflict, &opts),
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
    fn test_mismatched_split_markers_fall_back_to_unsplit_conflict() {
        let input = "\
<<<<<<< HEAD
ours
~~~~~~~
still-ours
||||||| base
base
=======
theirs
~~~~~~~
still-theirs
>>>>>>> branch
";
        let chunks = parse_conflicts(input).unwrap();
        let (resolved, stats) = resolve_chunks(chunks);

        assert_eq!(stats.failed, 1);
        assert_eq!(chunks_to_string(&resolved), input);
    }

    #[test]
    fn test_untabify_option() {
        let conflict = make_conflict(&["Hello\tBooya"], &["Hello   Booya"], &["Hello   Booya"]);
        let opts = ResolveOptions {
            untabify: Some(4),
            ..ResolveOptions::default()
        };
        assert!(matches!(
            resolve_conflict_with_options(&conflict, &opts),
            Resolution::Resolved(text) if text == "Hello   Booya\n"
        ));
    }
}
