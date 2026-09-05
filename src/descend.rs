use h3o::{CellIndex, Resolution};

use crate::disk::{COARSE_DISK_MARGIN, CoarseClassification, TARGETRESOLUTION_DISK_MARGIN};

/// Descended provides the outcome of descending into a Cell's subtree.
pub enum Descended {
    /// Either disk-accepted whole, or every child came back clean.
    Included(CellIndex),
    Pruned,
}

/// Recursively Classify `cell`, descending into Children
/// Streams maximally-compacted accepted cells to `emit` and returning whether the whole subtree collapsed to one clean cell.
///
/// Cell merges into its parent exactly when every sibling came back `Clean`,
/// and a clean cell whose group is incomplete is emitted at once because it can never merge higher —
/// its only candidate parent is already known incomplete.
///
/// Recursion depth is bounded by `target - cell.resolution() <= 15`.
pub fn descend_compact(
    cell: CellIndex,
    target: Resolution,
    classify_disk: &impl Fn(CellIndex, f64) -> CoarseClassification,
    leaf_included: &impl Fn(CellIndex) -> bool,
    emit: &mut impl FnMut(CellIndex),
) -> Descended {
    if cell.resolution() == target {
        // Leaf (Target Resolution)
        //
        // The Cell Bounding Disk need not contain any Children.
        //
        // `classify_disk` is a cheap pre-check
        let included = match classify_disk(cell, TARGETRESOLUTION_DISK_MARGIN) {
            CoarseClassification::Inside => true,
            CoarseClassification::Outside => false,
            CoarseClassification::Straddle => leaf_included(cell),
        };
        return if included {
            Descended::Included(cell)
        } else {
            Descended::Pruned
        };
    }

    // Cheap pre-check
    match classify_disk(cell, COARSE_DISK_MARGIN) {
        CoarseClassification::Inside => return Descended::Included(cell),
        CoarseClassification::Outside => return Descended::Pruned,
        CoarseClassification::Straddle => {} // no-op
    }

    let next = cell.resolution().succ().unwrap_or(target);
    // Clean children, buffered only until the group is known complete.
    // A cell has at most 7 children (6 for a pentagon), so this stack array never allocates.
    let mut clean: [CellIndex; 7] = [cell; 7];
    let mut n = 0usize;
    let mut all = true;
    for child in cell.children(next) {
        match descend_compact(child, target, classify_disk, leaf_included, emit) {
            Descended::Included(c) => {
                clean[n] = c;
                n += 1;
            }
            Descended::Pruned => all = false,
        }
    }

    if all {
        // Every child collapsed clean → merge the whole group back into `cell`
        Descended::Included(cell)
    } else {
        // Incomplete group: these clean children can never merge higher
        // stream them now and report the subtree as not collapsible
        for &c in &clean[..n] {
            emit(c);
        }
        Descended::Pruned
    }
}

/// Recursively fill `cell`'s subtree at the target resolution (non-compact).                                                      /// Emits every target-resolution cell that passes the containment test.
pub fn descend(
    cell: CellIndex,
    target: Resolution,
    classify_disk: &impl Fn(CellIndex, f64) -> CoarseClassification,
    leaf_included: &impl Fn(CellIndex) -> bool,
    emit: &mut impl FnMut(CellIndex),
) {
    if cell.resolution() == target {
        // Leaf (Target Resolution)
        //
        // The Cell Bounding Disk need not contain any Children.
        //
        // `classify_disk` is a cheap pre-check
        let included = match classify_disk(cell, TARGETRESOLUTION_DISK_MARGIN) {
            CoarseClassification::Inside => true,
            CoarseClassification::Outside => false,
            CoarseClassification::Straddle => leaf_included(cell),
        };
        if included {
            emit(cell);
        }
        return;
    }

    match classify_disk(cell, COARSE_DISK_MARGIN) {
        CoarseClassification::Inside => {
            for c in cell.children(target) {
                emit(c);
            }
        } // uncompact
        CoarseClassification::Outside => {}
        CoarseClassification::Straddle => {
            let next = cell.resolution().succ().unwrap_or(target);
            for child in cell.children(next) {
                descend(child, target, classify_disk, leaf_included, emit);
            }
        }
    }
}
