use std::ops::Deref;

/// A typed wrapper around the lines contained in one side of a conflict.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConflictBody(Vec<String>);

impl ConflictBody {
    pub fn new(lines: Vec<String>) -> Self {
        Self(lines)
    }

    pub fn lines(&self) -> &[String] {
        &self.0
    }

    pub fn into_lines(self) -> Vec<String> {
        self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn push(&mut self, line: String) {
        self.0.push(line);
    }

    pub fn extend<I>(&mut self, lines: I)
    where
        I: IntoIterator<Item = String>,
    {
        self.0.extend(lines);
    }
}

impl From<Vec<String>> for ConflictBody {
    fn from(lines: Vec<String>) -> Self {
        Self::new(lines)
    }
}

impl FromIterator<String> for ConflictBody {
    fn from_iter<T: IntoIterator<Item = String>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl IntoIterator for ConflictBody {
    type Item = String;
    type IntoIter = std::vec::IntoIter<String>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a ConflictBody {
    type Item = &'a String;
    type IntoIter = std::slice::Iter<'a, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl AsRef<[String]> for ConflictBody {
    fn as_ref(&self) -> &[String] {
        self.lines()
    }
}

impl Deref for ConflictBody {
    type Target = [String];

    fn deref(&self) -> &Self::Target {
        self.lines()
    }
}

/// A three-sided container representing (ours, base, theirs) in a merge conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictSides<T> {
    pub ours: T,
    pub base: T,
    pub theirs: T,
}

impl<T> ConflictSides<T> {
    pub fn new(ours: T, base: T, theirs: T) -> Self {
        Self { ours, base, theirs }
    }

    pub fn map<U>(self, f: impl Fn(T) -> U) -> ConflictSides<U> {
        ConflictSides {
            ours: f(self.ours),
            base: f(self.base),
            theirs: f(self.theirs),
        }
    }

    pub fn as_ref(&self) -> ConflictSides<&T> {
        ConflictSides {
            ours: &self.ours,
            base: &self.base,
            theirs: &self.theirs,
        }
    }

    pub fn zip_with<U, V>(
        self,
        other: ConflictSides<U>,
        f: impl Fn(T, U) -> V,
    ) -> ConflictSides<V> {
        ConflictSides {
            ours: f(self.ours, other.ours),
            base: f(self.base, other.base),
            theirs: f(self.theirs, other.theirs),
        }
    }
}

impl<T: PartialEq> ConflictSides<T> {
    /// Returns true if all three sides are equal.
    pub fn all_equal(&self) -> bool {
        self.ours == self.base && self.base == self.theirs
    }
}

/// A line of source content paired with its original line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SrcContent {
    pub line_number: usize,
    pub text: String,
}

impl SrcContent {
    pub fn new(line_number: usize, text: String) -> Self {
        Self { line_number, text }
    }
}

/// The four marker lines that bound a diff3 merge conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictMarkers {
    pub ours: SrcContent,
    pub base: SrcContent,
    pub separator: SrcContent,
    pub theirs: SrcContent,
}

impl ConflictMarkers {
    pub fn new(
        ours: SrcContent,
        base: SrcContent,
        separator: SrcContent,
        theirs: SrcContent,
    ) -> Self {
        Self {
            ours,
            base,
            separator,
            theirs,
        }
    }
}

/// A single merge conflict with markers and three-sided content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub markers: ConflictMarkers,
    pub bodies: ConflictSides<ConflictBody>,
}

impl Conflict {
    /// Returns the line number of the first marker (`<<<<<<<`).
    pub fn start_line(&self) -> usize {
        self.markers.ours.line_number
    }

    /// Returns the line number of the last marker (`>>>>>>>`).
    pub fn end_line(&self) -> usize {
        self.markers.theirs.line_number
    }

    pub fn to_conflict_lines(&self) -> ConflictBody {
        let mut out = Vec::new();
        out.push(self.markers.ours.text.clone());
        out.extend(self.bodies.ours.lines().iter().cloned());
        out.push(self.markers.base.text.clone());
        out.extend(self.bodies.base.lines().iter().cloned());
        out.push(self.markers.separator.text.clone());
        out.extend(self.bodies.theirs.lines().iter().cloned());
        out.push(self.markers.theirs.text.clone());
        ConflictBody::from(out)
    }

    /// Reconstructs the full conflict text with markers.
    pub fn to_conflict_text(&self) -> String {
        let mut out = self.to_conflict_lines().lines().join("\n");
        out.push('\n');
        out
    }
}

/// A parsed chunk of a file: either plain text or a conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chunk {
    /// Non-conflicting text (lines without conflict markers).
    Plain(String),
    /// A merge conflict.
    Conflict(Conflict),
}

/// The result of attempting to resolve a single conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Conflict fully resolved to this text.
    Resolved(String),
    /// Conflict partially reduced (some matching prefix/suffix stripped).
    PartiallyReduced(Conflict),
    /// No resolution found; conflict unchanged.
    Unchanged,
}

/// Aggregate result counts for a file.
#[derive(Debug, Clone, Default)]
pub struct FileResult {
    pub resolved: usize,
    pub partially_resolved: usize,
    pub failed: usize,
}

impl FileResult {
    pub fn is_fully_resolved(&self) -> bool {
        self.partially_resolved == 0 && self.failed == 0
    }

    pub fn total_conflicts(&self) -> usize {
        self.resolved + self.partially_resolved + self.failed
    }
}

/// Status of an unmerged file from `git status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnmergedStatus {
    /// Both modified (UU)
    BothModified,
    /// Deleted by us (DU)
    DeletedByUs,
    /// Deleted by them (UD)
    DeletedByThem,
}

/// An unmerged file as reported by git.
#[derive(Debug, Clone)]
pub struct UnmergedFile {
    pub status: UnmergedStatus,
    pub path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(lines: &[&str]) -> ConflictBody {
        ConflictBody::from(
            lines
                .iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn test_conflict_text_roundtrip_uses_renamed_markers() {
        let conflict = Conflict {
            markers: ConflictMarkers::new(
                SrcContent::new(12, "<<<<<<< HEAD".to_string()),
                SrcContent::new(15, "||||||| ancestor".to_string()),
                SrcContent::new(18, "=======".to_string()),
                SrcContent::new(21, ">>>>>>> branch".to_string()),
            ),
            bodies: ConflictSides::new(body(&["ours"]), body(&["base"]), body(&["theirs"])),
        };

        assert_eq!(
            conflict.to_conflict_text(),
            "<<<<<<< HEAD\nours\n||||||| ancestor\nbase\n=======\ntheirs\n>>>>>>> branch\n"
        );
    }

    #[test]
    fn test_conflict_line_numbers_use_conflict_markers() {
        let conflict = Conflict {
            markers: ConflictMarkers::new(
                SrcContent::new(7, "<<<<<<< HEAD".to_string()),
                SrcContent::new(10, "||||||| ancestor".to_string()),
                SrcContent::new(13, "=======".to_string()),
                SrcContent::new(15, ">>>>>>> branch".to_string()),
            ),
            bodies: ConflictSides::new(body(&[]), body(&[]), body(&[])),
        };

        assert_eq!(conflict.start_line(), 7);
        assert_eq!(conflict.end_line(), 15);
    }
}
