use crate::types::{Chunk, Conflict, Sides, SrcContent};

const MARKER_OURS: &str = "<<<<<<<";
const MARKER_BASE: &str = "|||||||";
const MARKER_SEP: &str = "=======";
const MARKER_THEIRS: &str = ">>>>>>>";

/// Errors that can occur during conflict parsing.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("line {line}: expected '{expected}' marker but found '{found}'")]
    UnexpectedMarker {
        line: usize,
        expected: String,
        found: String,
    },
    #[error("line {line}: unterminated conflict (started at line {start_line})")]
    UnterminatedConflict { line: usize, start_line: usize },
    #[error("file has no diff3 base markers (|||||||). Set merge.conflictstyle=diff3")]
    NoDiff3Style,
}

/// Parse a file's content into a sequence of plain text and conflict chunks.
///
/// The file must use diff3 conflict style (with `|||||||` base markers).
pub fn parse_conflicts(content: &str) -> Result<Vec<Chunk>, ParseError> {
    let lines: Vec<&str> = content.lines().collect();
    let mut chunks = Vec::new();
    let mut plain_start = 0;
    let mut i = 0;
    let mut found_any_conflict = false;
    let mut has_base_marker = true;

    while i < lines.len() {
        if lines[i].starts_with(MARKER_OURS) {
            found_any_conflict = true;

            // Flush accumulated plain text
            if i > plain_start {
                let plain = lines[plain_start..i].join("\n");
                chunks.push(Chunk::Plain(plain + "\n"));
            }

            let marker_a = SrcContent::new(i + 1, lines[i].to_string());
            let conflict_start = i;
            i += 1;

            // Collect side A lines until ||||||| or =======
            let mut body_a = Vec::new();
            while i < lines.len()
                && !lines[i].starts_with(MARKER_BASE)
                && !lines[i].starts_with(MARKER_SEP)
            {
                body_a.push(lines[i].to_string());
                i += 1;
            }

            if i >= lines.len() {
                return Err(ParseError::UnterminatedConflict {
                    line: i,
                    start_line: conflict_start + 1,
                });
            }

            // Parse base marker (optional — if missing, base is empty)
            let (marker_base, body_base) = if lines[i].starts_with(MARKER_BASE) {
                let marker = SrcContent::new(i + 1, lines[i].to_string());
                i += 1;

                let mut body = Vec::new();
                while i < lines.len() && !lines[i].starts_with(MARKER_SEP) {
                    body.push(lines[i].to_string());
                    i += 1;
                }
                (marker, body)
            } else {
                // No diff3 base marker — synthesize an empty base
                has_base_marker = false;
                let marker = SrcContent::new(i + 1, format!("{MARKER_BASE} (no base)"));
                (marker, Vec::new())
            };

            if i >= lines.len() {
                return Err(ParseError::UnterminatedConflict {
                    line: i,
                    start_line: conflict_start + 1,
                });
            }

            // Parse separator (=======)
            let marker_b = SrcContent::new(i + 1, lines[i].to_string());
            i += 1;

            // Collect side B lines until >>>>>>>
            let mut body_b = Vec::new();
            while i < lines.len() && !lines[i].starts_with(MARKER_THEIRS) {
                body_b.push(lines[i].to_string());
                i += 1;
            }

            if i >= lines.len() {
                return Err(ParseError::UnterminatedConflict {
                    line: i,
                    start_line: conflict_start + 1,
                });
            }

            let marker_end = SrcContent::new(i + 1, lines[i].to_string());
            i += 1;

            chunks.push(Chunk::Conflict(Conflict {
                marker_a,
                marker_base,
                marker_b,
                marker_end,
                bodies: Sides::new(body_a, body_base, body_b),
            }));

            plain_start = i;
        } else {
            i += 1;
        }
    }

    // Flush trailing plain text
    if plain_start < lines.len() {
        let plain = lines[plain_start..].join("\n");
        // Preserve trailing newline if original content had one
        if content.ends_with('\n') {
            chunks.push(Chunk::Plain(plain + "\n"));
        } else {
            chunks.push(Chunk::Plain(plain));
        }
    } else if plain_start == lines.len() && content.ends_with('\n') && !chunks.is_empty() {
        // Content ended exactly at the end of a conflict marker line,
        // but the original file had a trailing newline — nothing extra to add
    }

    if found_any_conflict && !has_base_marker {
        return Err(ParseError::NoDiff3Style);
    }

    Ok(chunks)
}

