mod internal;
mod normalize;
mod split;
mod strategies;
mod window;

use crate::types::{Chunk, Conflict, ConflictBody, FileResult, Resolution};

use internal::{reduce_delete_modify_common, reduce_internal_common};
use normalize::PreprocessedConflict;
use window::ConflictWindow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveOptions {
    pub trivial: bool,
    pub reduce: bool,
    pub untabify: Option<usize>,
    pub line_endings: bool,
    pub lines_added_around: bool,
    pub reduce_deleted: bool,
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
            reduce_deleted: false,
            split_markers: true,
            indentation: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolverOutcome {
    Resolved(ConflictBody),
    Reduced(ConflictWindow),
    ReducedConflict(Conflict),
    ReducedChunks(Vec<Chunk>),
    Unchanged,
}

impl ResolverOutcome {
    fn into_resolution(self, template: &Conflict) -> Resolution {
        match self {
            Self::Resolved(body) => Resolution::Resolved(body.to_text()),
            Self::Reduced(window) => {
                Resolution::PartiallyReduced(window.reduced_conflict(template))
            }
            Self::ReducedConflict(conflict) => Resolution::PartiallyReduced(conflict),
            Self::ReducedChunks(_) => Resolution::Unchanged,
            Self::Unchanged => Resolution::Unchanged,
        }
    }

    fn into_chunks(self, template: &Conflict) -> Vec<Chunk> {
        match self {
            Self::Resolved(body) => body_to_chunks(body),
            Self::Reduced(window) => window.reduced_chunks(template),
            Self::ReducedConflict(conflict) => vec![Chunk::Conflict(conflict)],
            Self::ReducedChunks(chunks) => chunks,
            Self::Unchanged => vec![Chunk::Conflict(template.clone())],
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
            Self::ReducedConflict(_) => FileResult {
                resolved: 0,
                partially_resolved: 1,
                failed: 0,
            },
            Self::ReducedChunks(_) => FileResult {
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
    conflict.resolve()
}

pub fn resolve_conflict_with_options(conflict: &Conflict, options: &ResolveOptions) -> Resolution {
    conflict.resolve_with_options(options)
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
                let (chunk_stats, resolved) = resolve_conflict_chunks(&conflict, options);
                stats.resolved += chunk_stats.resolved;
                stats.partially_resolved += chunk_stats.partially_resolved;
                stats.failed += chunk_stats.failed;
                append_chunks(&mut result, resolved);
            }
        }
    }

    (result, stats)
}

fn resolve_conflict_chunks(
    conflict: &Conflict,
    options: &ResolveOptions,
) -> (FileResult, Vec<Chunk>) {
    let parts = if options.split_markers {
        conflict
            .split_marked_parts()
            .unwrap_or_else(|| vec![conflict.clone()])
    } else {
        vec![conflict.clone()]
    };

    let mut aggregate = FileResult::default();
    let mut combined = Vec::new();

    for part in &parts {
        let (part_stats, chunks) = resolve_conflict_part_chunks(part, options);
        aggregate.resolved += part_stats.resolved;
        aggregate.partially_resolved += part_stats.partially_resolved;
        aggregate.failed += part_stats.failed;
        append_chunks(&mut combined, chunks);
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

fn resolve_conflict_part_chunks(
    conflict: &Conflict,
    options: &ResolveOptions,
) -> (FileResult, Vec<Chunk>) {
    let processed = conflict.preprocess(options);
    let template = processed.as_conflict();
    let outcome = processed.resolve(options);
    let is_resolved = matches!(&outcome, ResolverOutcome::Resolved(_));
    let mut part_stats = outcome.file_result();
    let mut chunks = outcome.into_chunks(template);

    if options.reduce && !is_resolved && reduce_internal_common_chunks(&mut chunks) {
        part_stats = FileResult {
            resolved: 0,
            partially_resolved: 1,
            failed: 0,
        };
    }

    (part_stats, chunks)
}

fn reduce_internal_common_chunks(chunks: &mut Vec<Chunk>) -> bool {
    let original = std::mem::take(chunks);
    let mut changed = false;
    let mut reduced_chunks = Vec::new();

    for chunk in original {
        match chunk {
            Chunk::Plain(text) => append_chunk(&mut reduced_chunks, Chunk::Plain(text)),
            Chunk::Conflict(conflict) => {
                if let Some(reduced) = reduce_internal_common(&conflict) {
                    append_chunks(&mut reduced_chunks, reduced);
                    changed = true;
                } else {
                    append_chunk(&mut reduced_chunks, Chunk::Conflict(conflict));
                }
            }
        }
    }

    *chunks = reduced_chunks;
    changed
}

fn body_to_chunks(body: ConflictBody) -> Vec<Chunk> {
    let text = body.to_text();
    if text.is_empty() {
        Vec::new()
    } else {
        vec![Chunk::Plain(text)]
    }
}

fn append_chunks(result: &mut Vec<Chunk>, chunks: Vec<Chunk>) {
    for chunk in chunks {
        append_chunk(result, chunk);
    }
}

fn append_chunk(result: &mut Vec<Chunk>, chunk: Chunk) {
    match chunk {
        Chunk::Plain(text) if text.is_empty() => {}
        Chunk::Plain(text) => {
            if let Some(Chunk::Plain(previous)) = result.last_mut() {
                previous.push_str(&text);
            } else {
                result.push(Chunk::Plain(text));
            }
        }
        Chunk::Conflict(conflict) => result.push(Chunk::Conflict(conflict)),
    }
}

impl Conflict {
    pub fn resolve(&self) -> Resolution {
        self.resolve_with_options(&ResolveOptions::default())
    }

    pub fn resolve_with_options(&self, options: &ResolveOptions) -> Resolution {
        let processed = self.preprocess(options);
        processed
            .resolve(options)
            .into_resolution(processed.as_conflict())
    }

    fn preprocess(&self, options: &ResolveOptions) -> PreprocessedConflict {
        PreprocessedConflict::new(self, options)
    }
}

impl PreprocessedConflict {
    fn resolve(&self, options: &ResolveOptions) -> ResolverOutcome {
        let conflict = self.as_conflict();

        if let Some(body) = conflict.bodies.resolve(options) {
            return ResolverOutcome::Resolved(body);
        }

        if !options.reduce {
            return ResolverOutcome::Unchanged;
        }

        if conflict.is_delete_modify() {
            if !options.reduce_deleted {
                return ResolverOutcome::Unchanged;
            }
            if let Some(reduced) = reduce_delete_modify_common(conflict) {
                if reduced.is_empty() {
                    return ResolverOutcome::Resolved(ConflictBody::default());
                }
                if let [Chunk::Conflict(conflict)] = reduced.as_slice() {
                    return ResolverOutcome::ReducedConflict(conflict.clone());
                }
                return ResolverOutcome::ReducedChunks(reduced);
            }
            return ResolverOutcome::Unchanged;
        }

        let window = ConflictWindow::from_conflict(conflict);
        if !window.is_reduced() {
            return ResolverOutcome::Unchanged;
        }

        if let Some(body) = window.core().resolve(options) {
            return ResolverOutcome::Resolved(window.surround(body));
        }

        ResolverOutcome::Reduced(window)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{chunks_to_string, parse_conflicts};
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
        assert!(!opts.reduce_deleted);
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

        assert!(matches!(conflict.resolve(), Resolution::Unchanged));

        let opts = ResolveOptions {
            indentation: true,
            ..ResolveOptions::default()
        };
        assert!(matches!(
            conflict.resolve_with_options(&opts),
            Resolution::Resolved(text) if text == "        foo\n        baz\n"
        ));
    }

    #[test]
    fn test_lines_added_around_option() {
        let conflict = make_conflict(&["before", "base"], &["base"], &["base", "after"]);

        assert!(matches!(conflict.resolve(), Resolution::Unchanged));

        let opts = ResolveOptions {
            lines_added_around: true,
            ..ResolveOptions::default()
        };
        assert!(matches!(
            conflict.resolve_with_options(&opts),
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
            conflict.resolve_with_options(&opts),
            Resolution::Resolved(text) if text == "Hello   Booya\n"
        ));
    }

    #[test]
    fn test_line_ending_fix_resolves_deleted_theirs() {
        let conflict = make_conflict(
            &["fn main() {\r", "\r", "    println!(\"hi\");\r", "}\r"],
            &["fn main() {", "", "    println!(\"hi\");", "}"],
            &[],
        );

        assert!(matches!(
            conflict.resolve(),
            Resolution::Resolved(text) if text.is_empty()
        ));
    }

    #[test]
    fn test_line_ending_fix_resolves_deleted_ours() {
        let conflict = make_conflict(&[], &["fn main() {}\r"], &["fn main() {}"]);

        assert!(matches!(
            conflict.resolve(),
            Resolution::Resolved(text) if text.is_empty()
        ));
    }

    #[test]
    fn test_line_ending_fix_runs_before_reduction_with_empty_side() {
        let conflict = make_conflict(&["shared\r", "ours\r"], &[], &["shared", "theirs"]);

        assert!(matches!(
            conflict.resolve(),
            Resolution::PartiallyReduced(reduced)
                if reduced.bodies.ours == body(&["ours\r"])
                    && reduced.bodies.base == body(&[])
                    && reduced.bodies.theirs == body(&["theirs\r"])
        ));
    }

    #[test]
    fn test_line_ending_fix_reduces_delete_modify_conflict() {
        let conflict = make_conflict(
            &["shared\r", "ours\r", "tail\r"],
            &["shared", "base", "tail"],
            &[],
        );
        let opts = ResolveOptions {
            reduce_deleted: true,
            ..ResolveOptions::default()
        };

        assert!(matches!(
            conflict.resolve_with_options(&opts),
            Resolution::PartiallyReduced(reduced)
                if reduced.bodies.ours == body(&["ours"])
                    && reduced.bodies.base == body(&["base"])
                    && reduced.bodies.theirs == body(&[])
        ));
    }

    #[test]
    fn test_chunk_resolution_uses_direct_conflict_pipeline_for_structured_reduction() {
        let conflict = make_conflict(
            &["shared\r", "ours\r", "tail\r"],
            &["shared", "base", "tail"],
            &[],
        );
        let opts = ResolveOptions {
            reduce_deleted: true,
            ..ResolveOptions::default()
        };

        let direct = conflict.resolve_with_options(&opts);
        let (resolved, stats) = resolve_chunks_with_options(vec![Chunk::Conflict(conflict)], &opts);

        assert_eq!(stats.partially_resolved, 1);
        assert!(matches!(
            (direct, resolved.as_slice()),
            (Resolution::PartiallyReduced(direct), [Chunk::Conflict(chunk)])
                if direct == *chunk
        ));
    }

    #[test]
    fn test_delete_modify_reduction_does_not_auto_resolve_reduced_core() {
        let conflict = make_conflict(&["shared", "added"], &["shared"], &[]);
        let opts = ResolveOptions {
            reduce_deleted: true,
            ..ResolveOptions::default()
        };

        assert!(matches!(
            conflict.resolve_with_options(&opts),
            Resolution::PartiallyReduced(reduced)
                if reduced.bodies.ours == body(&["added"])
                    && reduced.bodies.base == body(&[])
                    && reduced.bodies.theirs == body(&[])
        ));
    }

    #[test]
    fn test_delete_modify_reduction_is_opt_in() {
        let conflict = make_conflict(&["shared", "added"], &["shared"], &[]);

        assert!(matches!(conflict.resolve(), Resolution::Unchanged));
    }

    #[test]
    fn test_reduce_deleted_treats_indentation_as_common() {
        let conflict = make_conflict(
            &["        fn method() {", "            call();", "        }"],
            &["    fn method() {", "        call();", "    }"],
            &[],
        );
        let opts = ResolveOptions {
            reduce_deleted: true,
            ..ResolveOptions::default()
        };

        assert!(matches!(
            conflict.resolve_with_options(&opts),
            Resolution::Resolved(text) if text.is_empty()
        ));
    }

    #[test]
    fn test_reduce_deleted_treats_whitespace_runs_as_common() {
        let conflict = make_conflict(
            &["let value = compute(1,  2);", "", "return value;"],
            &["let   value = compute(1, 2);", "   ", "return   value;"],
            &[],
        );
        let opts = ResolveOptions {
            reduce_deleted: true,
            ..ResolveOptions::default()
        };

        assert!(matches!(
            conflict.resolve_with_options(&opts),
            Resolution::Resolved(text) if text.is_empty()
        ));
    }

    #[test]
    fn test_internal_common_block_splits_delete_modify_conflict() {
        let input = "\
<<<<<<< HEAD
ours-start
shared-a
shared-b
ours-end
||||||| base
base-start
shared-a
shared-b
base-end
=======
>>>>>>> branch
";
        let chunks = parse_conflicts(input).unwrap();
        let opts = ResolveOptions {
            reduce_deleted: true,
            ..ResolveOptions::default()
        };
        let (resolved, stats) = resolve_chunks_with_options(chunks, &opts);

        assert_eq!(stats.partially_resolved, 1);
        assert_eq!(
            resolved
                .iter()
                .filter(|chunk| matches!(chunk, Chunk::Conflict(_)))
                .count(),
            2
        );
        assert_eq!(
            chunks_to_string(&resolved),
            "\
<<<<<<< HEAD
ours-start
||||||| base
base-start
=======
>>>>>>> branch
<<<<<<< HEAD
ours-end
||||||| base
base-end
=======
>>>>>>> branch
"
        );
    }

    #[test]
    fn test_internal_common_block_uses_normalized_line_endings() {
        let input = "\
<<<<<<< HEAD
ours-start\r
shared\r
ours-end\r
||||||| base
base-start
shared
base-end
=======
>>>>>>> branch
";
        let chunks = parse_conflicts(input).unwrap();
        let opts = ResolveOptions {
            reduce_deleted: true,
            ..ResolveOptions::default()
        };
        let (resolved, stats) = resolve_chunks_with_options(chunks, &opts);

        assert_eq!(stats.partially_resolved, 1);
        assert_eq!(
            chunks_to_string(&resolved),
            "\
<<<<<<< HEAD
ours-start
||||||| base
base-start
=======
>>>>>>> branch
<<<<<<< HEAD
ours-end
||||||| base
base-end
=======
>>>>>>> branch
"
        );
    }

    #[test]
    fn test_internal_common_block_reduction_does_not_reparse_rendered_text() {
        let input = "\
<<<<<<< HEAD
ours-start
<<<<<<< shared-marker
ours-end
||||||| base
=======
theirs-start
<<<<<<< shared-marker
theirs-end
>>>>>>> branch
";
        let chunks = parse_conflicts(input).unwrap();
        let (resolved, stats) = resolve_chunks(chunks);

        assert_eq!(stats.partially_resolved, 1);
        assert_eq!(
            chunks_to_string(&resolved),
            "\
<<<<<<< HEAD
ours-start
||||||| base
=======
theirs-start
>>>>>>> branch
<<<<<<< shared-marker
<<<<<<< HEAD
ours-end
||||||| base
=======
theirs-end
>>>>>>> branch
"
        );
    }
}
