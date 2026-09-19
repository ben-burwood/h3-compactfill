/// Bounding Disk Classification of a coarse Cell against the Polygon
pub enum CoarseClassification {
    Inside,
    Outside,
    Straddle,
}

/// First projection-valid resolution:
/// Handoff Resolution defines where projections are valid.
///
/// Cells at this resolution and finer are classified with R-tree.
/// Cells coarser than this resolution use a bbox classifier.
pub const HANDOFF_RES: usize = 2;
