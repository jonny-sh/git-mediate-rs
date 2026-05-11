use crate::types::{ConflictBody, ConflictSides};

use super::ResolveOptions;

impl ConflictSides<ConflictBody> {
    pub(super) fn resolve(&self, options: &ResolveOptions) -> Option<ConflictBody> {
        if options.indentation {
            self.resolve_with_indentation(options)
        } else {
            self.resolve_without_indentation(options)
        }
    }

    fn resolve_with_indentation(&self, options: &ResolveOptions) -> Option<ConflictBody> {
        let prefixes = ConflictSides::new(
            self.ours.indentation_prefix(),
            self.base.indentation_prefix(),
            self.theirs.indentation_prefix(),
        );
        let prefix = prefixes.resolve_value()?;

        let unprefixed = ConflictSides::new(
            self.ours.strip_prefix(&prefixes.ours),
            self.base.strip_prefix(&prefixes.base),
            self.theirs.strip_prefix(&prefixes.theirs),
        );

        let resolved = unprefixed.resolve_without_indentation(options)?;
        Some(
            resolved
                .into_iter()
                .map(|line| {
                    if line.is_empty() {
                        line
                    } else {
                        format!("{prefix}{line}")
                    }
                })
                .collect(),
        )
    }

    fn resolve_without_indentation(&self, options: &ResolveOptions) -> Option<ConflictBody> {
        if options.trivial {
            if let Some(body) = self.resolve_value() {
                return Some(body);
            }
        }

        if options.lines_added_around {
            let mut candidates = Vec::new();
            if let Some(lines) = self.ours.added_around(&self.base, &self.theirs) {
                candidates.push(lines);
            }
            if let Some(lines) = self.theirs.added_around(&self.base, &self.ours) {
                candidates.push(lines);
            }
            if candidates.len() == 1 {
                return candidates.into_iter().next();
            }
        }

        None
    }
}

impl<T: Eq + Clone> ConflictSides<T> {
    pub(super) fn resolve_value(&self) -> Option<T> {
        if self.ours == self.base {
            Some(self.theirs.clone())
        } else if self.theirs == self.base {
            Some(self.ours.clone())
        } else if self.ours == self.theirs {
            Some(self.ours.clone())
        } else {
            None
        }
    }
}

pub(super) fn resolve_value<T: Eq + Clone>(ours: &T, base: &T, theirs: &T) -> Option<T> {
    ConflictSides::new(ours, base, theirs)
        .resolve_value()
        .cloned()
}

impl ConflictBody {
    fn added_around(&self, base: &ConflictBody, other: &ConflictBody) -> Option<ConflictBody> {
        if self.len() < base.len() || other.len() < base.len() {
            return None;
        }

        if self.lines()[self.len() - base.len()..] != *base.lines()
            || other.lines()[..base.len()] != *base.lines()
        {
            return None;
        }

        let mut out = self.lines().to_vec();
        out.extend_from_slice(&other.lines()[base.len()..]);
        Some(ConflictBody::from(out))
    }

    fn indentation_prefix(&self) -> String {
        let common = self.common_string_prefixes();
        common.chars().take_while(|c| *c == ' ').collect()
    }

    fn common_string_prefixes(&self) -> String {
        let mut iter = self.iter();
        let Some(first) = iter.next() else {
            return String::new();
        };

        let mut prefix = first.clone();
        for line in iter {
            prefix = common_string_prefix(&prefix, line);
            if prefix.is_empty() {
                break;
            }
        }
        prefix
    }

    fn strip_prefix(&self, prefix: &str) -> ConflictBody {
        self.iter()
            .map(|line| line.strip_prefix(prefix).unwrap_or(line).to_string())
            .collect()
    }
}

fn common_string_prefix(left: &str, right: &str) -> String {
    left.chars()
        .zip(right.chars())
        .take_while(|(left, right)| left == right)
        .map(|(ch, _)| ch)
        .collect()
}
