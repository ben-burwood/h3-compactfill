/// Classification of a Coarse Cell against the Polygon
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

/// Scalar for Coarse Cell Bounding Box
/// Cover ALL descendants down to the target - H3's `CHILD_SCALE_FACTOR`
pub const COARSE_SCALE: f64 = 1.4;

/// Scalar for Target Cell
pub const TARGET_SCALE: f64 = 1.05;
