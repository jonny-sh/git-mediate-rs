use crate::types::{Chunk, Conflict, ConflictBody, ConflictSides};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Ours,
    Base,
    Theirs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CommonBlock {
    left: Side,
    left_start: usize,
    right: Side,
    right_start: usize,
    len: usize,
}

pub(super) fn reduce_internal_common(conflict: &Conflict) -> Option<Vec<Chunk>> {
    if conflict.is_delete_modify() {
        return None;
    }

    let reduced = internal_common_reduction(conflict);
    let changed = !matches!(reduced.as_slice(), [Chunk::Conflict(reduced)] if reduced == conflict);
    changed.then_some(reduced)
}

pub(super) fn reduce_delete_modify_common(conflict: &Conflict) -> Option<Conflict> {
    if !conflict.is_delete_modify() {
        return None;
    }

    let mut bodies = conflict.bodies.clone();

    let changed = if bodies.ours.is_empty() {
        let (base, theirs, removed_common) =
            without_deleted_common_blocks(bodies.base.lines(), bodies.theirs.lines());
        bodies.base = ConflictBody::from(base);
        bodies.theirs = ConflictBody::from(theirs);
        removed_common
    } else {
        let (ours, base, removed_common) =
            without_deleted_common_blocks(bodies.ours.lines(), bodies.base.lines());
        bodies.ours = ConflictBody::from(ours);
        bodies.base = ConflictBody::from(base);
        removed_common
    };

    changed.then(|| conflict.with_bodies(bodies))
}

fn without_deleted_common_blocks(
    left: &[String],
    right: &[String],
) -> (Vec<String>, Vec<String>, bool) {
    without_common_blocks_by(left, right, |left, right| {
        whitespace_key(left) == whitespace_key(right)
    })
}

fn whitespace_key(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn without_common_blocks_by(
    left: &[String],
    right: &[String],
    is_common: impl Fn(&str, &str) -> bool + Copy,
) -> (Vec<String>, Vec<String>, bool) {
    let Some((left_start, right_start, len)) =
        longest_common_contiguous_block_by(left, right, is_common)
    else {
        return (left.to_vec(), right.to_vec(), false);
    };

    let (mut left_before, mut right_before, _) =
        without_common_blocks_by(&left[..left_start], &right[..right_start], is_common);
    let (left_after, right_after, _) = without_common_blocks_by(
        &left[left_start + len..],
        &right[right_start + len..],
        is_common,
    );

    left_before.extend(left_after);
    right_before.extend(right_after);
    (left_before, right_before, true)
}

fn internal_common_reduction(conflict: &Conflict) -> Vec<Chunk> {
    let Some(block) = find_internal_common_block(conflict) else {
        return vec![Chunk::Conflict(conflict.clone())];
    };

    let before = split_bodies(conflict, block, Segment::Before);
    let common = common_body(conflict, block);
    let after = split_bodies(conflict, block, Segment::After);

    let mut chunks = Vec::new();
    if !before.all_empty() {
        chunks.extend(internal_common_reduction(&conflict.with_bodies(before)));
    }
    push_plain_chunk(&mut chunks, common.to_text());
    if !after.all_empty() {
        chunks.extend(internal_common_reduction(&conflict.with_bodies(after)));
    }
    chunks
}

fn push_plain_chunk(chunks: &mut Vec<Chunk>, text: String) {
    if text.is_empty() {
        return;
    }

    if let Some(Chunk::Plain(previous)) = chunks.last_mut() {
        previous.push_str(&text);
    } else {
        chunks.push(Chunk::Plain(text));
    }
}

fn find_internal_common_block(conflict: &Conflict) -> Option<CommonBlock> {
    let bodies = [
        (Side::Ours, &conflict.bodies.ours),
        (Side::Base, &conflict.bodies.base),
        (Side::Theirs, &conflict.bodies.theirs),
    ];
    let non_empty = bodies
        .into_iter()
        .filter(|(_, body)| !body.is_empty())
        .collect::<Vec<_>>();

    if non_empty.len() != 2 {
        return None;
    }

    let (left, left_body) = non_empty[0];
    let (right, right_body) = non_empty[1];
    let (left_start, right_start, len) =
        longest_common_contiguous_block(left_body.lines(), right_body.lines())?;

    Some(CommonBlock {
        left,
        left_start,
        right,
        right_start,
        len,
    })
}

fn longest_common_contiguous_block(
    left: &[String],
    right: &[String],
) -> Option<(usize, usize, usize)> {
    longest_common_contiguous_block_by(left, right, |left, right| left == right)
}

fn longest_common_contiguous_block_by(
    left: &[String],
    right: &[String],
    is_common: impl Fn(&str, &str) -> bool,
) -> Option<(usize, usize, usize)> {
    let mut lengths = vec![vec![0usize; right.len() + 1]; left.len() + 1];
    let mut best = (0usize, 0usize, 0usize);

    for left_index in (0..left.len()).rev() {
        for right_index in (0..right.len()).rev() {
            if is_common(&left[left_index], &right[right_index]) {
                let len = lengths[left_index + 1][right_index + 1] + 1;
                lengths[left_index][right_index] = len;
                if len > best.2 {
                    best = (left_index, right_index, len);
                }
            }
        }
    }

    (best.2 > 0).then_some(best)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Segment {
    Before,
    After,
}

fn split_bodies(
    conflict: &Conflict,
    block: CommonBlock,
    segment: Segment,
) -> ConflictSides<ConflictBody> {
    ConflictSides::new(
        split_body(Side::Ours, &conflict.bodies.ours, block, segment),
        split_body(Side::Base, &conflict.bodies.base, block, segment),
        split_body(Side::Theirs, &conflict.bodies.theirs, block, segment),
    )
}

fn split_body(
    side: Side,
    body: &ConflictBody,
    block: CommonBlock,
    segment: Segment,
) -> ConflictBody {
    let Some(start) = block_start_for_side(side, block) else {
        return ConflictBody::default();
    };

    match segment {
        Segment::Before => ConflictBody::from(body.lines()[..start].to_vec()),
        Segment::After => ConflictBody::from(body.lines()[start + block.len..].to_vec()),
    }
}

fn common_body(conflict: &Conflict, block: CommonBlock) -> ConflictBody {
    let body = body_for_side(conflict, block.left);
    ConflictBody::from(body.lines()[block.left_start..block.left_start + block.len].to_vec())
}

fn block_start_for_side(side: Side, block: CommonBlock) -> Option<usize> {
    if side == block.left {
        Some(block.left_start)
    } else if side == block.right {
        Some(block.right_start)
    } else {
        None
    }
}

fn body_for_side(conflict: &Conflict, side: Side) -> &ConflictBody {
    match side {
        Side::Ours => &conflict.bodies.ours,
        Side::Base => &conflict.bodies.base,
        Side::Theirs => &conflict.bodies.theirs,
    }
}
