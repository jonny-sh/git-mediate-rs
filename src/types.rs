/// A three-sided container representing (ours, base, theirs) in a merge conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sides<T> {
    pub a: T,
    pub base: T,
    pub b: T,
}

impl<T> Sides<T> {
    pub fn new(a: T, base: T, b: T) -> Self {
        Self { a, base, b }
    }

    pub fn map<U>(self, f: impl Fn(T) -> U) -> Sides<U> {
        Sides {
            a: f(self.a),
            base: f(self.base),
            b: f(self.b),
        }
    }

    pub fn as_ref(&self) -> Sides<&T> {
        Sides {
            a: &self.a,
            base: &self.base,
            b: &self.b,
        }
    }

    pub fn zip_with<U, V>(self, other: Sides<U>, f: impl Fn(T, U) -> V) -> Sides<V> {
        Sides {
            a: f(self.a, other.a),
            base: f(self.base, other.base),
            b: f(self.b, other.b),
        }
    }
}

impl<T: PartialEq> Sides<T> {
    /// Returns true if all three sides are equal.
    pub fn all_equal(&self) -> bool {
        self.a == self.base && self.base == self.b
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

/// A single merge conflict with markers and three-sided content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The `<<<<<<<` marker line
    pub marker_a: SrcContent,
    /// The `|||||||` marker line
    pub marker_base: SrcContent,
    /// The `=======` marker line
    pub marker_b: SrcContent,
    /// The `>>>>>>>` marker line
    pub marker_end: SrcContent,
    /// The content lines for each side (ours, base, theirs)
    pub bodies: Sides<Vec<String>>,
}

impl Conflict {
    /// Returns the line number of the first marker (`<<<<<<<`).
    pub fn start_line(&self) -> usize {
        self.marker_a.line_number
    }

    /// Returns the line number of the last marker (`>>>>>>>`).
    pub fn end_line(&self) -> usize {
        self.marker_end.line_number
    }

    /// Reconstructs the full conflict text with markers.
    pub fn to_conflict_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&self.marker_a.text);
        out.push('\n');
        for line in &self.bodies.a {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(&self.marker_base.text);
        out.push('\n');
        for line in &self.bodies.base {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(&self.marker_b.text);
        out.push('\n');
        for line in &self.bodies.b {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(&self.marker_end.text);
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
