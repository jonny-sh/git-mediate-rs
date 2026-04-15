use crate::types::{Conflict, ConflictBody, ConflictSides};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ConflictWindow {
    prefix: ConflictBody,
    core: ConflictSides<ConflictBody>,
    suffix: ConflictBody,
}

impl ConflictWindow {
    pub(super) fn from_conflict(conflict: &Conflict) -> Self {
        let ours = conflict.bodies.ours.lines();
        let base = conflict.bodies.base.lines();
        let theirs = conflict.bodies.theirs.lines();

        let prefix_len = shared_prefix(base, ours, theirs);
        let suffix_len = shared_suffix_after_prefix(base, ours, theirs, prefix_len);

        Self {
            prefix: boundary_prefix(ours, prefix_len),
            core: ConflictSides::new(
                trimmed_body(ours, prefix_len, suffix_len),
                trimmed_body(base, prefix_len, suffix_len),
                trimmed_body(theirs, prefix_len, suffix_len),
            ),
            suffix: boundary_suffix(ours, suffix_len),
        }
    }

    pub(super) fn is_reduced(&self) -> bool {
        !self.prefix.is_empty() || !self.suffix.is_empty()
    }

    pub(super) fn core(&self) -> &ConflictSides<ConflictBody> {
        &self.core
    }

    pub(super) fn reduced_conflict(&self, template: &Conflict) -> Conflict {
        Conflict {
            markers: template.markers.clone(),
            bodies: self.core.clone(),
        }
    }

    pub(super) fn surround(&self, body: ConflictBody) -> ConflictBody {
        let mut lines = self.prefix.lines().to_vec();
        lines.extend(body);
        lines.extend(self.suffix.lines().iter().cloned());
        ConflictBody::from(lines)
    }

    pub(super) fn render_reduced_conflict_text(&self, template: &Conflict) -> String {
        let reduced = self.reduced_conflict(template);
        body_to_string(&self.surround(reduced.to_conflict_lines()))
    }
}

fn body_to_string(body: &ConflictBody) -> String {
    if body.is_empty() {
        return String::new();
    }

    let mut text = body.lines().join("\n");
    text.push('\n');
    text
}

fn shared_prefix(base: &[String], ours: &[String], theirs: &[String]) -> usize {
    if base.is_empty() {
        common_prefix_len(ours, theirs)
    } else {
        common_prefix_len(base, ours).min(common_prefix_len(base, theirs))
    }
}

fn shared_suffix_after_prefix(
    base: &[String],
    ours: &[String],
    theirs: &[String],
    prefix_len: usize,
) -> usize {
    let ours_after_prefix = &ours[prefix_len.min(ours.len())..];
    let base_after_prefix = &base[prefix_len.min(base.len())..];
    let theirs_after_prefix = &theirs[prefix_len.min(theirs.len())..];

    if base_after_prefix.is_empty() {
        common_suffix_len(ours_after_prefix, theirs_after_prefix)
    } else {
        common_suffix_len(base_after_prefix, ours_after_prefix)
            .min(common_suffix_len(base_after_prefix, theirs_after_prefix))
    }
}

fn boundary_prefix(lines: &[String], prefix_len: usize) -> ConflictBody {
    lines
        .iter()
        .take(prefix_len.min(lines.len()))
        .cloned()
        .collect()
}

fn boundary_suffix(lines: &[String], suffix_len: usize) -> ConflictBody {
    let start = lines.len().saturating_sub(suffix_len.min(lines.len()));
    ConflictBody::from(lines[start..].to_vec())
}

fn trimmed_body(lines: &[String], prefix_len: usize, suffix_len: usize) -> ConflictBody {
    let end = lines.len().saturating_sub(suffix_len);
    let start = prefix_len.min(end);
    ConflictBody::from(lines[start..end].to_vec())
}

fn common_prefix_len(left: &[String], right: &[String]) -> usize {
    left.iter()
        .zip(right.iter())
        .take_while(|(left, right)| left == right)
        .count()
}

fn common_suffix_len(left: &[String], right: &[String]) -> usize {
    left.iter()
        .rev()
        .zip(right.iter().rev())
        .take_while(|(left, right)| left == right)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ConflictMarkers, ConflictSides, SrcContent};

    fn body(lines: &[&str]) -> ConflictBody {
        ConflictBody::from(
            lines
                .iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>(),
        )
    }

    fn lines(body: &ConflictBody) -> Vec<&str> {
        body.lines().iter().map(String::as_str).collect()
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
    fn test_window_trims_shared_prefix_with_empty_base() {
        let window = ConflictWindow::from_conflict(&make_conflict(
            &["shared", "ours"],
            &[],
            &["shared", "theirs"],
        ));

        assert_eq!(lines(&window.prefix), vec!["shared"]);
        assert_eq!(lines(&window.core.ours), vec!["ours"]);
        assert!(window.core.base.is_empty());
        assert_eq!(lines(&window.core.theirs), vec!["theirs"]);
        assert!(window.suffix.is_empty());
    }

    #[test]
    fn test_window_trims_shared_suffix_with_empty_base() {
        let window = ConflictWindow::from_conflict(&make_conflict(
            &["ours", "shared"],
            &[],
            &["theirs", "shared"],
        ));

        assert!(window.prefix.is_empty());
        assert_eq!(lines(&window.core.ours), vec!["ours"]);
        assert!(window.core.base.is_empty());
        assert_eq!(lines(&window.core.theirs), vec!["theirs"]);
        assert_eq!(lines(&window.suffix), vec!["shared"]);
    }

    #[test]
    fn test_window_clamps_to_empty_core_when_everything_matches() {
        let window = ConflictWindow::from_conflict(&make_conflict(&["shared"], &[], &["shared"]));

        assert_eq!(lines(&window.prefix), vec!["shared"]);
        assert!(window.core.ours.is_empty());
        assert!(window.core.base.is_empty());
        assert!(window.core.theirs.is_empty());
        assert!(window.suffix.is_empty());
    }
}
