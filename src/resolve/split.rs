use crate::types::{Conflict, ConflictBody, ConflictSides};

impl Conflict {
    pub(super) fn split_marked_parts(&self) -> Option<Vec<Conflict>> {
        let ours_parts = self.bodies.ours.split_on_marker_lines();
        let base_parts = self.bodies.base.split_on_marker_lines();
        let theirs_parts = self.bodies.theirs.split_on_marker_lines();

        if ours_parts.len() != base_parts.len()
            || ours_parts.len() != theirs_parts.len()
            || ours_parts.len() <= 1
        {
            return None;
        }

        Some(
            ours_parts
                .into_iter()
                .zip(base_parts)
                .zip(theirs_parts)
                .map(|((ours, base), theirs)| {
                    self.with_bodies(ConflictSides::new(ours, base, theirs))
                })
                .collect(),
        )
    }
}

impl ConflictBody {
    fn split_on_marker_lines(&self) -> Vec<ConflictBody> {
        let mut parts = vec![Vec::new()];

        for line in self {
            if line.starts_with("~~~~~~~") {
                parts.push(Vec::new());
            } else {
                parts
                    .last_mut()
                    .expect("parts is never empty")
                    .push(line.clone());
            }
        }

        parts.into_iter().map(ConflictBody::from).collect()
    }
}
