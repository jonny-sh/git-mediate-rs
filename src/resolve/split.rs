use crate::types::{Conflict, ConflictBody, ConflictSides};

pub(super) struct ConflictSplitter;

impl ConflictSplitter {
    pub(super) fn split(conflict: &Conflict) -> Option<Vec<Conflict>> {
        let ours_parts = split_body(&conflict.bodies.ours);
        let base_parts = split_body(&conflict.bodies.base);
        let theirs_parts = split_body(&conflict.bodies.theirs);

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
                .map(|((ours, base), theirs)| Conflict {
                    markers: conflict.markers.clone(),
                    bodies: ConflictSides::new(ours, base, theirs),
                })
                .collect(),
        )
    }
}

fn split_body(body: &ConflictBody) -> Vec<ConflictBody> {
    let mut parts = vec![Vec::new()];

    for line in body {
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