/// Reconstruct file content from parsed chunks (resolved or not).
pub fn chunks_to_string(chunks: &[Chunk]) -> String {
    let mut out = String::new();
    for chunk in chunks {
        match chunk {
            Chunk::Plain(text) => out.push_str(text),
            Chunk::Conflict(conflict) => out.push_str(&conflict.to_conflict_text()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_conflict() {
        let input = "\
before
<<<<<<< HEAD
ours
||||||| base
original
=======
theirs
>>>>>>> branch
after
";
        let chunks = parse_conflicts(input).unwrap();
        assert_eq!(chunks.len(), 3);

        match &chunks[0] {
            Chunk::Plain(text) => assert_eq!(text, "before\n"),
            other => panic!("expected Plain, got {:?}", other),
        }

        match &chunks[1] {
            Chunk::Conflict(c) => {
                assert_eq!(c.bodies.a, vec!["ours"]);
                assert_eq!(c.bodies.base, vec!["original"]);
                assert_eq!(c.bodies.b, vec!["theirs"]);
                assert_eq!(c.start_line(), 2);
                assert_eq!(c.end_line(), 8);
            }
            other => panic!("expected Conflict, got {:?}", other),
        }

        match &chunks[2] {
            Chunk::Plain(text) => assert_eq!(text, "after\n"),
            other => panic!("expected Plain, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_multiple_conflicts() {
        let input = "\
<<<<<<< HEAD
a1
||||||| base
b1
=======
c1
>>>>>>> branch
middle
<<<<<<< HEAD
a2
||||||| base
b2
=======
c2
>>>>>>> branch
";
        let chunks = parse_conflicts(input).unwrap();
        assert_eq!(chunks.len(), 3);

        assert!(matches!(&chunks[0], Chunk::Conflict(_)));
        assert!(matches!(&chunks[1], Chunk::Plain(_)));
        assert!(matches!(&chunks[2], Chunk::Conflict(_)));
    }

    #[test]
    fn test_parse_no_diff3_style() {
        let input = "\
<<<<<<< HEAD
ours
=======
theirs
>>>>>>> branch
";
        let result = parse_conflicts(input);
        assert!(matches!(result, Err(ParseError::NoDiff3Style)));
    }

    #[test]
    fn test_parse_empty_sides() {
        let input = "\
<<<<<<< HEAD
||||||| base
original line
=======
>>>>>>> branch
";
        let chunks = parse_conflicts(input).unwrap();
        assert_eq!(chunks.len(), 1);

        match &chunks[0] {
            Chunk::Conflict(c) => {
                assert!(c.bodies.a.is_empty());
                assert_eq!(c.bodies.base, vec!["original line"]);
                assert!(c.bodies.b.is_empty());
            }
            other => panic!("expected Conflict, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_multiline_bodies() {
        let input = "\
<<<<<<< HEAD
line 1a
line 2a
line 3a
||||||| base
line 1b
line 2b
=======
line 1c
>>>>>>> branch
";
        let chunks = parse_conflicts(input).unwrap();
        match &chunks[0] {
            Chunk::Conflict(c) => {
                assert_eq!(c.bodies.a, vec!["line 1a", "line 2a", "line 3a"]);
                assert_eq!(c.bodies.base, vec!["line 1b", "line 2b"]);
                assert_eq!(c.bodies.b, vec!["line 1c"]);
            }
            other => panic!("expected Conflict, got {:?}", other),
        }
    }

    #[test]
    fn test_roundtrip() {
        let input = "\
before
<<<<<<< HEAD
ours
||||||| base
original
=======
theirs
>>>>>>> branch
after
";
        let chunks = parse_conflicts(input).unwrap();
        let output = chunks_to_string(&chunks);
        assert_eq!(input, output);
    }

    #[test]
    fn test_roundtrip_no_trailing_newline() {
        let input = "\
before
<<<<<<< HEAD
ours
||||||| base
original
=======
theirs
>>>>>>> branch
after";
        let chunks = parse_conflicts(input).unwrap();
        let output = chunks_to_string(&chunks);
        assert_eq!(input, output);
    }

    #[test]
    fn test_unterminated_conflict() {
        let input = "\
<<<<<<< HEAD
ours
||||||| base
original
";
        let result = parse_conflicts(input);
        assert!(matches!(result, Err(ParseError::UnterminatedConflict { .. })));
    }

    #[test]
    fn test_plain_only() {
        let input = "just some normal text\nwith multiple lines\n";
        let chunks = parse_conflicts(input).unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], Chunk::Plain(_)));
    }
}
