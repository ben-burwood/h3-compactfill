//! R-Tree of the Polygon's Edge Segments
//!
//! Required for Coarse Containment:
//! - whether any polygon edge falls within the cell's axis-aligned bounding box
//! - whether the centre is inside the polygon
//!
//! Doing both against the raw ring lists is `O(edges)` per Cell
//! R-tree makes them `O(log edges)` and — unlike geo's `relate` — allocation-free per query.
//!
//! This is NOT EXACT, geo relate should be used for exactness.

use geo::{MultiPolygon, Point};
use rstar::primitives::Line;
use rstar::{AABB, RTree};

use crate::map::CoordMap;

/// Polygon Edge; `rstar`'s [`Line`]
/// Provides `RTreeObject` (envelope for the AABB query), plus `from`/`to` for the Ray Cast.
type Edge = Line<[f64; 2]>;

/// R-tree over every Edge of a [`MultiPolygon`], exterior and holes.
pub(crate) struct PolygonIndex {
    rtree: RTree<Edge>,
    /// Right Edge of the Geometry, so a ray cast can stop at a finite x
    max_x: f64,
    bbox_min: [f64; 2],
    bbox_max: [f64; 2],
}

impl PolygonIndex {
    /// Flattens `polygons` to an edge list, normalising each vertex into the
    /// seam-free frame as it goes — so the caller need not materialise a whole
    /// second, normalised [`MultiPolygon`] just to feed the R-tree.
    pub(crate) fn build(polygons: &MultiPolygon, coord_map: &CoordMap) -> Self {
        let mut edges = Vec::new();
        let mut max_x = f64::NEG_INFINITY;
        let (mut min_x, mut min_y, mut max_y) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for polygon in polygons {
            for ring in std::iter::once(polygon.exterior()).chain(polygon.interiors()) {
                for line in ring.lines() {
                    let start = coord_map.normalise_coord(line.start.x, line.start.y);
                    let end = coord_map.normalise_coord(line.end.x, line.end.y);
                    let a = [start.x, start.y];
                    let b = [end.x, end.y];
                    max_x = max_x.max(a[0]).max(b[0]);
                    min_x = min_x.min(a[0]).min(b[0]);
                    min_y = min_y.min(a[1]).min(b[1]);
                    max_y = max_y.max(a[1]).max(b[1]);
                    edges.push(Line::new(a, b));
                }
            }
        }
        Self {
            rtree: RTree::bulk_load(edges),
            max_x,
            bbox_min: [min_x, min_y],
            bbox_max: [max_x, max_y],
        }
    }

    /// Whether the box `[min, max]` is disjoint from the whole Geometry's Bounds - CHEAP
    pub(crate) fn aabb_outside_bounds(&self, min: [f64; 2], max: [f64; 2]) -> bool {
        max[0] < self.bbox_min[0]
            || min[0] > self.bbox_max[0]
            || max[1] < self.bbox_min[1]
            || min[1] > self.bbox_max[1]
    }

    /// Whether any Polygon Edge's Bounding envelope intersects the axis-aligned box`[min, max]`.
    /// Conservative: an edge whose envelope (not the segment itself) clips the
    /// box still counts, which only ever yields an extra `Straddle`.
    pub(crate) fn any_edge_in_aabb(&self, min: [f64; 2], max: [f64; 2]) -> bool {
        let aabb = AABB::from_corners(min, max);
        self.rtree
            .locate_in_envelope_intersecting(&aabb)
            .next()
            .is_some()
    }

    /// Whether `point` is inside the Polygon (holes excluded) -
    /// casts a ray in `+x` and counts Edge crossings.
    ///
    /// Only correct for a `point` that is not on an Edge.
    pub(crate) fn contains_point(&self, point: Point) -> bool {
        let q = [point.x(), point.y()];
        // A zero-height strip from the point rightwards: exactly the edges whose
        // y-span includes the ray and that lie at or to the right of the point.
        let strip = AABB::from_corners(q, [self.max_x + 1.0, q[1]]);
        let mut inside = false;
        for edge in self.rtree.locate_in_envelope_intersecting(&strip) {
            if ray_crosses(q, edge.from, edge.to) {
                inside = !inside;
            }
        }
        inside
    }
}

/// Does the `+x` ray from `p` cross segment `a`–`b`?
/// Uses the half-open rule (an edge owns its lower endpoint) so a ray through a vertex is counted once.
fn ray_crosses(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> bool {
    if (a[1] > p[1]) == (b[1] > p[1]) {
        return false; // Both endpoints on the same side of the ray.
    }
    let t = (p[1] - a[1]) / (b[1] - a[1]);
    let x = a[0] + t * (b[0] - a[0]);
    x > p[0]
}
