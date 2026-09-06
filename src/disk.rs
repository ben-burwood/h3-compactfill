use crate::map::CoordMap;
use geo::Point;
use h3o::CellIndex;

/// Scalar on a Cells Circumradius to generate the Bounding Disk.
///
/// This includes a fair amount of buffer but gurantees that ALL of a
/// coarse Cell's Children fallwithin the Disk
pub const COARSE_DISK_MARGIN: f64 = 1.2;

/// Reduced margin for the target resolution check as we no longer care to include Children
/// Feasibly this can be anything >1 to ensure a decisive In/Out Check
pub const TARGETRESOLUTION_DISK_MARGIN: f64 = 1.02;

/// Bounding Disk Classification of a coarse Cell against the Polygon
pub enum CoarseClassification {
    Inside,
    Outside,
    Straddle,
}

/// Cell Bounding Disk
pub struct Disk {
    pub centre: Point,
    pub radius: f64,
}

/// Bounding Disk of `cell`: the cell centre and `margin ×` the greatest centre→vertex Distance
pub fn cell_disk(cell: CellIndex, margin: f64, coord_map: &CoordMap) -> Disk {
    let centre = coord_map.cellindex_centroid_point(cell);
    // TODO - This could just use a precomputed circumradius table instead of the `cell.boundary()` call
    let max_r2 = coord_map
        .cellindex_boundary(cell)
        .iter()
        .map(|ll| ((ll.lng()) - centre.x()).powi(2) + (ll.lat() - centre.y()).powi(2))
        .fold(0.0_f64, f64::max);

    Disk {
        centre,
        radius: max_r2.sqrt() * margin,
    }
}
