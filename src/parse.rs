use crate::types::{Chunk, Conflict, ConflictBody, ConflictMarkers, ConflictSides, SrcContent};

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

            if i > plain_start {
                let plain = lines[plain_start..i].join("\n");
                chunks.push(Chunk::Plain(plain + "\n"));
            }

            let ours_marker = SrcContent::new(i + 1, lines[i].to_string());
            let conflict_start = i;
            i += 1;

            let mut ours_body = Vec::new();
            while i < lines.len()
                && !lines[i].starts_with(MARKER_BASE)
                && !lines[i].starts_with(MARKER_SEP)
            {
                ours_body.push(lines[i].to_string());
                i += 1;
            }

            if i >= lines.len() {
                return Err(ParseError::UnterminatedConflict {
                    line: i,
                    start_line: conflict_start + 1,
                });
            }

            let (base_marker, base_body) = if lines[i].starts_with(MARKER_BASE) {
                let marker = SrcContent::new(i + 1, lines[i].to_string());
                i += 1;

                let mut body = Vec::new();
                while i < lines.len() && !lines[i].starts_with(MARKER_SEP) {
                    body.push(lines[i].to_string());
                    i += 1;
                }
                (marker, body)
            } else {
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

            let separator_marker = SrcContent::new(i + 1, lines[i].to_string());
            i += 1;

            let mut theirs_body = Vec::new();
            while i < lines.len() && !lines[i].starts_with(MARKER_THEIRS) {
                theirs_body.push(lines[i].to_string());
                i += 1;
            }

            if i >= lines.len() {
                return Err(ParseError::UnterminatedConflict {
                    line: i,
                    start_line: conflict_start + 1,
                });
            }

            let theirs_marker = SrcContent::new(i + 1, lines[i].to_string());
            i += 1;

            chunks.push(Chunk::Conflict(Conflict {
                markers: ConflictMarkers::new(
                    ours_marker,
                    base_marker,
                    separator_marker,
                    theirs_marker,
                ),
                bodies: ConflictSides::new(
                    ConflictBody::from(ours_body),
                    ConflictBody::from(base_body),
                    ConflictBody::from(theirs_body),
                ),
            }));

            plain_start = i;
        } else {
            i += 1;
        }
    }

    if plain_start < lines.len() {
        let plain = lines[plain_start..].join("\n");
        if content.ends_with('\n') {
            chunks.push(Chunk::Plain(plain + "\n"));
        } else {
            chunks.push(Chunk::Plain(plain));
        }
    } else if plain_start == lines.len() && content.ends_with('\n') && !chunks.is_empty() {
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

    fn lines(body: &ConflictBody) -> Vec<&str> {
        body.lines().iter().map(String::as_str).collect()
    }

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
                assert_eq!(lines(&c.bodies.ours), vec!["ours"]);
                assert_eq!(lines(&c.bodies.base), vec!["original"]);
                assert_eq!(lines(&c.bodies.theirs), vec!["theirs"]);
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
                assert!(c.bodies.ours.is_empty());
                assert_eq!(lines(&c.bodies.base), vec!["original line"]);
                assert!(c.bodies.theirs.is_empty());
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
                assert_eq!(lines(&c.bodies.ours), vec!["line 1a", "line 2a", "line 3a"]);
                assert_eq!(lines(&c.bodies.base), vec!["line 1b", "line 2b"]);
                assert_eq!(lines(&c.bodies.theirs), vec!["line 1c"]);
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
        assert!(matches!(
            result,
            Err(ParseError::UnterminatedConflict { .. })
        ));
    }

    #[test]
    fn test_plain_only() {
        let input = "just some normal text\nwith multiple lines\n";
        let chunks = parse_conflicts(input).unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], Chunk::Plain(_)));
    }
}
