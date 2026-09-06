use h3o::{CellIndex, Resolution};

/// Multi-Resolution Cell List Compaction
///
/// Cells are bucketed by Resolution and processed finest → coarsest.
///
/// Within a resolution, sorting an H3 index set places every parent's children
/// consecutively (children differ only in the last direction digit),
/// so a complete group is `children_count` consecutive cells sharing a parent.
///
/// Complete groups are replaced by their parent, which is dropped into the next
/// coarser bucket and revisited there, so promotions cascade all the way up.
pub fn compact_multiresolution(cells: Vec<CellIndex>) -> Vec<CellIndex> {
    // Bucket by Resolution (index 0..=15)
    let mut by_res: [Vec<CellIndex>; 16] = Default::default();
    for cell in cells {
        by_res[usize::from(cell.resolution())].push(cell);
    }

    let mut result: Vec<CellIndex> = Vec::new();
    for res in Resolution::range(Resolution::Zero, Resolution::Fifteen).rev() {
        // Take the bucket out so we can push promoted parents into `by_res[r-1]`.
        let mut level = std::mem::take(&mut by_res[usize::from(res)]);
        if level.is_empty() {
            continue;
        }
        level.sort_unstable();
        level.dedup();

        // Resolution 0 has no parent to merge into — keep its cells as-is.
        let Some(parent_res) = res.pred() else {
            result.append(&mut level);
            continue;
        };

        let mut i = 0;
        while i < level.len() {
            let parent = level[i].parent(parent_res).expect("non-zero resolution");
            let group = usize::try_from(parent.children_count(res)).expect("small child count");
            // Count the run of consecutive cells sharing this parent.
            let mut j = i + 1;
            while j < level.len() && level[j].parent(parent_res) == Some(parent) {
                j += 1;
            }
            if j - i == group {
                by_res[usize::from(res as u8 - 1)].push(parent); // Complete → merge up.
            } else {
                result.extend_from_slice(&level[i..j]); // Incomplete → keep as-is.
            }
            i = j;
        }
    }

    result
}
